//! Small UI helpers for labels and word selection.

/// Reads a boolean environment feature flag using shared core parsing rules.
///
/// # Returns
/// `true` when the named flag resolves to an enabled value.
pub(super) fn env_flag_enabled(name: &str) -> bool {
    localpaste_core::config::env_flag_enabled(name)
}

/// Formats the language label shown in the UI, falling back to auto/plain.
///
/// # Arguments
/// - `language`: Optional detected or manually selected language label.
/// - `is_manual`: Whether language was manually pinned by the user.
/// - `is_large`: Whether current content is above plain-render threshold.
///
/// # Returns
/// User-facing language label for headers and list rows.
pub(super) fn display_language_label(
    language: Option<&str>,
    is_manual: bool,
    is_large: bool,
) -> String {
    if is_large {
        return "plain".to_string();
    }
    let Some(raw) = language else {
        return if is_manual {
            "plain".to_string()
        } else {
            "auto".to_string()
        };
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return if is_manual {
            "plain".to_string()
        } else {
            "auto".to_string()
        };
    }
    let lowered = trimmed.to_ascii_lowercase();
    match lowered.as_str() {
        "plaintext" | "plain" | "text" | "txt" => "plain".to_string(),
        _ => trimmed.to_string(),
    }
}

/// Formats clipboard/export content as a fenced code block.
///
/// # Arguments
/// - `content`: Text body to wrap.
/// - `language`: Optional code-fence language hint.
///
/// # Returns
/// Markdown fenced-code representation.
pub(super) fn format_fenced_code_block(content: &str, language: Option<&str>) -> String {
    let lang = language.unwrap_or("text");
    format!("```{}\n{}\n```", lang, content)
}

/// Builds a copyable API URL for the selected paste.
///
/// Unspecified bind addresses (0.0.0.0 / ::) are rewritten to loopback so the
/// copied link is locally routable in browsers and external apps.
///
/// # Arguments
/// - `addr`: Runtime API socket address.
/// - `id`: Paste id to include in the URL path.
///
/// # Returns
/// Routable API URL string for clipboard copy.
pub(super) fn api_paste_link_for_copy(addr: std::net::SocketAddr, id: &str) -> String {
    let routed = match addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            addr.port(),
        ),
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => std::net::SocketAddr::new(
            std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            addr.port(),
        ),
        _ => addr,
    };
    format!("http://{}/api/paste/{}", routed, id)
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Returns a word-selection range (in char indices) around the given char index.
///
/// # Arguments
/// - `text`: Source text to scan.
/// - `char_index`: Cursor character index within `text`.
///
/// # Returns
/// `(start, end)` character range when a selection target exists.
///
/// # Panics
/// Panics only if internal UTF-8 boundary assumptions are violated.
pub(super) fn word_range_at(text: &str, char_index: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    let total_chars = text.chars().count();
    let target_char_index = if char_index >= total_chars {
        total_chars.saturating_sub(1)
    } else {
        char_index
    };
    let byte_index = text
        .char_indices()
        .nth(target_char_index)
        .map(|(idx, _)| idx)?;
    let mut iter = text[byte_index..].chars();
    let current = iter.next()?;
    let current_is_word = is_word_char(current);
    let mut start_byte = byte_index;
    let mut head = text[..byte_index].chars().rev();
    for ch in &mut head {
        if is_word_char(ch) == current_is_word {
            start_byte = start_byte.saturating_sub(ch.len_utf8());
        } else {
            break;
        }
    }
    let mut end_byte = byte_index + current.len_utf8();
    let mut tail = text[end_byte..].chars();
    for ch in &mut tail {
        if is_word_char(ch) == current_is_word {
            end_byte = end_byte.saturating_add(ch.len_utf8());
        } else {
            break;
        }
    }

    let start_char = text[..start_byte].chars().count();
    let selected_chars = text[start_byte..end_byte].chars().count();
    Some((start_char, start_char + selected_chars))
}

#[cfg(test)]
mod tests {
    use super::{api_paste_link_for_copy, display_language_label, format_fenced_code_block};

    #[test]
    fn format_fenced_code_block_uses_language_or_text_default() {
        assert_eq!(
            format_fenced_code_block("let x = 1;", Some("rust")),
            "```rust\nlet x = 1;\n```"
        );
        assert_eq!(
            format_fenced_code_block("print('hi')", None),
            "```text\nprint('hi')\n```"
        );
    }

    #[test]
    fn api_paste_link_for_copy_rewrites_unspecified_hosts() {
        let v4 = api_paste_link_for_copy(
            std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 38411),
            "abc",
        );
        assert_eq!(v4, "http://127.0.0.1:38411/api/paste/abc");

        let v6 = api_paste_link_for_copy(
            std::net::SocketAddr::new(std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), 38411),
            "abc",
        );
        assert_eq!(v6, "http://[::1]:38411/api/paste/abc");
    }

    #[test]
    fn display_language_label_distinguishes_auto_and_manual_plain() {
        assert_eq!(display_language_label(None, false, false), "auto");
        assert_eq!(display_language_label(None, true, false), "plain");
        assert_eq!(display_language_label(Some("txt"), false, false), "plain");
        assert_eq!(display_language_label(Some("rust"), false, false), "rust");
        assert_eq!(display_language_label(Some("rust"), false, true), "plain");
    }
}
