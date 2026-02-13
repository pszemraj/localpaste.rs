//! Rope-backed virtual editor primitives.

pub(crate) mod buffer;
pub(crate) mod history;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod state;

pub(crate) use buffer::RopeBuffer;
pub(crate) use history::{EditIntent, RecordedEdit, VirtualEditorHistory};
pub(crate) use input::{commands_from_events, VirtualCommandRoute, VirtualInputCommand};
pub(crate) use layout::WrapLayoutCache;
pub(crate) use state::VirtualEditorState;
