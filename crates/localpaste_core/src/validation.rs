//! Shared paste-domain validation helpers.

use crate::error::AppError;

/// Return the stable user-facing paste size limit message.
///
/// # Arguments
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
///
/// # Returns
/// A formatted message suitable for API and GUI error surfaces.
pub fn paste_size_limit_message(max_paste_size: usize) -> String {
    format!("Paste size exceeds maximum of {} bytes", max_paste_size)
}

/// Return a user-facing error message when paste content exceeds the byte limit.
///
/// # Arguments
/// - `content_len`: Paste content size in bytes.
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
///
/// # Returns
/// `Some(message)` when the content is too large, otherwise `None`.
pub fn paste_content_size_error(content_len: usize, max_paste_size: usize) -> Option<String> {
    (content_len > max_paste_size).then(|| paste_size_limit_message(max_paste_size))
}

/// Enforce a paste content byte-size limit from a precomputed byte length.
///
/// # Arguments
/// - `content_len`: Paste content size in bytes.
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
///
/// # Returns
/// `Ok(())` when the byte length is within the configured limit.
///
/// # Errors
/// Returns [`AppError::BadRequest`] when the content exceeds `max_paste_size`.
pub fn ensure_paste_content_size_bytes(
    content_len: usize,
    max_paste_size: usize,
) -> Result<(), AppError> {
    if let Some(message) = paste_content_size_error(content_len, max_paste_size) {
        return Err(AppError::BadRequest(message));
    }
    Ok(())
}

/// Enforce a paste content byte-size limit for UTF-8 paste text.
///
/// # Arguments
/// - `content`: Paste content text.
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
///
/// # Returns
/// `Ok(())` when the content is within the configured limit.
///
/// # Errors
/// Returns [`AppError::BadRequest`] when `content` exceeds `max_paste_size`.
pub fn ensure_paste_content_size(content: &str, max_paste_size: usize) -> Result<(), AppError> {
    ensure_paste_content_size_bytes(content.len(), max_paste_size)
}
