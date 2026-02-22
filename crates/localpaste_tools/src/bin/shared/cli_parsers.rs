//! Shared clap value parsers used by localpaste_tools binaries.

/// Parses a strictly positive `usize` (`> 0`).
///
/// # Returns
/// Parsed `usize` value when input is a valid positive integer.
///
/// # Errors
/// Returns an error when the value is not an integer or is `0`.
pub(crate) fn parse_positive_usize(raw: &str) -> Result<usize, String> {
    let parsed = raw
        .parse::<usize>()
        .map_err(|_| format!("invalid integer value '{}'", raw))?;
    if parsed == 0 {
        Err("value must be greater than zero".to_string())
    } else {
        Ok(parsed)
    }
}
