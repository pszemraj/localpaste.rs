# LocalPaste.rs

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](assets/ui.jpg)

## What it is

LocalPaste is a local-first paste manager with:

- Native GUI rewrite (primary desktop app)
- Legacy GUI reference app (feature-complete)
- Headless JSON API server + CLI

## Binaries

- `localpaste-gui` - native rewrite desktop app
- `localpaste-gui-legacy` - legacy GUI reference
- `localpaste` - headless API server
- `lpaste` - CLI client

## Quick start

### GUI

```bash
cargo run -p localpaste_gui --bin localpaste-gui
```

Install to your PATH:

```bash
cargo install --path crates/localpaste_gui --bin localpaste-gui
```

### Server + CLI

```bash
cargo run -p localpaste_server --bin localpaste --release
```

The server listens on `http://127.0.0.1:38411` by default.

```bash
# Build the CLI
cargo build -p localpaste_cli --bin lpaste --release

# Create a paste
"Hello, World!" | ./target/release/lpaste new

# List pastes
./target/release/lpaste list
```

## Configuration

Copy `.env.example` to `.env` for overrides:

```bash
cp .env.example .env
```

For environment variables, security guidance, and public exposure notes, see [docs/security.md](docs/security.md).

For background services and OS-specific setup, see [docs/deployment.md](docs/deployment.md).

## Documentation

- [docs/README.md](docs/README.md) - docs table of contents
- [docs/security.md](docs/security.md) - security defaults + public exposure notes
- [docs/deployment.md](docs/deployment.md) - background service setup
- [docs/dev](docs/dev) - dev notes and checklists

## License

MIT
