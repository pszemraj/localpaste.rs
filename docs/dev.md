# Development Documentation

## Project Structure

```text
localpaste.rs/
|-- Cargo.toml
|-- crates/
|   |-- localpaste_core/        # Config, storage, models, naming, core errors
|   |   `-- src/
|   |       |-- config.rs
|   |       |-- db/
|   |       |-- error.rs
|   |       |-- models/
|   |       `-- naming/
|   |-- localpaste_gui/         # New egui rewrite (Phase 2+)
|   |   `-- src/
|   |       |-- app.rs
|   |       `-- backend/
|   `-- localpaste_server/      # Axum API server (used by headless + GUI)
|       `-- src/
|           |-- embedded.rs
|           |-- handlers/
|           |-- error.rs
|           `-- locks.rs
|-- src/
|   |-- bin/
|   |   |-- localpaste-gui.rs   # Primary desktop launcher (rewrite)
|   |   `-- lpaste.rs           # CLI client (requires `cli` feature)
|-- legacy/
|   |-- gui/                    # Legacy egui widgets / layout
|   `-- bin/                    # Legacy desktop launcher
|-- docs/                       # Project documentation
|-- assets/                     # Screenshots / design references
`-- target/                     # Build artifacts (git-ignored)
```


## Key Design Decisions

### Single Binary Distribution

- No external dependencies at runtime
- Desktop app embeds the API server in-process
- Database is embedded (Sled)
- Server binds to loopback unless `ALLOW_PUBLIC_ACCESS` is set

### Database Choice

- **Sled**: Embedded, ACID-compliant, fast
- No external database server required
- Data stored in `~/.cache/localpaste/db/`

### Edit Locks

- When a paste is open in the GUI, it is locked against API/CLI deletion.
- Only the GUI instance editing the paste may delete it.

### Frontend Architecture

- **Rewrite (primary):** `crates/localpaste_gui` (egui/eframe app with backend worker)
- **Legacy GUI:** `legacy/gui/mod.rs` (feature-complete reference while rewrite lands)
- Syntax highlighting via `egui_extras` in the rewrite
- Folder management via dialogs with cycle-safe parenting rules (legacy)
- Auto-save with debouncing and manual export support (legacy)
- Incremental in-memory paste index keeps the sidebar responsive without requerying sled (legacy)
- Large pastes (above ~256KB) fall back to a plain renderer so highlight work stays bounded (rewrite + legacy)

### GUI Shortcuts

- `Ctrl/Cmd+N` New paste
- `Ctrl/Cmd+S` Save current paste
- `Ctrl/Cmd+Delete` Delete selected paste
- `Ctrl/Cmd+F` or `Ctrl/Cmd+K` Focus the paste filter in the sidebar

### Known Issues

- The egui text editor caret can “flash” to the start of the next line, especially on macOS, when typing in the middle of a large paste. This is cosmetic but distracting; root cause suspected in the highlight/layout invalidation path. Track and fix in a follow-up branch.

### API Design

RESTful endpoints:

- `POST /api/paste` - Create paste
- `GET /api/paste/:id` - Get paste
- `PUT /api/paste/:id` - Update paste
- `DELETE /api/paste/:id` - Delete paste
- `GET /api/pastes` - List pastes
- `GET /api/search?q=` - Search pastes
- `POST /api/folder` - Create folder
- `GET /api/folders` - List folders
- `PUT /api/folder/:id` - Update folder (rename or re-parent; rejects cycles)
- `DELETE /api/folder/:id` - Delete folder

Folder operations enforce tree integrity: the API returns `400 Bad Request` if a move would introduce a cycle in the hierarchy.

## Development Workflow

### Building the Project

The project contains multiple binaries:

- `localpaste` - The web server (headless)
- `lpaste` - CLI tool for interacting with the server (enable the `cli` feature)
- `localpaste-gui` - Native rewrite (primary desktop app, feature `gui`)
- `localpaste-gui-legacy` - Legacy egui desktop app (feature `gui-legacy`)
- `localpaste_gui` - Rewrite crate (workspace crate)

```bash
# Build the primary GUI and server
cargo build --release

# Build core library only
cargo build -p localpaste_core

# Build only the server
cargo build --release --bin localpaste

# Build only the CLI
cargo build --release --bin lpaste --features cli
```

### Running Locally

```bash
# Run the desktop app (rewrite)
cargo run
# or:
cargo run --bin localpaste-gui

# Run the legacy desktop app
cargo run --bin localpaste-gui-legacy --features="gui-legacy"
# (append --release when you need an optimized build)

# Run the new native rewrite crate directly (Phase 2+)
cargo run -p localpaste_gui

# Run the server/API (JSON endpoints)
cargo run --bin localpaste --release

# Run with auto-reload (server)
cargo install cargo-watch
cargo watch -x "run --bin localpaste"

# Run with debug logging
RUST_LOG=debug cargo run --bin localpaste

# Run tests (include legacy GUI when necessary)
cargo test --features gui-legacy

# Run core tests only
cargo test -p localpaste_core

# Run rewrite tests only
cargo test -p localpaste_gui

# Format code
cargo fmt

# Lint (all targets)
cargo clippy --all-targets --all-features
```

### Building for Production

