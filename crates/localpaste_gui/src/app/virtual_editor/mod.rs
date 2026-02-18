//! Rope-backed virtual editor primitives.

/// Rope-backed text storage and mutation delta tracking.
pub(crate) mod buffer;
/// Per-frame galley cache keyed by render geometry.
pub(crate) mod galley_cache;
/// Undo/redo stacks with coalescing and bounded memory usage.
pub(crate) mod history;
/// Event-to-command reducer for keyboard, clipboard, and IME input.
pub(crate) mod input;
/// Cursor/selection/IME interaction state independent of rendering.
pub(crate) mod state;
/// Visual-row layout cache and row/column coordinate mapping.
pub(crate) mod visual_rows;

pub(crate) use buffer::{RopeBuffer, VirtualEditDelta};
pub(crate) use galley_cache::{VirtualGalleyCache, VirtualGalleyContext};
pub(crate) use history::{EditIntent, RecordedEdit, VirtualEditorHistory};
pub(crate) use input::{commands_from_events, VirtualCommandRoute, VirtualInputCommand};
pub(crate) use state::{VirtualEditorState, WrapBoundaryAffinity};
pub(crate) use visual_rows::VisualRowLayoutCache as WrapLayoutCache;
