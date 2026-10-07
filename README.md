# LocalPaste.rs

Your local scratch layer for technical text that should be easy to save, search, edit, diff, and recover.

![LocalPaste Screenshot](assets/ui.jpg)

LocalPaste is for the text that lives between your clipboard, a repo, and a formal document: code snippets, logs, stack traces, config fragments, prompts, queries, links, notes, and half-formed fixes you do not want to lose.

It is local-first by design. The desktop app is the main workspace, backed by an embedded database on your machine. A localhost API and the `lpaste` CLI can use the same store, so terminal capture, scripts, and the GUI can fit into one workflow without sending sensitive material to a cloud pastebin.

> [!WARNING]
> Follow the [storage operational expectations](docs/storage.md#operational-expectations) when combining GUI, server, and CLI workflows.

## Why It Exists

Clipboard history is too transient. A repo is too heavy for every useful fragment. Cloud pastebins are the wrong default for secrets, logs, client data, and work-in-progress.

LocalPaste gives those scraps a durable home:

- paste first, organize later
- search by content, name, tags, language, and derived metadata
- edit in a code-aware desktop surface
- recover older versions when a scratch edit goes sideways
- automate through a CLI or localhost HTTP API when the terminal is faster

## Quick Start

Download the latest binary for your system from
[GitHub Releases](https://github.com/pszemraj/localpaste.rs/releases), then install and run LocalPaste.
Release downloads install the desktop GUI; build from source for `lpaste` or the standalone server.
See [GUI releases](docs/release-gui.md) for artifact names and platform installation details.

To build from source:

```bash
git clone https://github.com/pszemraj/localpaste.rs.git
cd localpaste.rs
cargo run
```

Use the standalone server and CLI when you want a headless workflow:

```bash
# Terminal A: run the server on the default local endpoint, 127.0.0.1:38411
cargo run -p localpaste_server --bin localpaste

# Terminal B: create and list pastes through the CLI
echo "hello from quickstart" | cargo run -p localpaste_cli --bin lpaste -- new --name "quickstart"
cargo run -p localpaste_cli --bin lpaste -- list --limit 5
```

For endpoint selection, GUI discovery, and terminal examples, see
[CLI workflows](docs/cli-gui-workflows.md).

## Documentation

Start here:

- Terminal workflows with the GUI: [`docs/cli-gui-workflows.md`](docs/cli-gui-workflows.md)
- Language detection and highlighting: [`docs/language-detection.md`](docs/language-detection.md#feature-topology)
- Storage and durability: [`docs/storage.md`](docs/storage.md)
- Security, exposure, and configuration: [`docs/security.md`](docs/security.md#environment-variables)
- Deployment and service operations: [`docs/deployment.md`](docs/deployment.md)
- Full documentation map: [`docs/README.md`](docs/README.md)

For development:

- Build, validation, and smoke-test workflow: [`docs/dev/devlog.md`](docs/dev/devlog.md)
- GUI behavior notes, navigation probe, and manual test checklist: [`docs/dev/gui-notes.md`](docs/dev/gui-notes.md)

## License

MIT
