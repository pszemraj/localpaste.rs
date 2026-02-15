//! Magika-powered language detection.

use std::sync::{Mutex, OnceLock};

const GENERIC_LABELS: &[&str] = &["txt", "randomtxt", "unknown", "empty", "undefined"];

static MAGIKA_SESSION: OnceLock<Result<Mutex<magika::Session>, String>> = OnceLock::new();

fn session() -> Option<&'static Mutex<magika::Session>> {
    MAGIKA_SESSION
        .get_or_init(|| {
            magika::Session::new().map(Mutex::new).map_err(|err| {
                tracing::warn!("magika session init failed: {}", err);
                err.to_string()
            })
        })
        .as_ref()
        .ok()
}

pub(crate) fn prewarm() {
    let _ = session();
}

pub(crate) fn detect(content: &str) -> Option<String> {
    let session = session()?;
    let mut guard = session.lock().ok()?;
    let result = guard.identify_content_sync(content.as_bytes()).ok()?;
    let info = result.info();

    if !info.is_text {
        return None;
    }

    let label = info.label;
    if GENERIC_LABELS
        .iter()
        .any(|generic| generic.eq_ignore_ascii_case(label))
    {
        return None;
    }

    Some(label.to_string())
}
