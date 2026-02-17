//! Rope-backed virtual editor primitives.

pub(crate) mod buffer;
pub(crate) mod galley_cache;
pub(crate) mod history;
pub(crate) mod input;
pub(crate) mod state;
pub(crate) mod visual_rows;

pub(crate) use buffer::{RopeBuffer, VirtualEditDelta};
pub(crate) use galley_cache::{VirtualGalleyCache, VirtualGalleyContext};
pub(crate) use history::{EditIntent, RecordedEdit, VirtualEditorHistory};
pub(crate) use input::{commands_from_events, VirtualCommandRoute, VirtualInputCommand};
pub(crate) use state::{VirtualEditorState, WrapBoundaryAffinity};
pub(crate) use visual_rows::VisualRowLayoutCache as WrapLayoutCache;
