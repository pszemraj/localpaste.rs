//! Selection-load reply ownership across editor replacements and lock handoffs.

use super::{CoreEvent, LocalPasteApp};

impl LocalPasteApp {
    /// Accepts selection-load outcomes only for the current paste and editor epoch.
    ///
    /// A revisited id may have replies queued from before its edit lock was released.
    ///
    /// # Returns
    /// `true` for matching selection loads and events unrelated to selection loading.
    pub(super) fn selection_load_is_current(&self, event: &CoreEvent) -> bool {
        let selection_load = match event {
            CoreEvent::PasteLoaded {
                paste,
                selection_epoch,
            } => Some((paste.id.as_str(), *selection_epoch)),
            CoreEvent::PasteSelectionMissing {
                id,
                selection_epoch,
            }
            | CoreEvent::PasteLoadFailed {
                id,
                selection_epoch,
                ..
            } => Some((id.as_str(), *selection_epoch)),
            _ => None,
        };
        selection_load.is_none_or(|(id, epoch)| {
            self.selected_id.as_deref() == Some(id) && self.active_buffer_epoch == epoch
        })
    }
}
