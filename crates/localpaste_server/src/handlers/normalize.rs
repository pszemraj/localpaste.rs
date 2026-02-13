//! Shared normalization helpers for optional string request fields.

/// Normalize optional identifiers for create/list/search semantics.
///
/// Empty or whitespace-only values are treated as absent.
pub(super) fn normalize_optional_for_create(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Normalize optional identifiers for update semantics.
///
/// Empty or whitespace-only values are preserved as explicit clear markers.
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
