# LocalPaste.rs

![Status](https://img.shields.io/badge/status-active-success.svg?style=for-the-badge)

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](assets/ui.jpg)

---

## Features (Current State)

**Native rewrite (primary, in progress):**
- **egui/eframe UI** with legacy palette + typography
- **Async backend worker** to keep `App::update` free of blocking I/O
- **Virtualized paste list + selection** (read-only editor pane for now)

**Legacy GUI (feature-complete reference while rewrite lands):**
- Auto-save + export, folders, language detection, shortcuts, theming

**Shared backend:**
- **Zero runtime dependencies** - single binary, embedded Sled database

## Quick Start

LocalPaste.rs provides multiple ways to interact with your pastes:

- `localpaste-gui` - Native rewrite (primary desktop app)
- `localpaste-gui-legacy` - Legacy egui desktop app (feature-complete reference)
- `localpaste_native` - Workspace crate for direct rewrite development
- `localpaste` - Axum HTTP API server (headless, JSON only)
- `lpaste` - Command-line interface for terminal usage

### Run the Native Rewrite (Primary)

```bash
cargo run
# or, explicitly:
cargo run --bin localpaste-gui
# or, for the workspace crate:
cargo run -p localpaste_native
```

### Run the Legacy Desktop App

```bash
cargo run --bin localpaste-gui-legacy --features="gui-legacy"
```

### Run the Web Server / API

```bash
# Run with cargo (development)
cargo run --bin localpaste --release

# Or build and run the binary (production)
cargo build --release
./target/release/localpaste
```

The server exposes a JSON API on <http://localhost:38411>. Use the CLI or your own tooling to interact with it.

## CLI Usage

The CLI tool (`lpaste`) interacts with the running server (or the legacy desktop app, which hosts the same API locally):

```bash
# Build the CLI binary (requires the `cli` feature)
cargo build --release --bin lpaste --features cli

# List all pastes
./target/release/lpaste list

# Create a new paste
echo "Hello, World!" | ./target/release/lpaste new

# Get a specific paste
./target/release/lpaste get <paste-id>

# Search pastes
./target/release/lpaste search "rust"

# Delete a paste
./target/release/lpaste delete <paste-id>
```

## Configuration

Copy `.env.example` to `.env` to customize settings:

```bash
cp .env.example .env
```

Available environment variables:

- `PORT` - Server port (default: 38411)
- `DB_PATH` - Database path (default: ~/.cache/localpaste/db)
- `MAX_PASTE_SIZE` - Maximum paste size in bytes (default: 10MB)
- `AUTO_BACKUP` - Enable automatic backups on startup (default: false)
- `RUST_LOG` - Logging level (default: info)

Override the port for a single session:

PowerShell:
```powershell
$env:PORT = "38411"
# or
$env:BIND = "127.0.0.1:38411"
```

bash/zsh:
```bash
PORT=38411 cargo run --bin localpaste
# or
BIND=127.0.0.1:38411 cargo run --bin localpaste
```

For advanced configuration and security settings, see [docs/security.md](docs/security.md).

## Running as a Background Service

LocalPaste can run automatically in the background. See [docs/deployment.md](docs/deployment.md) for headless/server instructions:

- systemd (Linux)
- launchd (macOS)
- Task Scheduler (Windows)
- Docker setup
- Process managers (PM2, Supervisor)
- Auto-restart scripts

## Development

See [docs/dev.md](docs/dev.md) for development documentation, including desktop build steps.

## Architecture

- **Core**: `localpaste_core` holds the storage model + domain logic
- **Native rewrite**: `localpaste_native` (egui/eframe app, async worker)
- **Legacy desktop**: `localpaste-gui-legacy` (existing egui UI, feature reference)
- **Backend**: Axum web framework with Sled embedded database
- **Storage**: Embedded Sled database (no external DB required)
- **Deployment**: Per-platform binaries; legacy GUI behind `--features gui-legacy`

## License

MIT
