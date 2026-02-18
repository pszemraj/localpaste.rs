//! Unit tests for the `lpaste` CLI entrypoint module.

use super::{
    api_url, default_resolution_connect_hint, discovered_server_from_file_with_reachability,
    discovery_probe_response_looks_like_localpaste, error_message_for_response,
    format_delete_output, format_get_output, format_summary_output, normalize_server,
    paste_id_and_name, resolve_server, resolve_server_with_source, ServerResolutionSource,
};
use super::{Cli, Commands};
use clap::Parser;
use localpaste_core::config::api_addr_file_path_from_env_or_default;
use localpaste_core::env::{env_lock, EnvGuard};
use localpaste_core::{DEFAULT_CLI_SERVER_URL, DEFAULT_PORT};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

struct DiscoveryTestEnv {
    _db_path_guard: EnvGuard,
    _lp_server_guard: EnvGuard,
    discovery_path: std::path::PathBuf,
}

impl DiscoveryTestEnv {
    fn new(label: &str, lp_server: Option<&str>) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("lpaste-cli-{}-{}", label, nonce));
        let db_path = db_path.join("db");
        let db_path_string = db_path.to_string_lossy().to_string();
        let db_path_guard = EnvGuard::set("DB_PATH", db_path_string.as_str());
        let lp_server_guard = match lp_server {
            Some(value) => EnvGuard::set("LP_SERVER", value),
            None => EnvGuard::remove("LP_SERVER"),
        };

        let discovery_path = api_addr_file_path_from_env_or_default();
        if let Some(parent) = discovery_path.parent() {
            std::fs::create_dir_all(parent).expect("create discovery dir");
        }

        Self {
            _db_path_guard: db_path_guard,
            _lp_server_guard: lp_server_guard,
            discovery_path,
        }
    }

    fn write_discovery(&self, value: &str) {
        std::fs::write(&self.discovery_path, value).expect("write discovery");
    }
}

impl Drop for DiscoveryTestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.discovery_path);
    }
}

fn with_discovery_env<T>(
    label: &str,
    lp_server: Option<&str>,
    test: impl FnOnce(&DiscoveryTestEnv) -> T,
) -> T {
    let _lock = env_lock().lock().expect("env lock");
    let env = DiscoveryTestEnv::new(label, lp_server);
    test(&env)
}

fn bind_reachable_discovery(env: &DiscoveryTestEnv) -> (String, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let discovered = format!("http://{}", listener.local_addr().expect("listener addr"));
    env.write_discovery(discovered.as_str());
    (discovered, listener)
}

struct LocalpasteProbeServer {
    shutdown_tx: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl LocalpasteProbeServer {
    fn new(listener: TcpListener) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        listener
            .set_nonblocking(true)
            .expect("set listener non-blocking");
        let worker = thread::spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
                    let mut request_buf = [0_u8; 1024];
                    let _ = stream.read(&mut request_buf);
                    let body = "[]";
                    let response = format!(
                        "{}{}",
                        concat!(
                            "HTTP/1.1 200 OK\r\n",
                            "Content-Type: application/json\r\n",
                            "X-Content-Type-Options: nosniff\r\n",
                            "X-Frame-Options: DENY\r\n",
                            "X-LocalPaste-Server: 1\r\n",
                            "Content-Length: "
                        ),
                        body.len()
                    );
                    let response = format!("{}\r\nConnection: close\r\n\r\n{}", response, body);
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        });
        Self {
            shutdown_tx,
            worker: Some(worker),
        }
    }
}

impl Drop for LocalpasteProbeServer {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn bind_localpaste_discovery(env: &DiscoveryTestEnv) -> (String, LocalpasteProbeServer) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let discovered = format!("http://{}", listener.local_addr().expect("listener addr"));
    env.write_discovery(discovered.as_str());
    let server = LocalpasteProbeServer::new(listener);
    (discovered, server)
}

#[derive(Clone, Copy)]
enum DiscoveryKind {
    Localpaste,
    ReachableNonLocalpaste,
}

fn assert_resolve_server_falls_back_to_default(label: &str, discovery: &str) {
    with_discovery_env(label, None, |env| {
        env.write_discovery(discovery);
        assert_eq!(resolve_server(None), DEFAULT_CLI_SERVER_URL);
    });
}

