//! Background worker thread for database access.

use crate::backend::{CoreCmd, CoreEvent, PasteSummary};
use crossbeam_channel::{unbounded, Receiver, Sender};
use localpaste_core::Database;
use std::thread;
use tracing::error;

/// Handle for sending commands to, and receiving events from, the backend worker.
pub struct BackendHandle {
    pub cmd_tx: Sender<CoreCmd>,
    pub evt_rx: Receiver<CoreEvent>,
}

/// Spawn the backend worker thread that performs blocking database access.
///
/// All I/O stays off the UI thread; the worker replies with [`CoreEvent`] values
/// that are polled each frame.
///
/// # Returns
/// A [`BackendHandle`] containing the command sender and event receiver.
///
/// # Panics
/// Panics if the worker thread cannot be spawned.
pub fn spawn_backend(db: Database) -> BackendHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();

    thread::Builder::new()
        .name("localpaste-gui-backend".to_string())
        .spawn(move || {
            for cmd in cmd_rx.iter() {
                match cmd {
                    CoreCmd::ListAll { limit } => match db.pastes.list(limit, None) {
                        Ok(pastes) => {
                            let items = pastes.iter().map(PasteSummary::from_paste).collect();
                            let _ = evt_tx.send(CoreEvent::PasteList { items });
                        }
                        Err(err) => {
                            error!("backend list failed: {}", err);
                            let _ = evt_tx.send(CoreEvent::Error {
                                message: format!("List failed: {}", err),
                            });
                        }
                    },
                    CoreCmd::GetPaste { id } => match db.pastes.get(&id) {
                        Ok(Some(paste)) => {
                            let _ = evt_tx.send(CoreEvent::PasteLoaded { paste });
                        }
                        Ok(None) => {
                            let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                        }
                        Err(err) => {
                            error!("backend get failed: {}", err);
                            let _ = evt_tx.send(CoreEvent::Error {
                                message: format!("Get failed: {}", err),
                            });
                        }
                    },
                }
            }
        })
        .expect("spawn backend thread");

    BackendHandle { cmd_tx, evt_rx }
}
