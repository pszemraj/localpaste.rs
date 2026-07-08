//! CLI response formatting helpers.

use localpaste_core::diff::{DiffResponse, EqualResponse};
use serde_json::Value;

fn error_message_for_response(status: reqwest::StatusCode, body: &str) -> String {
    if body.trim().is_empty() {
        return status
            .canonical_reason()
            .unwrap_or("Request failed")
            .to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or(body)
            .to_string();
    }

    body.to_string()
}

/// Returns a successful response or exits after printing the server error body.
///
/// # Arguments
/// - `res`: HTTP response returned by the server.
/// - `action`: User-facing action label for the error prefix.
///
/// # Returns
/// The original response when the status is successful.
pub(super) async fn ensure_success_or_exit(
    res: reqwest::Response,
    action: &str,
) -> reqwest::Response {
    let status = res.status();
    if status.is_success() {
        return res;
    }

    let body = match res.text().await {
        Ok(body) => body,
        Err(err) => format!("failed to read error response body: {}", err),
    };
    let message = error_message_for_response(status, &body);
    eprintln!("{} failed ({}): {}", action, status, message);
    std::process::exit(1);
}

/// Reads the required id/name fields from a paste JSON object.
///
/// # Returns
/// The paste id/name pair when both fields are present and strings.
pub(super) fn paste_id_and_name(paste: &Value) -> Option<(&str, &str)> {
    let id = paste.get("id").and_then(Value::as_str)?;
    let name = paste.get("name").and_then(Value::as_str)?;
    Some((id, name))
}

/// Formats paste summaries as JSON or the default two-column CLI table.
///
/// # Arguments
/// - `pastes`: Paste summary JSON objects returned by the API.
/// - `json`: Whether to preserve the response shape as pretty JSON.
///
/// # Returns
/// Formatted CLI output.
///
/// # Errors
/// Returns an error when JSON encoding fails or a non-JSON summary row is missing required fields.
pub(super) fn format_summary_output(pastes: &[Value], json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(pastes)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    let mut rows = Vec::with_capacity(pastes.len());
    for (index, p) in pastes.iter().enumerate() {
        let Some((id, name)) = paste_id_and_name(p) else {
            return Err(format!(
                "response item {} missing 'id' or 'name' field",
                index
            ));
        };
        rows.push(format!("{:<36} {:<30}", id, name));
    }

    Ok(rows.join("\n"))
}

/// Formats a single paste response as JSON or raw content.
///
/// # Arguments
/// - `paste`: Paste JSON object returned by the API.
/// - `json`: Whether to preserve the response shape as pretty JSON.
///
/// # Returns
/// Formatted CLI output.
///
/// # Errors
/// Returns an error when JSON encoding fails or a non-JSON response does not include string content.
pub(super) fn format_get_output(paste: &Value, json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(paste)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    paste
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "response missing 'content' field".to_string())
}

/// Formats a delete response as JSON or the default confirmation line.
///
/// # Arguments
/// - `id`: Paste id requested for deletion.
/// - `response`: Delete response JSON object returned by the API.
/// - `json`: Whether to preserve the response shape as pretty JSON.
///
/// # Returns
/// Formatted CLI output.
///
/// # Errors
/// Returns an error when JSON encoding fails.
pub(super) fn format_delete_output(
    id: &str,
    response: &Value,
    json: bool,
) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(response)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    Ok(format!("Deleted paste: {}", id))
}

/// Formats paste version summaries as JSON or the default tabular output.
///
/// # Arguments
/// - `items`: Version summary JSON objects returned by the API.
/// - `json`: Whether to preserve the response shape as pretty JSON.
///
/// # Returns
/// Formatted CLI output.
///
/// # Errors
/// Returns an error when JSON encoding fails or a non-JSON version row is missing required fields.
pub(super) fn format_versions_output(items: &[Value], json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(items)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let version_id = item
            .get("version_id_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("response item {} missing 'version_id_ms' field", index))?;
        let created_at = item
            .get("created_at")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("response item {} missing 'created_at' field", index))?;
        let len = item
            .get("len")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("response item {} missing 'len' field", index))?;
        rows.push(format!(
            "{:<16} {:<28} {} bytes",
            version_id, created_at, len
        ));
    }
    Ok(rows.join("\n"))
}

fn format_cli_diff_lines(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| line.trim_end_matches(['\r', '\n']))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Formats a diff response as JSON, a no-op message, or unified diff text.
///
/// # Arguments
/// - `diff`: Diff response returned by the API.
/// - `json`: Whether to preserve the response shape as pretty JSON.
///
/// # Returns
/// Formatted CLI output.
///
/// # Errors
/// Returns an error when JSON encoding fails.
pub(super) fn format_diff_output(diff: &DiffResponse, json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(diff)
            .map_err(|err| format!("response encoding error: {}", err));
    }
    if diff.equal {
        return Ok("No changes.".to_string());
    }
    Ok(format_cli_diff_lines(&diff.unified))
}

/// Formats an equality response as JSON or a stable human-readable token.
///
/// # Arguments
/// - `equal`: Equality response returned by the API.
/// - `json`: Whether to preserve the response shape as pretty JSON.
///
/// # Returns
/// Formatted CLI output.
///
/// # Errors
/// Returns an error when JSON encoding fails.
pub(super) fn format_equal_output(equal: &EqualResponse, json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(equal)
            .map_err(|err| format!("response encoding error: {}", err));
    }
    Ok(if equal.equal { "equal" } else { "different" }.to_string())
}

#[cfg(test)]
/// Test-only accessors for private output helpers.
pub(crate) mod test_exports {
    /// Extracts a concise error message from an HTTP status/body pair.
    ///
    /// # Arguments
    /// - `status`: HTTP status returned by the server.
    /// - `body`: Response body text.
    ///
    /// # Returns
    /// A concise CLI error message.
    pub(crate) fn error_message_for_response(status: reqwest::StatusCode, body: &str) -> String {
        super::error_message_for_response(status, body)
    }
}
