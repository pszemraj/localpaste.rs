# Running LocalPaste as a Background Service

> These instructions apply to the headless localpaste server. The desktop GUI (localpaste-gui) is intended to be launched manually. The legacy GUI binary is `localpaste-gui-legacy`.

## Quick Start

```bash
# Start in background
nohup ./localpaste > ~/.cache/localpaste/server.log 2>&1 &

# Stop gracefully (preserves database integrity)
pkill localpaste
```

## Process Management

### Stopping LocalPaste Safely

```bash
# Graceful shutdown (recommended - allows database cleanup)
pkill -f localpaste
pkill -f "cargo run"

# Check if port is still in use
lsof -i :38411

# Full cleanup (if processes are stuck)
pkill -f localpaste && pkill -f "cargo run" && sleep 2

# ONLY if absolutely necessary (may leave database locks):
# lsof -t -i :38411 | xargs kill -9 2>/dev/null
```

⚠️ **Important**: Avoid using `kill -9` as it prevents graceful shutdown and can leave database locks. See [dev/devlog.md](dev/devlog.md#database-lock-error) for recovery steps if locks occur.

## Linux (systemd)

### System-wide Service

Create `/etc/systemd/system/localpaste.service`:

```ini
[Unit]
Description=LocalPaste
After=network.target

[Service]
Type=simple
User=username
ExecStart=/home/username/.local/bin/localpaste
Restart=on-failure
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable localpaste
sudo systemctl start localpaste
```

### User Service (No root)

Create `~/.config/systemd/user/localpaste.service`:

```ini
[Unit]
Description=LocalPaste

[Service]
Type=simple
ExecStart=%h/.local/bin/localpaste
Restart=on-failure

[Install]
WantedBy=default.target
```

```bash
systemctl --user enable localpaste
systemctl --user start localpaste
```

## macOS (launchd)

Create `~/Library/LaunchAgents/rs.localpaste.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>rs.localpaste</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/username/.local/bin/localpaste</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/rs.localpaste.plist
launchctl start rs.localpaste
```

## Windows

### Task Scheduler

1. Open Task Scheduler
2. Create Basic Task
3. Trigger: "When I log on"
4. Action: Start `C:\Users\username\.local\bin\localpaste.exe`

### PowerShell

```powershell
$Action = New-ScheduledTaskAction -Execute "$env:USERPROFILE\.local\bin\localpaste.exe"
$Trigger = New-ScheduledTaskTrigger -AtLogOn
Register-ScheduledTask -TaskName "LocalPaste" -Action $Action -Trigger $Trigger
```

## Docker

```dockerfile
FROM rust:slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/localpaste /usr/local/bin/
EXPOSE 38411
CMD ["localpaste"]
```

```bash
docker build -t localpaste .
docker run -d -p 127.0.0.1:38411:38411 -v localpaste-data:/data localpaste
```

## Common Patterns

### Auto-restart on Crash

With systemd:

```ini
Restart=always
RestartSec=5
```

With cron:

```bash
# Add to crontab
*/5 * * * * pgrep localpaste || nohup /path/to/localpaste &
```

### Log Rotation

```bash
# /etc/logrotate.d/localpaste
/home/username/.cache/localpaste/*.log {
    daily
    rotate 7
    compress
    missingok
    notifempty
}
```

### Health Check

```bash
# Simple health check
curl -f http://127.0.0.1:38411/api/pastes?limit=1 || echo "Service down"
```