#[test]
fn normalize_server_matrix() {
    let cases = [
        (
            DEFAULT_CLI_SERVER_URL.to_string(),
            format!("http://127.0.0.1:{}", DEFAULT_PORT),
        ),
        (
            format!("https://localhost:{}", DEFAULT_PORT),
            format!("https://localhost:{}", DEFAULT_PORT),
        ),
        (
            format!("http://127.0.0.1:{}/", DEFAULT_PORT),
            format!("http://127.0.0.1:{}", DEFAULT_PORT),
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(normalize_server(input), expected);
    }
}

#[test]
fn default_cli_server_url_uses_default_port_constant() {
    assert_eq!(
        DEFAULT_CLI_SERVER_URL,
        format!("http://localhost:{}", DEFAULT_PORT)
    );
}

#[test]
fn error_message_for_response_matrix_covers_json_reason_and_passthrough() {
    let cases = [
        (
            reqwest::StatusCode::NOT_FOUND,
            r#"{"error":"Not found"}"#,
            "Not found",
        ),
        (reqwest::StatusCode::BAD_REQUEST, "   ", "Bad Request"),
        (
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "raw failure body",
            "raw failure body",
        ),
    ];

    for (status, body, expected) in cases {
        assert_eq!(error_message_for_response(status, body), expected);
    }
}

#[test]
fn paste_id_and_name_requires_both_fields() {
    let cases = [
        (
            serde_json::json!({
                "id": "abc123",
                "name": "demo"
            }),
            Some(("abc123", "demo")),
        ),
        (
            serde_json::json!({
                "id": "abc123"
            }),
            None,
        ),
    ];
    for (paste, expected) in cases {
        assert_eq!(paste_id_and_name(&paste), expected);
    }
}

#[test]
fn json_output_helpers_preserve_payload_shape() {
    let pastes = vec![serde_json::json!({
        "id": "abc123",
        "name": "demo"
    })];
    let paste = serde_json::json!({
        "id": "abc123",
        "name": "demo",
        "content": "hello"
    });
    let response = serde_json::json!({ "success": true });

    let summary_rendered =
        format_summary_output(&pastes, true).expect("summary json output should render");
    let summary_parsed: serde_json::Value =
        serde_json::from_str(&summary_rendered).expect("rendered summary should be valid json");
    assert_eq!(summary_parsed[0]["id"], "abc123");
    assert_eq!(summary_parsed[0]["name"], "demo");

    let get_rendered = format_get_output(&paste, true).expect("get json output should render");
    let get_parsed: serde_json::Value =
        serde_json::from_str(&get_rendered).expect("rendered get should be valid json");
    assert_eq!(get_parsed["content"], "hello");

    let delete_rendered =
        format_delete_output("abc123", &response, true).expect("delete json output should render");
    let delete_parsed: serde_json::Value =
        serde_json::from_str(&delete_rendered).expect("rendered delete should be valid json");
    assert_eq!(delete_parsed["success"], true);
}

#[test]
fn api_url_matrix_covers_encoding_and_base_path_append() {
    let cases = [
        (
            format!("http://127.0.0.1:{}", DEFAULT_PORT),
            ["api", "paste", "id/with?reserved#chars"],
            format!(
                "http://127.0.0.1:{}/api/paste/id%2Fwith%3Freserved%23chars",
                DEFAULT_PORT
            ),
        ),
        (
            format!("http://127.0.0.1:{}/base", DEFAULT_PORT),
            ["api", "paste", "abc123"],
            format!("http://127.0.0.1:{}/base/api/paste/abc123", DEFAULT_PORT),
        ),
    ];

    for (base, segments, expected) in cases {
        let url = api_url(base.as_str(), &segments).expect("api_url should build");
        assert_eq!(url.as_str(), expected);
    }
}

#[test]
fn cli_parses_search_meta_subcommand() {
    let cli = Cli::try_parse_from(["lpaste", "search-meta", "needle"])
        .expect("cli should parse search-meta");
    match cli.command {
        Commands::SearchMeta { query } => assert_eq!(query, "needle"),
        _ => panic!("expected search-meta command"),
    }
}

#[test]
fn cli_parses_no_discovery_flag() {
    let cli = Cli::try_parse_from(["lpaste", "--no-discovery", "list"])
        .expect("cli should parse no-discovery flag");
    assert!(cli.no_discovery);
}

#[test]
fn resolve_server_prefers_explicit_over_discovery() {
    with_discovery_env("explicit", None, |env| {
        env.write_discovery("http://127.0.0.1:45555");
        assert_eq!(
            resolve_server(Some("http://127.0.0.1:45556".to_string())),
            "http://127.0.0.1:45556"
        );
    });
}

#[test]
fn resolve_server_with_source_matrix_covers_precedence_and_no_discovery_mode() {
    with_discovery_env("source-matrix", None, |env| {
        let (discovered, _server) = bind_localpaste_discovery(env);

        let explicit = resolve_server_with_source(Some("http://127.0.0.1:45556".to_string()), true);
        assert_eq!(explicit.0, "http://127.0.0.1:45556");
        assert_eq!(explicit.1, ServerResolutionSource::Explicit);

        let discovered_source = resolve_server_with_source(None, true);
        assert_eq!(discovered_source.0, discovered);
        assert_eq!(discovered_source.1, ServerResolutionSource::Discovery);

        let no_discovery = resolve_server_with_source(None, false);
        assert_eq!(no_discovery.0, DEFAULT_CLI_SERVER_URL);
        assert_eq!(no_discovery.1, ServerResolutionSource::Default);
    });
}

#[test]
fn resolve_server_fallback_matrix_for_invalid_unreachable_and_non_loopback_discovery() {
    let cases = [
        ("invalid", "not a url"),
        // Unknown scheme is parseable but has no known default port, so reachability
        // check always treats it as unavailable.
        ("stale", "custom-scheme://discovery-host"),
        ("non-loopback-host", "http://example.com:45555"),
    ];

    for (label, discovery) in cases {
        assert_resolve_server_falls_back_to_default(label, discovery);
    }
}

#[test]
fn discovered_server_file_returns_none_when_reachability_check_fails() {
    with_discovery_env("stub-unreachable", None, |env| {
        env.write_discovery("http://127.0.0.1:45555");
        let discovered = discovered_server_from_file_with_reachability(|_| false);
        assert!(discovered.is_none());
    });
}

#[test]
fn resolve_server_discovery_matrix_handles_absent_blank_and_non_localpaste_endpoints() {
    let cases = [
        ("discovery", None, DiscoveryKind::Localpaste),
        ("blank-explicit", Some("   "), DiscoveryKind::Localpaste),
        (
            "non-localpaste",
            None,
            DiscoveryKind::ReachableNonLocalpaste,
        ),
    ];

    for (label, explicit, discovery_kind) in cases {
        with_discovery_env(label, None, |env| match discovery_kind {
            DiscoveryKind::Localpaste => {
                let (discovered, _server) = bind_localpaste_discovery(env);
                assert_eq!(
                    resolve_server(explicit.map(|value| value.to_string())),
                    discovered
                );
            }
            DiscoveryKind::ReachableNonLocalpaste => {
                let (_discovered, _listener) = bind_reachable_discovery(env);
                assert_eq!(
                    resolve_server(explicit.map(|value| value.to_string())),
                    DEFAULT_CLI_SERVER_URL
                );
            }
        });
    }
}

#[test]
fn discovery_identity_probe_requires_localpaste_headers() {
    let valid = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: application/json\r\n",
        "X-Content-Type-Options: nosniff\r\n",
        "X-Frame-Options: DENY\r\n",
        "X-LocalPaste-Server: 1\r\n",
        "\r\n",
        "[]"
    )
    .as_bytes();
    assert!(discovery_probe_response_looks_like_localpaste(valid));

    let missing_headers = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: application/json\r\n",
        "\r\n",
        "[]"
    )
    .as_bytes();
    assert!(!discovery_probe_response_looks_like_localpaste(
        missing_headers
    ));
}

#[test]
fn discovery_host_loopback_allows_only_local_targets() {
    assert!(localpaste_core::text::is_loopback_host("localhost"));
    assert!(localpaste_core::text::is_loopback_host("127.0.0.1"));
    assert!(localpaste_core::text::is_loopback_host("::1"));
    assert!(!localpaste_core::text::is_loopback_host("example.com"));
    assert!(!localpaste_core::text::is_loopback_host("192.168.1.20"));
}

#[test]
fn lp_server_env_value_beats_discovery_file() {
    with_discovery_env("env", Some("http://127.0.0.1:47777"), |env| {
        env.write_discovery("http://127.0.0.1:48888");
        let cli = Cli::parse_from(["lpaste", "list"]);
        assert_eq!(resolve_server(cli.server), "http://127.0.0.1:47777");
    });
}

#[test]
fn default_resolution_connect_hint_only_applies_to_default_source() {
    assert!(default_resolution_connect_hint(ServerResolutionSource::Default).is_some());
    assert!(default_resolution_connect_hint(ServerResolutionSource::Explicit).is_none());
    assert!(default_resolution_connect_hint(ServerResolutionSource::Discovery).is_none());
}
