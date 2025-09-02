# LocalPaste.rs

![Status](https://img.shields.io/badge/status-active-success.svg?style=for-the-badge)

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](assets/ui.png)

---

## Features

- **Single Binary** - Zero runtime dependencies, just run and go
- **Language Detection** - Auto-detects programming language
- **Auto-Save** - Changes save automatically after 1 second
- **Folder Organization** - Drag & drop pastes into folders
- **Semantic Naming** - Auto-generates memorable names (e.g., "mythic-ruby")
- **Fast Search** - Search through all your pastes instantly
- **Keyboard Shortcuts** - Ctrl+N (new), Ctrl+K (search), Ctrl+D (delete)
- **Dark Theme** - Native dark theme with Rust-themed colors

## Quick Start

LocalPaste.rs consists of two binaries:

- `localpaste` - The web server with UI (main application)
- `lpaste` - Command-line interface for terminal usage

### Run the Server

```bash
# Run with cargo (development)
cargo run --bin localpaste --release

# Or build and run the binary (production)
cargo build --release
./target/release/localpaste
```

Open <http://localhost:3030> in your browser.

## CLI Usage

The CLI tool (`lpaste`) interacts with the running server:

```bash
# Build the CLI binary
cargo build --release --bin lpaste

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

Environment variables:

- `PORT` - Server port (default: 3030)
- `DB_PATH` - Database path (default: ~/.cache/localpaste/db)
- `MAX_PASTE_SIZE` - Maximum paste size in bytes (default: 10MB)
- `RUST_LOG` - Logging level (default: info level, use `RUST_LOG=debug` for verbose logs)
- `BIND` - Override bind address (default: 127.0.0.1:3030) - ⚠️ Use with caution
- `ALLOW_PUBLIC_ACCESS` - Enable CORS for all origins (default: disabled) - ⚠️ Security risk

### Security Notes

By default, LocalPaste.rs is configured for local-only access:
- Binds to `127.0.0.1` (localhost only)
- CORS restricted to localhost origins
- Security headers enabled (CSP, X-Frame-Options, etc.)
- Request body size limits enforced
- Graceful shutdown with database flush

To expose the server publicly (not recommended):
1. Set `BIND=0.0.0.0:3030` to bind to all interfaces
2. Set `ALLOW_PUBLIC_ACCESS=1` to allow cross-origin requests
3. Ensure proper firewall rules and authentication are in place

## Development

See [docs/dev.md](docs/dev.md) for development documentation.

## Architecture

- **Backend**: Axum web framework with Sled embedded database
- **Frontend**: Vanilla JavaScript with custom syntax highlighting
- **Storage**: Embedded Sled database (no external DB required)
- **Deployment**: Single binary with embedded static assets

## License

MIT
