# Running LocalPaste as a Background Service

These instructions apply to the headless `localpaste` server. The desktop GUI (`localpaste-gui`) is intended to be launched manually.

---

- [Quick Start](#quick-start)
- [Process Management](#process-management)
- [Linux (systemd)](#linux-systemd)
- [macOS (launchd)](#macos-launchd)
- [Windows](#windows)
- [Common Patterns](#common-patterns)
- [Backups](#backups)
- [Embedded API Discovery](#embedded-api-discovery)

---

## Quick Start

Build/install commands are documented in [dev/devlog.md](dev/devlog.md).
The examples below assume the server binary is available at `$HOME/.cargo/bin/localpaste` (the default `cargo install` location on Unix-like systems).

Security defaults and public-bind policy live in [security.md](security.md).
Storage compatibility and single-writer rules live in [storage.md](storage.md).

```bash
mkdir -p ~/.cache/localpaste
nohup "$HOME/.cargo/bin/localpaste" > ~/.cache/localpaste/server.log 2>&1 &
echo $! > ~/.cache/localpaste/localpaste.pid
```

## Process Management

### Stopping LocalPaste Safely

```bash
# Preferred path: stop by recorded PID
if [ -f ~/.cache/localpaste/localpaste.pid ]; then
  kill -TERM "$(cat ~/.cache/localpaste/localpaste.pid)" 2>/dev/null || true
  rm -f ~/.cache/localpaste/localpaste.pid
fi

# Fallback: stop by process name
pkill -x localpaste || true

# Dev fallback (only if you started it via cargo run)
pkill -f "cargo run -p localpaste_server --bin localpaste" || true

# Verify port release
lsof -i :38411

# Last resort:
# lsof -t -i :38411 | xargs kill -9 2>/dev/null
```

Use graceful shutdown first. Forced termination skips orderly request completion; the OS releases the database owner lock when the process exits.

### Lock Safety

When lock acquisition fails, stop the owning process and retry.
There is no `--force-unlock` path.
The lock file's presence does not mean a process still owns it. Do not delete it to bypass a live writer; the lock is held by the OS on the open file.
For semantics and error contracts, use:
[dev/locking-model.md](dev/locking-model.md) and [storage.md](storage.md).

## Linux (systemd)

### System-wide Service

Create `/etc/systemd/system/localpaste.service`, replacing `username` and the
binary path with your account and installation path:

```ini
[Unit]
Description=LocalPaste
After=network.target

[Service]
Type=simple
User=username
ExecStart=/home/username/.cargo/bin/localpaste
Restart=on-failure
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
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
ExecStart=%h/.cargo/bin/localpaste
Restart=on-failure

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
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
        <string>/Users/username/.cargo/bin/localpaste</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

```bash
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/rs.localpaste.plist
launchctl kickstart -k "gui/$(id -u)/rs.localpaste"
```

## Windows

### Task Scheduler

1. Open Task Scheduler.
2. Create Basic Task.
3. Trigger: `When I log on`.
4. Action: start `C:\Users\username\.cargo\bin\localpaste.exe`.

### PowerShell

```powershell
$Action = New-ScheduledTaskAction -Execute "$env:USERPROFILE\.cargo\bin\localpaste.exe"
$Trigger = New-ScheduledTaskTrigger -AtLogOn
Register-ScheduledTask -TaskName "LocalPaste" -Action $Action -Trigger $Trigger
```

## Common Patterns

### Backups

Set `AUTO_BACKUP=true` to snapshot an existing database at startup, or run `localpaste --backup` while no other writer owns that `DB_PATH`. Backups are consistent redb snapshots stored beside `data.redb` as `data.redb.backup.<timestamp>.redb`, with an additional numeric suffix on name collisions. LocalPaste does not schedule backups or rotate them. Copy snapshots elsewhere for protection against loss of the DB directory.

Startup compatibility repairs create a backup independently of `AUTO_BACKUP`; see the [repair policy](storage.md#compatibility-policy).

To restore, stop the GUI or server that will use the restored database. Keep the
original database and backup, and copy the snapshot into a fresh directory as
`data.redb`. Do not copy the owner lock or `.api-addr` file.

For example, from the source checkout, replace `path/to/backup.redb` with your snapshot:

```bash
mkdir -p target
RESTORE_DB=$(mktemp -d "$PWD/target/lpaste-restore-XXXXXX")
cp path/to/backup.redb "$RESTORE_DB/data.redb"
DB_PATH="$RESTORE_DB" cargo run -p localpaste_gui --bin localpaste-gui
```

Verify the expected pastes and version history in this isolated copy before
using it as your normal `DB_PATH`. Startup may apply the compatibility repairs
linked above; the original snapshot remains available.

### Auto-restart on Crash

With systemd:

```ini
Restart=always
RestartSec=5
```

With cron:

```bash
# Add to crontab
*/5 * * * * pgrep -x localpaste >/dev/null || nohup /path/to/localpaste >/dev/null 2>&1 &
```

### Log Rotation

```text
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
curl -fsS "http://127.0.0.1:38411/api/pastes/meta?limit=1" >/dev/null || echo "Service down"
```

## Embedded API Discovery

For CLI endpoint selection, trust checks, and fallback rules, see [Discovery And Trust](architecture.md#10-discovery-and-trust).
