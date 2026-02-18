//! Shared normalization helpers for optional string request fields.

/// Normalize optional identifiers for create/list/search semantics.
///
/// Empty or whitespace-only values are treated as absent.
///
/// # Returns
/// `None` for blank input, otherwise a trimmed non-empty string.
pub(super) fn normalize_optional_for_create(value: Option<String>) -> Option<String> {
    localpaste_core::text::normalize_optional_nonempty(value)
}

/// Normalize optional identifiers for update semantics.
///
/// Empty or whitespace-only values are preserved as explicit clear markers.
///
/// # Returns
/// `None` when the field is unset, `Some("")` for explicit clear, or trimmed
/// content otherwise.
pub(super) fn normalize_optional_for_update(value: Option<String>) -> Option<String> {
    value.map(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed.to_string()
        }
    })
}
