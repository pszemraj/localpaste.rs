# LocalPaste.rs

A blazing-fast, localhost-only pastebin with a modern editor, built in Rust.

![Rust](https://img.shields.io/badge/rust-%23E57000.svg?style=for-the-badge&logo=rust&logoColor=white)
![Status](https://img.shields.io/badge/status-active-success.svg?style=for-the-badge)

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

### Run with Cargo
```bash
cargo run --release
```

### Build and Run Binary
```bash
cargo build --release
./target/release/localpaste
```

Open http://localhost:3030 in your browser.

## CLI Usage

LocalPaste includes a CLI tool (`lpaste`) for terminal usage:

```bash
# Build with CLI features
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

Environment variables:
- `PORT` - Server port (default: 3030)
- `DB_PATH` - Database path (default: ~/.cache/localpaste/db)
- `MAX_PASTE_SIZE` - Maximum paste size in bytes (default: 10MB)
- `RUST_LOG` - Logging level (default: info level, use `RUST_LOG=debug` for verbose logs)

## Development

See [docs/dev.md](docs/dev.md) for development documentation.

## Architecture

- **Backend**: Axum web framework with Sled embedded database
- **Frontend**: Vanilla JavaScript with custom syntax highlighting
- **Storage**: Embedded Sled database (no external DB required)
- **Deployment**: Single binary with embedded static assets

## License

MIT