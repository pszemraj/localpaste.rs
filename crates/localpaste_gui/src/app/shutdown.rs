//! Shutdown save-flush logic for `LocalPasteApp`.

use super::{LocalPasteApp, SaveStatus, SHUTDOWN_SAVE_FLUSH_TIMEOUT};
use std::time::{Duration, Instant};
use tracing::warn;

impl LocalPasteApp {
    pub(super) fn flush_pending_saves_for_shutdown(&mut self) {
        let content_flush_requested = self.save_status == SaveStatus::Dirty && !self.save_in_flight;
        if content_flush_requested {
            self.save_now();
            if self.save_status == SaveStatus::Dirty && !self.save_in_flight {
                warn!("Shutdown flush could not dispatch content save.");
            }
        }

        let metadata_flush_requested = self.metadata_dirty && !self.metadata_save_in_flight;
        if metadata_flush_requested {
            self.save_metadata_now();
            if self.metadata_dirty && !self.metadata_save_in_flight {
                warn!("Shutdown flush could not dispatch metadata save.");
            }
        }

        let deadline = Instant::now() + SHUTDOWN_SAVE_FLUSH_TIMEOUT;
        while (self.save_in_flight || self.metadata_save_in_flight) && Instant::now() < deadline {
            let wait_for = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25));
            if wait_for.is_zero() {
                break;
            }
            match self.backend.evt_rx.recv_timeout(wait_for) {
                Ok(event) => self.apply_event(event),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        }

        if self.save_in_flight || self.metadata_save_in_flight {
            warn!(
                save_in_flight = self.save_in_flight,
                metadata_save_in_flight = self.metadata_save_in_flight,
                timeout_ms = SHUTDOWN_SAVE_FLUSH_TIMEOUT.as_millis(),
                "Shutdown flush timeout expired with saves still in flight."
            );
        }
    }
}
