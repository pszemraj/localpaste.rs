# Security Configuration

Canonical scope:
- Security defaults, threat model, and security-relevant env toggles are defined here.
- Service operation and lock-recovery procedures are canonical in [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md).
- Build/run command matrices are canonical in [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).

---

- [Security Configuration](#security-configuration)
  - [Default Security Settings](#default-security-settings)
  - [Environment Variables](#environment-variables)
  - [Public Exposure (Not Recommended)](#public-exposure-not-recommended)
  - [Security Best Practices](#security-best-practices)
  - [Threat Model](#threat-model)
  - [Reporting Security Issues](#reporting-security-issues)
  - [Compliance Notes](#compliance-notes)

---

## Default Security Settings

LocalPaste.rs is designed for local use and comes with secure defaults:

- **Localhost-only binding**: Server binds to `127.0.0.1` by default
- **CORS restrictions**: In strict mode, only accepts loopback origins that match the active listener port
- **Security headers**: CSP, X-Frame-Options, X-Content-Type-Options
- **Request size limits**: Enforced at transport layer (default: 10MB)
- **Graceful shutdown**: Database flush on exit to prevent data loss
- **Single-writer owner lock**: Process-lifetime `db.owner.lock` prevents concurrent writers on the same `DB_PATH`

## Environment Variables

### Network Configuration

| Variable              | Default           | Description                                                                    |
| --------------------- | ----------------- | ------------------------------------------------------------------------------ |
| `PORT`                | `38411`           | Listener port used when `BIND` is unset                                        |
| `BIND`                | `127.0.0.1:38411` | Server bind address (non-loopback requires `ALLOW_PUBLIC_ACCESS=1`)            |
| `ALLOW_PUBLIC_ACCESS` | disabled          | Enable CORS for all origins and allow non-loopback bind                        |
| `MAX_PASTE_SIZE`      | `10485760`        | Max accepted paste size (bytes) for write paths (API and GUI backend)          |
| `AUTO_BACKUP`         | disabled          | Create DB backup on startup when existing DB is present                         |

`localpaste` startup now fails fast on malformed `BIND`/`PORT`/size/boolean env values so invalid deployment configuration is explicit.

### Security Headers

The following headers are automatically set:

- `Content-Security-Policy`: Restricts resource loading to same-origin
- `X-Content-Type-Options: nosniff`: Prevents MIME-type sniffing
- `X-Frame-Options: DENY`: Prevents clickjacking

To add a referrer policy, configure your reverse proxy or extend the Axum middleware layer.

### Lock Management Policy

Operational lock-recovery procedures are canonical in [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md).
Lock behavior semantics are canonical in [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md).
Security expectation:

- Treat uncertain lock ownership as unsafe.
- Do not classify "probe/tooling unavailable" as "safe to force unlock".

## Public Exposure (Not Recommended)

If you need to expose LocalPaste publicly, follow these steps:

### 1. Enable Public Binding

Build/run mechanics are canonical in [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md) and [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).
This section only defines the security-relevant overrides:

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

        # WebSocket support (if needed)
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
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
   Use the canonical service/logging patterns in [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md).

3. **Backups**: Regular database backups
   Use the backup and retention procedures in [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md).

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

- Prevents unauthorized remote access (localhost binding)
- Prevents XSS attacks (CSP headers, input sanitization)
- Prevents large payload DoS (size limits)
- Prevents clickjacking (X-Frame-Options)

### What's Not Protected

- No built-in authentication/authorization
- No encryption at rest (use disk encryption)
- No rate limiting (add reverse proxy if needed)
- No audit logging (basic access logs only)

## Reporting Security Issues

If you discover a security vulnerability, please:

1. Do not create a public GitHub issue
2. Email details to the maintainer
3. Allow time for a fix before disclosure

## Compliance Notes

LocalPaste stores all data locally and does not:

- Transmit data to external services
- Include analytics or tracking
- Store personal information beyond paste content
- Use cookies or local storage for tracking

This makes it suitable for environments with strict data residency requirements.
