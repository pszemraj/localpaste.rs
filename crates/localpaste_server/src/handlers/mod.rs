//! HTTP request handlers.

/// Deprecation warning helpers for legacy request pathways.
pub(crate) mod deprecation;
/// Folder-related endpoints.
pub mod folder;
/// Request normalization helpers shared across handlers.
pub(crate) mod normalize;
/// Paste-related endpoints.
pub mod paste;
