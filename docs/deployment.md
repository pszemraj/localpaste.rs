# Running LocalPaste as a Background Service

> These instructions apply to the headless `localpaste` server. The desktop GUI (`localpaste-gui`) is intended to be launched manually.
> This is the canonical service-operation runbook. Other docs should link here for stop/restart/lock-recovery guidance instead of duplicating procedures.
> Security posture, bind policy, and public exposure guidance are canonical in [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md).
> Build/run command matrices for development are canonical in [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).

## Quick Start

Build/install commands are canonical in [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).
The examples below assume the server binary is available at `$HOME/.cargo/bin/localpaste` (the default `cargo install` location on Unix-like systems).

```bash
mkdir -p ~/.cache/localpaste
nohup "$HOME/.cargo/bin/localpaste" > ~/.cache/localpaste/server.log 2>&1 &
echo $! > ~/.cache/localpaste/localpaste.pid
```

Important runtime rule:
- Do not run standalone `localpaste` and `localpaste-gui` against the same `DB_PATH` at the same time.

## Storage Migration Note (v0.4.0)

- LocalPaste now uses **redb** and stores data at `DB_PATH/data.redb`.
- Existing **sled** data is not opened automatically by this build.
- On startup, if `data.redb` is missing and legacy sled artifacts are detected, startup fails with an explicit migration-style error message.
- There is no bundled auto-migrator in this branch/release line. Back up legacy data first, then migrate with an external compatible sled->redb tool, or choose a fresh empty `DB_PATH`.

For stop/restart/cleanup procedures, use [Stopping LocalPaste Safely](#stopping-localpaste-safely).

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

# Last resort ONLY (can leave lock state requiring recovery):
# lsof -t -i :38411 | xargs kill -9 2>/dev/null
```

Avoid `kill -9` unless absolutely necessary. It bypasses graceful shutdown.

### Lock Safety

This section is the canonical operational guidance for writer coordination.
Security policy context remains in [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md).
Lock behavior semantics are canonical in [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md).

- LocalPaste uses a process-lifetime owner lock file (`db.owner.lock`) in the DB directory.
- Startup acquires that owner lock before opening redb; a second writer on the same `DB_PATH` is rejected.
- There is no `--force-unlock` mode. Stop the owning process and retry.
- Prefer changing `DB_PATH` for isolated tests over sharing one working directory.

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
ExecStart=/usr/local/bin/localpaste
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
curl -fsS "http://127.0.0.1:38411/api/pastes/meta?limit=1" >/dev/null || echo "Service down"
```

## Embedded API Address Discovery (.api-addr)

This section is operational-only. Canonical discovery/trust behavior is defined in:

- [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md) (discovery + trust model)
- [`crates/localpaste_cli/src/main.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_cli/src/main.rs) (actual endpoint resolution logic)

Operational summary:

- GUI sessions write the active embedded API endpoint to `.api-addr`.
- `lpaste` checks `.api-addr` only when `--server` and `LP_SERVER` are unset.
- Discovered endpoints must pass LocalPaste identity validation; stale/hijacked entries are ignored.
- If discovery validation fails, `lpaste` falls back to the default local endpoint.
- Use `lpaste --no-discovery ...` to disable discovery fallback.
- Use explicit `--server` or `LP_SERVER` when you need deterministic endpoint targeting.
- If `lpaste` cannot connect while resolved via `default`, treat mixed-version default endpoint mismatch as likely and set `--server`/`LP_SERVER` explicitly.
