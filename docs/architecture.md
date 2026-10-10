# LocalPaste Architecture

---

- [1) System At A Glance](#1-system-at-a-glance)
- [2) Runtime Topologies](#2-runtime-topologies)
- [3) Storage Design](#3-storage-design)
- [4) Consistency Model](#4-consistency-model)
- [5) Read And Write Paths](#5-read-and-write-paths)
- [6) Locking And Concurrency](#6-locking-and-concurrency)
- [7) HTTP Layer And Security Boundaries](#7-http-layer-and-security-boundaries)
- [8) Language Detection And Highlighting](#8-language-detection-and-highlighting)
- [9) GUI Save Pipeline](#9-gui-save-pipeline)
- [10) Discovery And Trust](#10-discovery-and-trust)
- [11) Validation Strategy](#11-validation-strategy)

## 1) System At A Glance

LocalPaste is a local-first paste manager with a shared core and multiple frontends:

- Desktop GUI (`localpaste-gui`) is the primary UX.
- Headless HTTP API server (`localpaste`) supports automation and integrations.
- CLI (`lpaste`) calls HTTP endpoints and can auto-discover the GUI embedded API.
- Tools (`generate-test-data`, `check-loc`, `check-ast-dupes`) support fixtures and repository hygiene.

Workspace crates:

- [`../crates/localpaste_core`](../crates/localpaste_core): config, models, storage, transaction helpers, invariants.
- [`../crates/localpaste_server`](../crates/localpaste_server): Axum routing, middleware, handlers, embedded server helper.
- [`../crates/localpaste_gui`](../crates/localpaste_gui): native app shell, backend worker, editor flows.
- [`../crates/localpaste_cli`](../crates/localpaste_cli): HTTP client and endpoint discovery logic.
- [`../crates/localpaste_tools`](../crates/localpaste_tools): test data generation and repo hygiene checks (`check-loc`, `check-ast-dupes`).

```mermaid
flowchart LR
    GUI["localpaste-gui"] -->|"commands/events"| GUIB["GUI backend worker"]
    GUI -->|"embedded API"| ES["EmbeddedServer (axum)"]
    CLI["lpaste"] -->|"HTTP"| ES
    CLI -->|"HTTP"| HS["localpaste (headless server)"]
    TOOLS["generate-test-data / check-loc / check-ast-dupes"] --> CORE["localpaste_core"]
    ES --> CORE
    HS --> CORE
    GUIB --> CORE
    CORE --> DB[("redb (data.redb)")]
    GUI --> DISC[".api-addr discovery file"]
    CLI --> DISC
```

## 2) Runtime Topologies

### GUI-Primary Topology

`localpaste-gui`:

1. Acquires the process-lifetime owner lock at `DB_PATH`.
2. Opens the database.
3. Starts an embedded API server on loopback.
4. Writes embedded API endpoint to `DB_PATH/.api-addr`.
5. Runs UI and backend worker in-process.

CLI endpoint selection follows [Discovery And Trust](#10-discovery-and-trust).

### Headless Topology

`localpaste`:

1. Acquires the process-lifetime owner lock at `DB_PATH`.
2. Opens the database.
3. Binds HTTP listener (`BIND` or loopback default).
4. Serves API requests until shutdown.

DB ownership rules for both topologies are in [storage.md#operational-expectations](storage.md#operational-expectations).

## 3) Storage Design

The core persists [paste rows, projections, and version history](storage.md) in redb.

- [`../crates/localpaste_core/src/db/mod.rs`](../crates/localpaste_core/src/db/mod.rs)
- [`../crates/localpaste_core/src/db/paste/mod.rs`](../crates/localpaste_core/src/db/paste/mod.rs)
- [`../crates/localpaste_core/src/db/folder.rs`](../crates/localpaste_core/src/db/folder.rs)

## 4) Consistency Model

The [storage atomicity contract](storage.md#durability-and-atomicity) applies to paste, metadata, recency-index, and folder updates.

Core transaction helper:

- [`../crates/localpaste_core/src/db/transactions.rs`](../crates/localpaste_core/src/db/transactions.rs)

Folder shared operations and invariant repair:

- [`../crates/localpaste_core/src/folder_ops.rs`](../crates/localpaste_core/src/folder_ops.rs)

## 5) Read And Write Paths

Write surfaces:

- API handlers (`localpaste_server`),
- GUI backend worker (`localpaste_gui`),
- tooling (`localpaste_tools`).

Folder API pathways remain for compatibility and emit deprecation headers; the GUI organizes pastes through smart filters and search. Folder assignment/delete invariants stay in shared core helpers so API and GUI backend paths enforce equivalent behavior.

Version and diff surfaces:

- `/api/paste/:id/versions*` supports list/get/reset-hard/duplicate for historical snapshots.
- `/api/diff` and `/api/equal` compare head or historical paste references. Distinct
  references with combined content above 1 MiB return `413 Payload Too Large`;
  identical references resolve existence and return equality without that size gate.
- Content-changing writes may persist an older-head snapshot. Snapshot interval and retention behavior are defined in [storage.md#version-history-storage](storage.md#version-history-storage).

Read behavior:

- List and search responses contain metadata rows; fetch `/api/paste/:id` for full content.
- Lists read metadata and recency projections backed by atomic write consistency.
- Title and Metadata searches use the projection; All fields and Body searches
  also read authoritative paste content. Metadata ranking uses `name`, derived
  handle/terms, tags, and normalized language. Every scope filters and ranks the
  full store before applying the result limit.
- Projection updates commit with authoritative writes; reads need no stale-index fallback.

## 6) Locking And Concurrency

See the [lock layers and mutation guards](dev/locking-model.md).

## 7) HTTP Layer And Security Boundaries

Axum router and middleware live in:

- [`../crates/localpaste_server/src/lib.rs`](../crates/localpaste_server/src/lib.rs)

HTTP requests use [loopback binding, size limits, and browser security headers](security.md).

## 8) Language Detection And Highlighting

The core [detects and normalizes languages](language-detection.md#detection-flow); the GUI [resolves grammars and stages highlighting](language-detection.md#gui-highlight-resolution).

## 9) GUI Save Pipeline

The GUI uses a command/event backend worker so UI rendering stays non-blocking.

Key properties:

- autosave and keyboard-triggered manual saves dispatch through backend commands,
- metadata save path is separate from content save path,
- shutdown attempts to drain saves, enqueue final dirty snapshots, and wait for the
  backend to finish; failures and timeouts are logged, so this is best effort.

Relevant code:

- [`../crates/localpaste_gui/src/app/state_ops.rs`](../crates/localpaste_gui/src/app/state_ops.rs)
- [`../crates/localpaste_gui/src/app/shutdown.rs`](../crates/localpaste_gui/src/app/shutdown.rs)
- [`../crates/localpaste_gui/src/backend/worker.rs`](../crates/localpaste_gui/src/backend/worker.rs)

```mermaid
sequenceDiagram
    participant UI as GUI App
    participant W as Backend Worker
    participant DB as redb

    UI->>UI: detect dirty content/metadata on exit
    UI->>W: enqueue final content save (forced)
    UI->>W: enqueue final metadata save (forced)
    UI->>W: send Shutdown{flush=true} (compatibility flag)
    alt saves and shutdown complete before timeout
        W->>DB: process queued saves and commit each mutation
        W-->>UI: ShutdownComplete
    else dispatch, save, or shutdown fails
        UI->>UI: log failure or pending unsaved state
    end
```

## 10) Discovery And Trust

`lpaste` resolves endpoints as follows. `--no-discovery` skips `.api-addr`
probing and uses explicit/env/default resolution.

```mermaid
sequenceDiagram
    participant API as LocalPaste API
    participant FS as Filesystem
    participant CLI as lpaste

    CLI->>CLI: check --server / LP_SERVER
    alt explicit endpoint provided
        CLI->>API: send request to explicit endpoint
    else no explicit endpoint
        CLI->>FS: read .api-addr
        CLI->>API: probe /api/pastes/meta?limit=1
        CLI->>CLI: validate LocalPaste identity headers
        alt probe valid
            CLI->>API: use discovered endpoint
        else probe invalid/stale
            CLI->>CLI: fall back to default local endpoint
        end
    end
```

Discovered endpoints must use HTTP with a loopback host and return `200` from
`/api/pastes/meta?limit=1` with JSON content type, `X-Content-Type-Options: nosniff`,
`X-Frame-Options: DENY`, and `x-localpaste-server: 1` headers.

The probe uses 250 ms connection/read/write timeouts. These checks reject stale or unrelated endpoints; the headers identify a compatible API but do not authenticate a local process.

Relevant code:

- [`../crates/localpaste_server/src/embedded.rs`](../crates/localpaste_server/src/embedded.rs)
- [`../crates/localpaste_cli/src/discovery.rs`](../crates/localpaste_cli/src/discovery.rs)

## 11) Validation Strategy

- [dev/devlog.md#validation-loop](dev/devlog.md#validation-loop)
- [dev/devlog.md#runtime-smoke-test-server-cli](dev/devlog.md#runtime-smoke-test-server-cli)
- [dev/gui-notes.md#manual-gui-human-step-checklist-comprehensive](dev/gui-notes.md#manual-gui-human-step-checklist-comprehensive)
- [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md)
