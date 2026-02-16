# LocalPaste.rs

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](assets/ui.jpg)

## What It Is

LocalPaste provides:

- Native desktop GUI (`localpaste-gui`) as the primary UX
- Headless API server (`localpaste`) for automation/integration
- CLI client (`lpaste`) for terminal workflows

Runtime note:

- `localpaste-gui` opens and owns the DB path, and runs an embedded API endpoint for compatibility while GUI is open.
- `localpaste` is the headless alternative and should not be run concurrently on the same `DB_PATH` as the GUI.

> [!WARNING]
> Do not run `localpaste-gui` and standalone `localpaste` against the same `DB_PATH` at the same time.

## Quick Start

GUI is the default workspace target:

```bash
# Desktop GUI
cargo run

# Explicit GUI binary
cargo run -p localpaste_gui --bin localpaste-gui
```

Headless server + CLI flow:

```bash
# Terminal A: run server on default local endpoint (127.0.0.1:38411)
cargo run -p localpaste_server --bin localpaste

# Terminal B: create/list via CLI
echo "hello from quickstart" | cargo run -p localpaste_cli --bin lpaste -- new --name "quickstart"
cargo run -p localpaste_cli --bin lpaste -- list --limit 5
```

If your server is not on the default endpoint, set `LP_SERVER` (or pass `--server`):

```bash
# bash example
export LP_SERVER="http://127.0.0.1:38973"

# powershell example
$env:LP_SERVER="http://127.0.0.1:38973"
```

Magika feature defaults:

- `localpaste_gui` and `localpaste_server` enable `magika` by default.
- `localpaste_cli` remains heuristic-only by default (no ORT payload).
- To run GUI/server without Magika:

```bash
cargo run -p localpaste_gui --bin localpaste-gui --no-default-features
cargo run -p localpaste_server --bin localpaste --no-default-features
```

Runtime/provider note:

- `MAGIKA_FORCE_CPU=true` is the default (see `.env.example`), so Magika uses CPU execution provider even when GPU is available.

Build/run/validation command matrices are maintained in:
[`docs/dev/devlog.md`](docs/dev/devlog.md).

## Configuration and Ops

- System architecture walkthrough: [`docs/architecture.md`](docs/architecture.md)
- Language detection + highlighting behavior: [`docs/language-detection.md`](docs/language-detection.md)
- Storage/backend compatibility contract: [`docs/storage.md`](docs/storage.md)
- Security and environment variables: [`docs/security.md`](docs/security.md)
- Service/background operation: [`docs/deployment.md`](docs/deployment.md)
- Locking semantics (_owner lock + paste edit locks_): [`docs/dev/locking-model.md`](docs/dev/locking-model.md)
- Documentation index: [`docs/README.md`](docs/README.md)

## License

MIT
