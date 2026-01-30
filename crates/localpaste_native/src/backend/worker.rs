//! Background worker thread for database access.

use crate::backend::{CoreCmd, CoreEvent, PasteSummary};
use crossbeam_channel::{unbounded, Receiver, Sender};
use localpaste_core::Database;
use std::thread;
use tracing::error;

pub struct BackendHandle {
    pub cmd_tx: Sender<CoreCmd>,
    pub evt_rx: Receiver<CoreEvent>,
}

pub fn spawn_backend(db: Database) -> BackendHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();

    thread::Builder::new()
        .name("localpaste-native-backend".to_string())
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
                }
            }
        })
        .expect("spawn backend thread");

    BackendHandle { cmd_tx, evt_rx }
}
