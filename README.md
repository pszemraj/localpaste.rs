# LocalPaste.rs

A fast, localhost-only pastebin with a modern editor, built in Rust.

![LocalPaste Screenshot](assets/ui.jpg)

## What It Is

LocalPaste provides:

- Native desktop GUI (`localpaste-gui`) as the primary UX
- Headless API server (`localpaste`) for automation/integration
- CLI client (`lpaste`) for terminal workflows

> [!WARNING]
> Keep exactly one writer process per `DB_PATH` (`localpaste-gui` or standalone `localpaste`).

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

## Precompiled Binaries

GitHub Releases publish GUI assets under `localpaste-*` filenames.

> [!IMPORTANT]
> Release assets currently cover the GUI only. `lpaste` and the standalone `localpaste` server are source-built with Cargo.

Platform coverage:

- Windows x64: `.msi` + `.zip`
- Linux x64: `.AppImage` + `.tar.gz`
- macOS arm64: `.dmg` + `.app.tar.gz` (may be unsigned/unnotarized when Apple signing secrets are unavailable)

Release workflow behavior, naming contract, and macOS signing policy are defined in
[`docs/release-gui.md`](docs/release-gui.md).

Checksum verification examples:

```bash
sha256sum -c checksums.sha256
```

```powershell
Get-FileHash .\localpaste-v<tag>-windows-x86_64.msi -Algorithm SHA256
```

## Configuration and Ops

- Documentation map: [`docs/README.md`](docs/README.md)
- Practical terminal workflows alongside the GUI: [`docs/cli-gui-workflows.md`](docs/cli-gui-workflows.md)
- Build/run/validation workflow: [`docs/dev/devlog.md`](docs/dev/devlog.md)
- GUI release pipeline and artifact contract: [`docs/release-gui.md`](docs/release-gui.md)

## License

MIT
