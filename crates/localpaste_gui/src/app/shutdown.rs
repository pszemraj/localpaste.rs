//! Shutdown save-flush logic for `LocalPasteApp`.

use super::{commands_from_events, egui, LocalPasteApp, SaveStatus, SHUTDOWN_SAVE_FLUSH_TIMEOUT};
use std::time::{Duration, Instant};
use tracing::warn;

const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

impl LocalPasteApp {
    /// Drains pending content/metadata saves before backend shutdown.
    ///
    /// This best-effort flush keeps dispatching and consuming backend events
    /// until timeout or steady state. Editor input accepted for a paste that is
    /// still loading counts as pending: the flush waits for the load, applies
    /// the input, and saves it with the rest.
    pub(super) fn flush_pending_saves_for_shutdown(&mut self) {
        let mut warned_content_dispatch_failure = false;
        let mut warned_metadata_dispatch_failure = false;
        let deadline = Instant::now() + SHUTDOWN_SAVE_FLUSH_TIMEOUT;
        // No frame runs after exit begins; editing only needs a context for clipboard output.
        let ctx = egui::Context::default();

        // Keep dispatching while dirty and draining while in flight: older save acks can
        // legitimately leave the editor dirty again when newer local edits exist.
        while Instant::now() < deadline {
            self.apply_deferred_editor_input_for_shutdown(&ctx);
            if self.save_status == SaveStatus::Dirty && !self.save_in_flight {
                self.save_now();
                if self.save_status == SaveStatus::Dirty
                    && !self.save_in_flight
                    && !warned_content_dispatch_failure
                {
                    warn!("Shutdown flush could not dispatch content save.");
                    warned_content_dispatch_failure = true;
                }
            }

            if self.metadata_dirty && !self.metadata_save_in_flight {
                self.save_metadata_now();
                if self.metadata_dirty
                    && !self.metadata_save_in_flight
                    && !warned_metadata_dispatch_failure
                {
                    warn!("Shutdown flush could not dispatch metadata save.");
                    warned_metadata_dispatch_failure = true;
                }
            }

            let awaiting_load = self.deferred_editor_input_awaits_load();
            let settled = !self.save_in_flight
                && !self.metadata_save_in_flight
                && self.save_status != SaveStatus::Dirty
                && !self.metadata_dirty
                && !awaiting_load;
            if settled {
                break;
            }

            // Dirty state with no in-flight requests means dispatch is currently impossible.
            if !self.save_in_flight && !self.metadata_save_in_flight && !awaiting_load {
                break;
            }

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

        // Shutdown must enqueue a final dirty snapshot even when an earlier save is still
        // in flight; relying on a late ack before timeout can otherwise drop tail edits.
        self.force_enqueue_dirty_shutdown_snapshots();

        if self.save_in_flight
            || self.metadata_save_in_flight
            || self.save_status == SaveStatus::Dirty
            || self.metadata_dirty
        {
            warn!(
                save_in_flight = self.save_in_flight,
                metadata_save_in_flight = self.metadata_save_in_flight,
                save_status = ?self.save_status,
                metadata_dirty = self.metadata_dirty,
                timeout_ms = SHUTDOWN_SAVE_FLUSH_TIMEOUT.as_millis(),
                "Shutdown flush exited with pending unsaved state."
            );
        }

        if let Err(err) = self
            .backend
            .shutdown_and_join(true, BACKEND_SHUTDOWN_TIMEOUT)
        {
            warn!(error = %err, "Backend shutdown did not complete cleanly.");
        }
    }

    /// Applies editor input that a frame would have applied once its paste loaded.
    ///
    /// Overlays no longer own the keyboard once the app exits, so only a
    /// version reset, which replaces the buffer, holds the input back.
    fn apply_deferred_editor_input_for_shutdown(&mut self, ctx: &egui::Context) {
        if self.reset_transition_active() || self.history_reset_flush_active() {
            return;
        }
        if let Some(events) = self.take_ready_pending_picker_input() {
            let commands = commands_from_events(&events, true);
            if self.apply_virtual_commands(ctx, &commands).changed {
                self.mark_dirty();
            }
        }
        if self.pending_editor_paste_target_loaded() {
            self.apply_pending_editor_paste(ctx);
        }
    }

    /// Whether accepted editor input still waits for its paste to load.
    fn deferred_editor_input_awaits_load(&self) -> bool {
        let loaded = |id: &str| {
            self.selected_paste
                .as_ref()
                .is_some_and(|paste| paste.id == id)
        };
        self.pending_editor_paste
            .as_ref()
            .is_some_and(|pending| !loaded(&pending.id))
            || self
                .pending_picker_open
                .as_ref()
                .is_some_and(|opening| !opening.input_events.is_empty() && !loaded(&opening.id))
    }

    fn force_enqueue_dirty_shutdown_snapshots(&mut self) {
        if self.save_status == SaveStatus::Dirty {
            self.save_in_flight = false;
            self.save_now();
        }
        if self.metadata_dirty {
            self.metadata_save_in_flight = false;
            self.save_metadata_now();
        }
    }
}
