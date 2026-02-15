# LocalPaste.rs

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](https://github.com/pszemraj/localpaste.rs/blob/main/assets/ui.jpg)

## What It Is

LocalPaste provides:

- Native desktop GUI (`localpaste-gui`) as the primary UX
- Headless API server (`localpaste`) for automation/integration
- CLI client (`lpaste`) for terminal workflows

Runtime note:
- `localpaste-gui` opens and owns the DB path, and runs an embedded API endpoint for compatibility while GUI is open.
- `localpaste` is the headless alternative and should not be run concurrently on the same `DB_PATH` as the GUI.

## Quick Start

```bash
# Desktop GUI
cargo run -p localpaste_gui --bin localpaste-gui
```

Canonical build/run/validation command matrices are maintained in:
[docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).

## Configuration And Ops

- System architecture walkthrough: [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
- Storage/backend compatibility contract: [docs/storage.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/storage.md)
- Security and environment variables: [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md)
- Service/background operation: [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md)
- Locking semantics (owner lock + paste edit locks): [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)
- Documentation source-of-truth map: [docs/README.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/README.md)

## License

MIT
