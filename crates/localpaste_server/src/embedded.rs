//! Embedded server helper for running the API inside another process (e.g. GUI).

use crate::{serve_router, AppError, AppState, Config};
use std::{
    net::SocketAddr,
    sync::mpsc,
    thread::{self, JoinHandle},
};
use tokio::sync::oneshot;
use tracing::{info, warn};

/// Handle to an embedded API server running on a background thread.
pub struct EmbeddedServer {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    addr: SocketAddr,
    used_fallback: bool,
}

impl EmbeddedServer {
    /// Start the API server on a background thread.
    ///
    /// The server binds to `BIND` or `127.0.0.1:PORT` from `Config`. If the
    /// requested address is in use, it will fall back to an auto-assigned port.
    ///
    /// # Arguments
    /// - `state`: Shared application state (config, db, locks).
    /// - `allow_public`: Whether to allow cross-origin requests from any origin.
    ///
    /// # Returns
    /// A running [`EmbeddedServer`] with the bound address.
    ///
    /// # Errors
    /// Returns an error if the runtime or server socket cannot be created.
    pub fn start(state: AppState, allow_public: bool) -> Result<Self, AppError> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("localpaste-embedded-server".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(err) => {
                        let _ = ready_tx.send(Err(format!("failed to start runtime: {}", err)));
                        return;
                    }
                };

                let bind_addr = resolve_bind_address(&state.config, allow_public);
                let mut used_fallback = false;
                let listener = match rt.block_on(tokio::net::TcpListener::bind(bind_addr)) {
                    Ok(listener) => listener,
                    Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                        warn!(
                            "API bind address {} is in use; falling back to an auto port",
                            bind_addr
                        );
                        used_fallback = true;
                        let fallback_addr = SocketAddr::new(bind_addr.ip(), 0);
                        match rt.block_on(tokio::net::TcpListener::bind(fallback_addr)) {
                            Ok(listener) => listener,
                            Err(fallback_err) => {
                                let _ = ready_tx.send(Err(format!(
                                    "failed to bind server socket: {}",
                                    fallback_err
                                )));
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ =
                            ready_tx.send(Err(format!("failed to bind server socket: {}", err)));
                        return;
                    }
                };

                let actual_addr = listener.local_addr().unwrap_or(bind_addr);
                if used_fallback {
                    warn!(
                        "API listening on http://{} (auto port; {} was in use)",
                        actual_addr, bind_addr
                    );
                } else {
                    info!("API listening on http://{}", actual_addr);
                }
                let _ = ready_tx.send(Ok((actual_addr, used_fallback)));

                let shutdown = async {
                    let _ = shutdown_rx.await;
                };

                if let Err(err) = rt.block_on(serve_router(
                    listener,
                    state.clone(),
                    allow_public,
                    shutdown,
                )) {
                    warn!("server error: {}", err);
                }

                if let Err(err) = state.db.flush() {
                    warn!("failed to flush database: {}", err);
                }
            })
            .map_err(|err| AppError::DatabaseError(format!("failed to spawn server: {}", err)))?;

        let mut thread_handle = Some(thread);

        match ready_rx.recv() {
            Ok(Ok((addr, used_fallback))) => {
                if !addr.ip().is_loopback() {
                    warn!("binding to non-localhost address {}", addr);
                }
                Ok(Self {
                    shutdown: Some(shutdown_tx),
                    thread: thread_handle.take(),
                    addr,
                    used_fallback,
                })
            }
            Ok(Err(message)) => {
                let _ = shutdown_tx.send(());
                if let Some(handle) = thread_handle.take() {
                    let _ = handle.join();
                }
                Err(AppError::DatabaseError(message))
            }
            Err(_) => {
                let _ = shutdown_tx.send(());
                if let Some(handle) = thread_handle.take() {
                    let _ = handle.join();
                }
                Err(AppError::Internal)
            }
        }
    }

    /// Address the server is listening on.
    ///
    /// # Returns
    /// The bound socket address for the API.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Whether the server had to fall back to an auto-assigned port.
    ///
    /// # Returns
    /// `true` if the requested bind address was in use and an auto port was used.
    pub fn used_fallback(&self) -> bool {
        self.used_fallback
    }
}

impl Drop for EmbeddedServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn resolve_bind_address(config: &Config, allow_public: bool) -> SocketAddr {
    let default_bind = SocketAddr::from(([127, 0, 0, 1], config.port));
    let requested = match std::env::var("BIND") {
        Ok(value) => match value.trim().parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(err) => {
                warn!(
                    "Invalid BIND='{}': {}. Falling back to {}",
                    value, err, default_bind
                );
                default_bind
            }
        },
        Err(_) => default_bind,
    };

    if allow_public || requested.ip().is_loopback() {
        return requested;
    }

    warn!(
        "Non-loopback bind {} requested without ALLOW_PUBLIC_ACCESS; forcing 127.0.0.1",
        requested
    );
    SocketAddr::from(([127, 0, 0, 1], requested.port()))
}
