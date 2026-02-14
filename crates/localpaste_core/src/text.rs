//! Shared text and host normalization helpers.

use std::net::IpAddr;

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

#[cfg(test)]
mod tests {
    use super::{is_loopback_host, normalize_optional_nonempty};

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
}
