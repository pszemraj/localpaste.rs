# Security Configuration

## Default Security Settings

LocalPaste is built for one user on one machine. By default the API listens on `127.0.0.1` only, accepts cross-origin browser requests only from loopback origins on its own port, and sends a Content-Security-Policy together with `X-Content-Type-Options: nosniff` and `X-Frame-Options: DENY` on every response.

CORS decides which web pages may read API responses; it does not authenticate callers. Any process on the machine, and any client that sends no `Origin` header, can use the loopback API.

## Runtime Configuration

The headless server (`localpaste`) and the API embedded in the GUI read the same environment variables, except where noted.

### Environment Variables

| Variable | Default | Description |
| --- | --- | --- |
| `DB_PATH` | `~/.cache/localpaste/db` (Windows: `%LOCALAPPDATA%\localpaste\db`) | Database directory; [storage.md](storage.md#backend-and-file-layout) lists the files it holds |
| `PORT` | `38411` | Listener port used when `BIND` is unset |
| `BIND` | unset | Listen address as `IP:port`, such as `127.0.0.1:3055` or `[::1]:3055` (hostnames are rejected); takes precedence over `PORT`. A non-loopback address requires `ALLOW_PUBLIC_ACCESS=1` |
| `ALLOW_PUBLIC_ACCESS` | disabled | Allow a non-loopback `BIND` and CORS from any origin; see [Public Exposure](#public-exposure-not-recommended) |
| `MAX_PASTE_SIZE` | `10485760` | Maximum paste size in bytes, for API and GUI writes |
| `AUTO_SAVE_INTERVAL` | `2000` | GUI autosave delay in milliseconds |
| `AUTO_BACKUP` | disabled | Headless server only: back up an existing database at startup |
| `LOCALPASTE_SEARCH_CASE_SENSITIVE` | disabled | Case-sensitive matching for search requests that omit `case_sensitive` |
| `LOCALPASTE_VERSION_INTERVAL_SECS` | `300` | Minimum seconds between saved history snapshots of a paste (`>= 1`) |
| `LOCALPASTE_VERSION_RETENTION_LIMIT` | `200` | Maximum history snapshots kept per paste (`1..=1000`) |
| `LOCALPASTE_PASTE_VERSION_INTERVAL_SECS` | unset | Legacy fallback for `LOCALPASTE_VERSION_INTERVAL_SECS` |
| `MAGIKA_FORCE_CPU` | `true` | With Magika enabled, run ONNX Runtime on the CPU execution provider; a falsey value (`0`, `false`, `no`, `off`) allows the runtime's provider defaults |
| `RUST_LOG` | server `localpaste=info,tower_http=warn`, GUI `localpaste=warn,localpaste_gui=info` | Tracing filter |
| `LP_SERVER` | unset | `lpaste` only: the API endpoint, skipping [discovery](cli-gui-workflows.md#connect-to-the-running-gui) |

`localpaste` refuses to start when `BIND`, `PORT`, or a numeric, boolean, or version setting is malformed, and when `BIND` is non-loopback without `ALLOW_PUBLIC_ACCESS`. The GUI instead logs a warning and uses the default for a malformed value, and binds a non-loopback `BIND` to `127.0.0.1`. [`.env.example`](../.env.example) lists the defaults; LocalPaste reads only the process environment, not a `.env` file.

`MAX_PASTE_SIZE` applies to decoded paste content. The HTTP request limit is six times `MAX_PASTE_SIZE` plus 16 KiB, because JSON escaping can make a body several times bigger than the text it carries, capped at 256 MiB, so with a `MAX_PASTE_SIZE` above about 42 MiB, heavily escaped content can reach the request cap first.

The single-writer rule for `DB_PATH` is in [storage.md](storage.md#operational-expectations), and lock recovery in [deployment.md](deployment.md#lock-safety).

## Public Exposure (Not Recommended)

The API has no authentication: anyone who can reach it can read, change, and delete every paste. For remote access, an SSH tunnel (`ssh -L 38411:127.0.0.1:38411 user@host`) or a VPN avoids exposing the API at all. If it must be published, put a reverse proxy in front that terminates TLS and requires authentication.

### Reverse proxy on the same host

Leave the server on its defaults. A proxy on the same machine forwards to `http://127.0.0.1:38411`, so neither `BIND` nor `ALLOW_PUBLIC_ACCESS` is needed, and only the proxy is reachable from the network. The server already sets `X-Content-Type-Options` and `X-Frame-Options`, so the proxy does not need to add them. The server keeps no audit log; the proxy's access log is the only record of who did what.

```nginx
server {
    listen 443 ssl;
    http2 on;    # nginx 1.25.1 or later; older versions use "listen 443 ssl http2;"
    server_name paste.example.com;

    ssl_certificate     /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    # The API has no authentication of its own.
    auth_basic           "LocalPaste";
    auth_basic_user_file /etc/nginx/localpaste.htpasswd;

    # nginx rejects request bodies over 1 MB by default. LocalPaste enforces
    # MAX_PASTE_SIZE itself, so allow at least its request limit (about 60 MiB
    # with the default MAX_PASTE_SIZE).
    client_max_body_size 64m;

    location / {
        proxy_pass http://127.0.0.1:38411;
    }
}
```

### When `ALLOW_PUBLIC_ACCESS` is needed

For the headless server, `ALLOW_PUBLIC_ACCESS=1` is required in two cases: the server itself must accept non-loopback connections (a non-loopback `BIND`, for example when the proxy runs on another host or in a separate container network), or web pages from other origins must call the API directly from a browser. Without it, `localpaste` exits at startup when `BIND` is a non-loopback address.

> [!WARNING]
> `ALLOW_PUBLIC_ACCESS=1` lifts the loopback-only restrictions and allows CORS from any origin. It adds no authentication.

With a non-loopback `BIND`, restrict the port at the firewall to the proxy host, for example `ufw allow from <proxy-ip> to any port 38411`.

## Threat Model

LocalPaste is designed for trusted local environments. Its limits:

- No built-in authentication or authorization.
- No HTML sanitization guarantee: paste content is preserved as written, and a client that renders it as HTML must escape it.
- No guarantee against denial of service from a trusted local caller.
- No rate limiting; add it at a reverse proxy if needed.
- No `Host` header check, so the API does not itself defend against DNS rebinding, where a web page re-points its own hostname at `127.0.0.1` and then reads the API as a same-origin page.
- No encryption at rest; use disk encryption.
- No audit logging; diagnostic tracing can be enabled with `RUST_LOG`.

## Reporting Security Issues

Report suspected vulnerabilities privately; do not post details in a public issue. No private contact channel is published yet. Open a [GitHub issue](https://github.com/pszemraj/localpaste.rs/issues/new) that says only that you have a security report and asks how to send it, without exploit details or reproduction steps. Allow time for a fix before disclosing publicly.

## Local Data

Paste content and metadata stay in the local database. LocalPaste has no built-in cloud sync, analytics, or tracking. Local storage does not prevent access through configured API clients or exports.
