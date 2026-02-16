//! Magika-powered language detection.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const GENERIC_LABELS: &[&str] = &["txt", "randomtxt", "unknown", "empty", "undefined"];
const MAGIKA_FORCE_CPU_ENV: &str = "MAGIKA_FORCE_CPU";

static MAGIKA_SESSION: OnceLock<Result<Mutex<magika::Session>, String>> = OnceLock::new();
static MAGIKA_POISON_WARNED: AtomicBool = AtomicBool::new(false);
static MAGIKA_IDENTIFY_WARNED: AtomicBool = AtomicBool::new(false);

fn magika_force_cpu() -> bool {
    crate::config::parse_bool_env(MAGIKA_FORCE_CPU_ENV, true)
}

fn configure_ort_execution_provider() {
    if !magika_force_cpu() {
        return;
    }

    if let Err(err) = ort::init()
        .with_execution_providers([
            ort::execution_providers::CPUExecutionProvider::default().build()
        ])
        .commit()
    {
        tracing::warn!(
            "failed to configure ONNX Runtime CPU execution provider: {}; continuing with Magika defaults",
            err
        );
    }
}

fn session() -> Option<&'static Mutex<magika::Session>> {
    MAGIKA_SESSION
        .get_or_init(|| {
            configure_ort_execution_provider();
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
    let mut guard = match session.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            if !MAGIKA_POISON_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "magika session mutex was poisoned; recovering and continuing with fallback safety"
                );
            }
            poisoned.into_inner()
        }
    };
    let result = match guard.identify_content_sync(content.as_bytes()) {
        Ok(result) => result,
        Err(err) => {
            if !MAGIKA_IDENTIFY_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "magika inference failed; falling back to heuristic detection: {}",
                    err
                );
            }
            return None;
        }
    };
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

#[cfg(test)]
mod tests {
    use super::magika_force_cpu;
    use crate::env::{env_lock, EnvGuard};

    #[test]
    fn magika_force_cpu_env_defaults_to_true_when_missing() {
        let _lock = env_lock().lock().expect("env lock");
        let _unset = EnvGuard::remove("MAGIKA_FORCE_CPU");
        assert!(magika_force_cpu());
    }

    #[test]
    fn magika_force_cpu_env_accepts_falsey_values() {
        let _lock = env_lock().lock().expect("env lock");
        for value in ["0", "false", "no", "off"] {
            let _set = EnvGuard::set("MAGIKA_FORCE_CPU", value);
            assert!(!magika_force_cpu(), "value: {value}");
        }
    }
}
