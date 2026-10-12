//! Shutdown save-flush logic for `LocalPasteApp`.

use super::{
    commands_from_events, egui, LocalPasteApp, SaveStatus, VirtualInputCommand, NAV_PROBE_PASTE_ID,
    SHUTDOWN_SAVE_FLUSH_TIMEOUT,
};
use std::time::{Duration, Instant};
use tracing::warn;

const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

impl LocalPasteApp {
    /// Defers a native close until accepted input finishes in the live UI.
    ///
    /// eframe exits after the close-request frame unless it emits `CancelClose`;
    /// requesting a repaint alone cannot preserve input queued for later frames.
    /// Live replay preserves widget ownership and delivers clipboard output before
    /// saving a cut. A stalled load cancels closing without discarding its input.
    pub(super) fn finish_deferred_close(&mut self, ctx: &egui::Context) {
        let pending_input = !self.deferred_discovery_events.is_empty()
            || self.deferred_editor_input_awaits_load()
            || self
                .pending_picker_open
                .as_ref()
                .is_some_and(|opening| !opening.input_events.is_empty());
        if !pending_input {
            if self.deferred_close_started_at.take().is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if self.deferred_close_started_at.is_some()
            || ctx.input(|input| input.viewport().close_requested())
        {
            let started = self
                .deferred_close_started_at
                .get_or_insert_with(Instant::now);
            if started.elapsed() >= SHUTDOWN_SAVE_FLUSH_TIMEOUT {
                self.deferred_close_started_at = None;
                self.notify("Close canceled: waiting for pending editor input. Try again after the paste loads.");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.request_repaint_after(Duration::from_millis(25));
        }
    }

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
        // No native integration consumes this context's clipboard output after exit.
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
    /// A queued append predates any input the picker retains for the same
    /// paste, because the picker retains `Ctrl/Cmd+Shift+V` itself, so the
    /// append applies first, as it does in a frame.
    fn apply_deferred_editor_input_for_shutdown(&mut self, ctx: &egui::Context) {
        if self.reset_transition_active() || self.history_reset_flush_active() {
            return;
        }
        if self.pending_editor_paste_target_loaded() {
            self.apply_pending_editor_paste(ctx);
        }
        if let Some(events) = self.take_ready_pending_picker_input() {
            let commands = commands_from_events(&events, true);
            if commands.contains(&VirtualInputCommand::Cut) {
                // Normal window close waits for live replay. If exit is forced,
                // retain the source rather than saving a cut whose copy cannot arrive.
                warn!("Shutdown left retained cut input unapplied to preserve its source text.");
            } else if self.apply_virtual_commands(ctx, &commands).changed {
                self.mark_dirty();
            }
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

impl Drop for LocalPasteApp {
    fn drop(&mut self) {
        if let Some(id) = self.selected_id.take() {
            if id == NAV_PROBE_PASTE_ID && self.nav_probe.is_some() {
                return;
            }
            if let Err(err) = self.locks.release(&id, &self.lock_owner_id) {
                warn!("failed to release paste lock '{}' on drop: {}", id, err);
            }
        }
    }
}
