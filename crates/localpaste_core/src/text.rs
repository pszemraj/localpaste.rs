//! Shared text and host normalization helpers.

use std::net::IpAddr;

/// Maximum byte sample used by local text classification/detection paths.
pub(crate) const TEXT_SAMPLE_MAX_BYTES: usize = 64 * 1024;

/// Trim an optional string and drop empty values.
///
/// # Returns
/// `None` when the input is missing or whitespace-only; otherwise the trimmed
/// string.
pub fn normalize_optional_nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Return `true` when `host` is localhost or a loopback IP literal.
///
/// Supports bracketed IPv6 hosts (for example `[::1]`).
///
/// # Returns
/// `true` when `host` resolves to loopback identity (`localhost`, `127.0.0.0/8`,
/// or `::1`), otherwise `false`.
pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let normalized_host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    normalized_host
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Return a byte-limited prefix without splitting a UTF-8 codepoint.
///
/// # Arguments
/// - `content`: Input text to truncate by byte count.
/// - `max_bytes`: Maximum number of bytes to include in the returned prefix.
///
/// # Returns
/// `content` when it already fits `max_bytes`; otherwise the largest prefix at or below `max_bytes` that ends on a character boundary.
///
/// # Panics
/// Panics only if the computed boundary is invalid; the loop keeps `end` on or below a valid UTF-8 character boundary.
pub(crate) fn utf8_prefix_by_bytes(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &content[..end]
}

#[cfg(test)]
mod tests {
    use super::{is_loopback_host, normalize_optional_nonempty, utf8_prefix_by_bytes};

    #[test]
    fn normalize_optional_nonempty_trims_and_drops_blank() {
        assert_eq!(
            normalize_optional_nonempty(Some("  value  ".to_string())),
            Some("value".to_string())
        );
        assert_eq!(normalize_optional_nonempty(Some("   ".to_string())), None);
        assert_eq!(normalize_optional_nonempty(None), None);
    }

    #[test]
    fn is_loopback_host_accepts_localhost_and_loopback_ips() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host("192.168.1.20"));
    }

    #[test]
    fn utf8_prefix_by_bytes_preserves_character_boundaries() {
        let value = "abé日";

        assert_eq!(utf8_prefix_by_bytes(value, 0), "");
        assert_eq!(utf8_prefix_by_bytes(value, 1), "a");
        assert_eq!(utf8_prefix_by_bytes(value, 2), "ab");
        assert_eq!(utf8_prefix_by_bytes(value, 3), "ab");
        assert_eq!(utf8_prefix_by_bytes(value, 4), "abé");
        assert_eq!(utf8_prefix_by_bytes(value, 128), value);
        assert_eq!(utf8_prefix_by_bytes("", 4), "");
    }
}
