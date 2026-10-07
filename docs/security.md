# Security Configuration

---

- [Default Security Settings](#default-security-settings)
- [Runtime Configuration](#runtime-configuration)
- [Public Exposure (Not Recommended)](#public-exposure-not-recommended)
- [Security Best Practices](#security-best-practices)
- [Threat Model](#threat-model)
- [Reporting Security Issues](#reporting-security-issues)
- [Local Data](#local-data)

---

## Default Security Settings

LocalPaste.rs is designed for local use and comes with secure defaults:

- **Localhost-only binding**: Server binds to `127.0.0.1` by default
- **CORS restrictions**: Browser cross-origin access is allowed only from loopback origins matching the active listener port
- **Security headers**: CSP, X-Frame-Options, X-Content-Type-Options
- **Request size limits**: Decoded paste content and transport bodies have separate limits; see [Environment Variables](#environment-variables).

## Runtime Configuration

### Environment Variables

| Variable | Default | Description |
| --- | --- | --- |
| `DB_PATH` | platform cache dir | Database directory containing `data.redb`, lock files, and GUI discovery metadata |
| `PORT` | `38411` | Listener port used when `BIND` is unset |
| `BIND` | unset | Overrides `PORT`; otherwise bind to `127.0.0.1:$PORT`. Non-loopback values require `ALLOW_PUBLIC_ACCESS=1` |
| `ALLOW_PUBLIC_ACCESS` | disabled | Enable CORS for all origins and allow non-loopback bind |
| `MAX_PASTE_SIZE` | `10485760` | Max accepted paste size in bytes for API and GUI backend write paths |
| `AUTO_SAVE_INTERVAL` | `2000` | GUI autosave delay in milliseconds |
| `AUTO_BACKUP` | disabled | Headless server: create a DB backup at startup when an existing DB is present |
| `LOCALPASTE_SEARCH_CASE_SENSITIVE` | disabled | Default case-sensitive matching for search endpoints when the request omits `case_sensitive` |
| `LOCALPASTE_VERSION_INTERVAL_SECS` | `300` | Minimum seconds between persisted historical snapshots (`>= 1`) |
| `LOCALPASTE_VERSION_RETENTION_LIMIT` | `200` | Maximum historical snapshots retained per paste (`1..=1000`) |
| `LOCALPASTE_PASTE_VERSION_INTERVAL_SECS` | unset | Legacy fallback key for `LOCALPASTE_VERSION_INTERVAL_SECS` |

`localpaste` startup fails fast on malformed `BIND`/`PORT`/numeric/boolean/version env values so invalid deployment configuration is explicit.
Reference defaults/examples: [`.env.example`](../.env.example).

`MAX_PASTE_SIZE` bounds decoded UTF-8 content, with a default of 10 MiB. The HTTP body limit allows worst-case JSON escaping: `min(6 * MAX_PASTE_SIZE + 16 KiB, 256 MiB)`. The default transport limit is therefore 60 MiB plus 16 KiB, rather than 10 MiB.

CORS controls browser access to responses; it does not authenticate callers or block clients that omit an `Origin` header. Processes on the machine can access the loopback API.

### Security Headers

The following headers are automatically set:

- `Content-Security-Policy`: Uses same-origin defaults, permits inline scripts/styles and `data:` images, and forbids framing
- `X-Content-Type-Options: nosniff`: Prevents MIME-type sniffing
- `X-Frame-Options: DENY`: Prevents clickjacking

To add a referrer policy, configure your reverse proxy or extend the Axum middleware layer.

### Lock Management

Operational recovery is documented in [deployment.md](deployment.md).
Lock semantics are documented in [dev/locking-model.md](dev/locking-model.md).
`DB_PATH` single-writer contract is documented in [storage.md](storage.md#operational-expectations).
Treat uncertain lock ownership as unsafe.

## Public Exposure (Not Recommended)

If you need to expose LocalPaste publicly, follow these steps:

> [!WARNING]
> Setting `ALLOW_PUBLIC_ACCESS=1` relaxes loopback-only protections. Use it only behind a firewall/reverse proxy you control.

### 1. Enable Public Binding

For launch and service setup, see [deployment.md](deployment.md) and [dev/devlog.md](dev/devlog.md). Public binding requires these overrides:

```bash
# Bind to all interfaces (requires ALLOW_PUBLIC_ACCESS)
export BIND=0.0.0.0:38411

# Allow cross-origin requests and non-loopback bind
export ALLOW_PUBLIC_ACCESS=1
```

### 2. Security Checklist

Before exposing publicly, ensure:

- [ ] Firewall rules configured to limit access
- [ ] Consider adding authentication (not built-in)
- [ ] Use HTTPS proxy (nginx/caddy) for encryption
- [ ] Monitor access logs
- [ ] Regular security updates
- [ ] Backup strategy in place

### 3. Reverse Proxy Example (nginx)

```nginx
server {
    listen 443 ssl http2;
    server_name paste.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    # Security headers
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "DENY" always;

    location / {
        proxy_pass http://127.0.0.1:38411;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## Security Best Practices

1. **Regular Updates**: Keep dependencies updated

   ```bash
   cargo update
   cargo audit
   ```

2. **Monitoring**: Watch logs for unusual activity
   Use the service/logging patterns in [deployment.md](deployment.md).

3. **Backups**: Use the [backup operations](deployment.md#backups) and keep copies outside the DB directory when needed.

4. **Access Control**: Use firewall rules

   ```bash
   # Allow only specific IPs (example with ufw)
   ufw allow from 192.168.1.0/24 to any port 38411
   ```

5. **Keep broad-list payloads bounded by design**
   `GET /api/pastes` and `GET /api/search` return metadata rows.
   Fetch full content with `GET /api/paste/:id` only for selected records.

## Threat Model

LocalPaste is designed for trusted local environments. The main security considerations:

### What's Protected

- Loopback binding keeps the default listener off external network interfaces.
- Browser headers constrain resource loading and forbid framing.
- Size limits bound individual paste content and HTTP request bodies.

### What's Not Protected

- No built-in authentication/authorization
- No HTML sanitization guarantee; paste content is preserved, and clients rendering it as HTML must escape it
- No guarantee against denial of service from a trusted local caller
- No encryption at rest (use disk encryption)
- No rate limiting (add reverse proxy if needed)
- No audit logging; diagnostic tracing can be configured through `RUST_LOG`

## Reporting Security Issues

If you discover a security vulnerability, please:

1. Do not create a public GitHub issue
2. Email details to the maintainer
3. Allow time for a fix before disclosure

## Local Data

Paste content and metadata stay in the local database. LocalPaste has no built-in cloud sync, analytics, or tracking. Local storage does not prevent access through configured API clients or exports.