```bash
# Optimized server build
cargo build --release

# Include the CLI binary
cargo build --release --features cli

# Include the desktop GUI
cargo build --release --bin localpaste-gui

# Include the legacy desktop GUI
cargo build --release --bin localpaste-gui-legacy --features gui-legacy

# Strip symbols for smaller binaries (build required first)
strip target/release/localpaste
strip target/release/lpaste
strip target/release/localpaste-gui
strip target/release/localpaste-gui-legacy

# Check binary sizes
du -h target/release/localpaste target/release/lpaste target/release/localpaste-gui target/release/localpaste-gui-legacy
```

### Using the CLI Tool

```bash
# Build with the CLI feature before running
cargo build --release --features cli --bin lpaste

# The CLI tool should be run from the compiled binary
./target/release/lpaste --help

# Examples
echo "test" | ./target/release/lpaste new
./target/release/lpaste list
./target/release/lpaste get <paste-id>
```

### Adding New Features

1. **Backend Changes**
   - Add model in `crates/localpaste_core/src/models/`
   - Add database operations in `crates/localpaste_core/src/db/`
   - Add naming helpers in `crates/localpaste_core/src/naming/`
   - Add handler in `crates/localpaste_server/src/handlers/`
   - Register route in `crates/localpaste_server/src/lib.rs`

2. **Frontend Changes**
   - Rewrite: update `crates/localpaste_gui/` and run `cargo run --bin localpaste-gui`
   - Legacy: update egui components in `legacy/gui/` and run `cargo run --bin localpaste-gui-legacy --features gui-legacy`
   - Refresh screenshots in `assets/` if the UI changes

3. **Database Migrations**
   - Sled handles schema evolution automatically
   - Add migration logic in `crates/localpaste_core/src/db/mod.rs` if needed

## Code Style

- Use `cargo fmt` before committing
- Follow Rust naming conventions
- Keep functions small and focused
- Add doc comments for public APIs

## Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture

# Run benchmarks
cargo bench
```

## Debugging

### Enable Debug Logging

```bash
RUST_LOG=debug cargo run
```

### Database Inspection

```bash
# View database files
ls -la ~/.cache/localpaste/db/

# Database size
du -sh ~/.cache/localpaste/

# List backups
ls -la ~/.cache/localpaste/db.backup.*

# Check database integrity (requires running server)
curl http://localhost:38411/api/health  # (if implemented)
```

### Database Management

**Backup and Recovery:**

```bash
# Manual backup (server can be running)
./target/release/localpaste --backup

# Enable automatic backups on startup (default: false)
AUTO_BACKUP=true ./target/release/localpaste

# Default behavior - no auto-backup
./target/release/localpaste

# Restore from backup
cp -r ~/.cache/localpaste/db.backup.TIMESTAMP ~/.cache/localpaste/db
```

**Clean Shutdown:**

- Always use Ctrl+C to stop the server (triggers graceful shutdown)
- This ensures database flush and lock cleanup
- Avoid `kill -9` which prevents cleanup

**Development Best Practices:**

- Use `timeout` with caution - it can leave locks
- Create backups before testing destructive operations
- Use a separate DB_PATH for testing: `DB_PATH=/tmp/test-db cargo run`

### Common Issues

**Database Lock Error**

⚠️ **CRITICAL: Never delete the entire database directory to fix lock issues!**

When you encounter: `Error: could not acquire lock on "/home/pszemraj/.cache/localpaste/db/db"`

**Safe Recovery Steps:**

1. Check if LocalPaste is actually running:

   ```bash
   ps aux | grep localpaste
   ```

2. If no process is running, the lock is stale. Use the built-in recovery:

   ```bash
   # Recommended: Use force-unlock (preserves data)
   ./target/release/localpaste --force-unlock

   # Or manually remove ONLY lock files (not the database!)
   rm ~/.cache/localpaste/db/*.lock
   ```

3. **Always backup before manual intervention:**

   ```bash
   # Create backup first
   ./target/release/localpaste --backup
   # Or manually: cp -r ~/.cache/localpaste/db ~/.cache/localpaste/db.backup
   ```

**What NOT to do:**

- ❌ `rm -rf ~/.cache/localpaste/db` - This deletes ALL your data!
- ❌ `pkill -9 localpaste` during normal operation - Use Ctrl+C for graceful shutdown
- ❌ Deleting the database directory - You'll lose all pastes permanently

**Understanding Sled Locks:**

- Sled creates internal lock files to prevent database corruption
- These locks are different from PID files - they're part of the database
- Force-killing processes (`kill -9`) can leave stale locks
- The lock error is Sled protecting your data from corruption

**Automatic Protection:**

- LocalPaste can create automatic backups on startup when `AUTO_BACKUP=true` is set
- Manual backups can be created with `./target/release/localpaste --backup`
- Backups are stored as `~/.cache/localpaste/db.backup.TIMESTAMP`
- To restore from backup: `cp -r ~/.cache/localpaste/db.backup.TIMESTAMP ~/.cache/localpaste/db`

**Port Already in Use**

- Check what's using port: `lsof -i :38411`
- Change port (bash/zsh): `PORT=38411 cargo run`
- Change port (PowerShell):

```powershell
$env:PORT = "38411"
# or
$env:BIND = "127.0.0.1:38411"
```

## Performance Optimization

- Release builds use `opt-level = "z"` for size
- LTO enabled for better optimization
- Single codegen unit for smaller binary
- Gzip compression for HTTP responses
- Embedded assets are compressed

## Security

See [security.md](security.md) for detailed security configuration and best practices.

## Contributing

1. Format code: `cargo fmt`
2. Check lints: `cargo clippy`
3. Run tests: `cargo test`
4. Update documentation if needed
5. Create descriptive commit messages
