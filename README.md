# LocalPaste.rs

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](assets/ui.jpg)

## What It Is

LocalPaste provides:

- Native desktop GUI (`localpaste-gui`) as the primary UX
- Headless API server (`localpaste`) for automation/integration
- CLI client (`lpaste`) for terminal workflows

## Quick Start

```bash
# Desktop GUI
cargo run -p localpaste_gui --bin localpaste-gui
```

Canonical build/run/validation command matrices are maintained in:
[docs/dev/devlog.md](docs/dev/devlog.md).

## Configuration And Ops

- Security and environment variables: [docs/security.md](docs/security.md)
- Service/background operation: [docs/deployment.md](docs/deployment.md)
- Documentation source-of-truth map: [docs/README.md](docs/README.md)

## License

MIT
