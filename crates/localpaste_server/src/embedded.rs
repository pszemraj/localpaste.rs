//! Embedded server helper for running the API inside another process (e.g. GUI).

use crate::{resolve_bind_address, serve_router, AppError, AppState};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
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
    api_addr_path: Option<PathBuf>,
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
        let api_addr_path =
            localpaste_core::config::api_addr_file_path_for_db_path(&state.config.db_path);
        let api_addr_path_for_thread = api_addr_path.clone();
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
                let api_addr = format!("http://{}", actual_addr);
                if let Some(parent) = api_addr_path_for_thread.parent() {
                    if let Err(err) = fs::create_dir_all(parent) {
                        warn!(
                            "failed to ensure API discovery directory '{}': {}",
                            parent.display(),
                            err
                        );
                    }
                }
                if let Err(err) = fs::write(&api_addr_path_for_thread, api_addr.as_bytes()) {
                    warn!(
                        "failed to write API discovery file '{}': {}",
                        api_addr_path_for_thread.display(),
                        err
                    );
                }
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
            })
            .map_err(|err| AppError::StorageMessage(format!("failed to spawn server: {}", err)))?;

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
                    api_addr_path: Some(api_addr_path),
                })
            }
            Ok(Err(message)) => {
                let _ = shutdown_tx.send(());
                if let Some(handle) = thread_handle.take() {
                    let _ = handle.join();
                }
                Err(AppError::StorageMessage(message))
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
        if let Some(path) = self.api_addr_path.take() {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    warn!(
                        "failed to remove API discovery file '{}': {}",
                        path.display(),
                        err
                    );
                }
            }
        }
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
