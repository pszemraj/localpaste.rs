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

---

## Quick Start

Install the server with the `cargo install` commands in the [README](../README.md#server-and-cli). The examples below assume the server binary is available at `$HOME/.cargo/bin/localpaste` (the default `cargo install` location on Unix-like systems).

Security defaults and public-bind policy live in [security.md](security.md).

Storage compatibility and single-writer rules live in [storage.md](storage.md).

```bash
mkdir -p ~/.cache/localpaste
nohup "$HOME/.cargo/bin/localpaste" > ~/.cache/localpaste/server.log 2>&1 &
echo $! > ~/.cache/localpaste/localpaste.pid
```

The server listens on `127.0.0.1:38411` by default, which is also where `lpaste` looks when it finds no running GUI, so `lpaste list` reaches it without flags. If `PORT` or `BIND` changes the address, pass `--server` or set `LP_SERVER`; see [CLI workflows](cli-gui-workflows.md#connect-to-the-running-gui).

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

# Verify port release
lsof -i :38411

# Last resort:
# lsof -t -i :38411 | xargs kill -9 2>/dev/null
```

Use graceful shutdown first. Forced termination skips orderly request completion; the OS releases the database owner lock when the process exits. The name-based fallback also matches a GUI started from a release archive, whose executable is likewise named `localpaste`, so prefer the PID file when both may be running.

### Lock Safety

A start that fails with "already held by another LocalPaste writer" means another LocalPaste process, either the GUI or another server, has the same `DB_PATH` open. Stop that process, or give this one a different `DB_PATH`, and start again. The OS releases the lock whenever the owning process exits, even if it was killed, so a `db.owner.lock` file left on disk is normal and needs no cleanup. Do not delete it to get past the error: the lock is held on the open file, so deleting the file frees nothing and can let two writers open the same database. For semantics and error contracts, see [dev/locking-model.md](dev/locking-model.md) and [storage.md](storage.md).

## Linux (systemd)

### System-wide Service

Create `/etc/systemd/system/localpaste.service`, replacing `username` and the binary path with your account and installation path:

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

Run `localpaste --backup` to write a snapshot of the database beside `data.redb`, named `data.redb.backup.<unix-timestamp>.redb`. The command needs the database to itself, so stop any GUI or server using that `DB_PATH` first. Setting `AUTO_BACKUP=true` makes the server take the same snapshot on every start against an existing database. LocalPaste neither schedules nor prunes snapshots; copy them to another disk or machine to survive the loss of the database directory, and remove old ones yourself.

To restore, work on a copy so the original database and the snapshot stay untouched:

1. Stop the GUI or server that will use the restored database.
2. Copy the snapshot into a new directory as `data.redb`. Nothing else needs copying: the owner lock and the GUI's `.api-addr` file are created again on start.
3. Start LocalPaste with `DB_PATH` pointing at that directory and check that the expected pastes and version history are present.

```bash
mkdir -p "$HOME/localpaste-restored"
cp path/to/snapshot.redb "$HOME/localpaste-restored/data.redb"
DB_PATH="$HOME/localpaste-restored" localpaste
```

To inspect the copy in the GUI instead, launch it with the same `DB_PATH`. Opening a snapshot from an older build can run a startup [compatibility repair](storage.md#compatibility-policy), which writes a backup of its own next to the copy. Once the contents look right, keep using the new directory as `DB_PATH`, or move it into the original location after setting the old directory aside.

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
