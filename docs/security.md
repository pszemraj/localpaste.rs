# Security Configuration

## Default Security Settings

LocalPaste.rs is designed for local use and comes with secure defaults. The desktop app (localpaste-gui) embeds the same HTTP API, so these recommendations apply there as well:

- **Localhost-only binding**: Server binds to `127.0.0.1` by default
- **CORS restrictions**: Only accepts requests from localhost origins
- **Security headers**: CSP, X-Frame-Options, X-Content-Type-Options, Referrer-Policy
- **Request size limits**: Enforced at transport layer (default: 10MB)
- **Graceful shutdown**: Database flush on exit to prevent data loss

## Environment Variables

### Network Configuration

| Variable              | Default          | Description                                      |
| --------------------- | ---------------- | ------------------------------------------------ |
| `BIND`                | `127.0.0.1:3030` | Server bind address. ⚠️ Use caution when changing |
| `ALLOW_PUBLIC_ACCESS` | disabled         | Enable CORS for all origins. ⚠️ Security risk     |

### Security Headers

The following headers are automatically set:

- `Content-Security-Policy`: Restricts resource loading to same-origin
- `X-Content-Type-Options: nosniff`: Prevents MIME-type sniffing
- `X-Frame-Options: DENY`: Prevents clickjacking
- `Referrer-Policy: no-referrer`: Prevents referrer leakage

## Public Exposure (Not Recommended)

If you need to expose LocalPaste publicly, follow these steps:

### 1. Enable Public Binding

```bash
# Bind to all interfaces
export BIND=0.0.0.0:3030

# Allow cross-origin requests
export ALLOW_PUBLIC_ACCESS=1

# Run the server
./localpaste
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
    add_header X-XSS-Protection "1; mode=block" always;

    location / {
        proxy_pass http://127.0.0.1:3030;
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

   ```bash
   RUST_LOG=info ./localpaste 2>&1 | tee localpaste.log
   ```

3. **Backups**: Regular database backups

   ```bash
   # Use built-in backup command
   ./localpaste --backup

   # Or manual backup
   cp -r ~/.cache/localpaste/db ~/.cache/localpaste/db.backup
   ```

4. **Access Control**: Use firewall rules

   ```bash
   # Allow only specific IPs (example with ufw)
   ufw allow from 192.168.1.0/24 to any port 3030
   ```

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
