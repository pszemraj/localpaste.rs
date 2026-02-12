//! Shared deprecation helpers for legacy API pathways.

use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};

const FOLDER_DEPRECATION_WARNING: &str =
    r#"299 - "Folder APIs are deprecated; prefer tags, search, and smart filters""#;
const FOLDER_DEPRECATION_SUNSET: &str = "Fri, 31 Dec 2027 23:59:59 GMT";

/// Attach deprecation headers to responses for legacy folder-based API pathways.
pub(super) fn with_folder_deprecation_headers<R>(response: R) -> Response
where
    R: IntoResponse,
{
    let mut response = response.into_response();
    let headers = response.headers_mut();
    headers.insert("deprecation", HeaderValue::from_static("true"));
    headers.insert(
        "sunset",
        HeaderValue::from_static(FOLDER_DEPRECATION_SUNSET),
    );
    headers.insert(
        header::WARNING,
        HeaderValue::from_static(FOLDER_DEPRECATION_WARNING),
    );
    response
}

/// Emit a structured warning when deprecated folder API behavior is used.
pub(super) fn warn_folder_deprecation(pathway: &str) {
    tracing::warn!(
        target: "localpaste_server::deprecation",
        pathway = pathway,
        "Folder pathway is deprecated; prefer tag/search based organization"
    );
}
