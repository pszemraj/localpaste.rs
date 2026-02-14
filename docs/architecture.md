# LocalPaste Architecture

This document is the canonical system walkthrough for LocalPaste.rs.
For command-level developer workflows, use [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).
For security posture, use [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md).
For service operations, use [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md).

## 1) System At A Glance

LocalPaste is a local-first paste manager with a shared core and multiple frontends:

- Desktop GUI (`localpaste-gui`) is the primary UX.
- Headless HTTP API server (`localpaste`) supports automation and integrations.
- CLI (`lpaste`) calls HTTP endpoints and can auto-discover the GUI embedded API.
- Tools (`generate-test-data`, `check-loc`) support fixtures and repository hygiene.

Workspace crates:

- [`crates/localpaste_core`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core): config, models, storage, transaction helpers, invariants.
- [`crates/localpaste_server`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server): Axum routing, middleware, handlers, embedded server helper.
- [`crates/localpaste_gui`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_gui): native app shell, backend worker, editor flows.
- [`crates/localpaste_cli`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_cli): HTTP client and endpoint discovery logic.
- [`crates/localpaste_tools`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_tools): test data generation and line-count checks.

```mermaid
flowchart LR
    GUI["localpaste-gui"] -->|"commands/events"| GUIB["GUI backend worker"]
    GUI -->|"embedded API"| ES["EmbeddedServer (axum)"]
    CLI["lpaste"] -->|"HTTP"| ES
    CLI -->|"HTTP"| HS["localpaste (headless server)"]
    TOOLS["generate-test-data / check-loc"] --> CORE["localpaste_core"]
    ES --> CORE
    HS --> CORE
    GUIB --> CORE
    CORE --> DB[("sled DB")]
    GUI --> DISC[".api-addr discovery file"]
    CLI --> DISC
```

## 2) Runtime Topologies

### GUI-Primary Topology

`localpaste-gui`:

1. Opens the DB at `DB_PATH`.
2. Acquires process-lifetime owner lock.
3. Starts an embedded API server on loopback.
4. Writes embedded API endpoint to `.api-addr` next to the DB path parent.
5. Runs UI and backend worker in-process.

CLI behavior in this mode:

- `lpaste` prefers explicit `--server` / `LP_SERVER`.
- If unset, it reads `.api-addr`, validates discovered endpoint identity, and only then uses it.
- If validation fails, it falls back to the default local endpoint.

### Headless Topology

`localpaste`:

1. Opens the DB at `DB_PATH`.
2. Acquires owner lock.
3. Binds HTTP listener (`BIND` or loopback default).
4. Serves API requests until shutdown.

Important invariant:

- Do not run standalone `localpaste` and `localpaste-gui` against the same `DB_PATH` concurrently.

```mermaid
sequenceDiagram
    participant GUI as localpaste-gui
    participant API as Embedded API
    participant FS as Filesystem
    participant CLI as lpaste

    GUI->>FS: acquire owner lock + open DB
    GUI->>API: bind loopback listener
    GUI->>FS: write .api-addr
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

## 3) Storage Design

LocalPaste uses sled with canonical and derived structures.

Canonical trees:

- `pastes`: authoritative full paste rows.
- `folders`: authoritative folder rows.
- `folders_deleting`: in-progress delete markers for folder-tree operations.

Derived/index trees:

- `pastes_meta`: metadata projection for list/search.
- `pastes_by_updated`: recency ordering index.
- `pastes_meta_state`: metadata index health markers (version/in-progress/faulted).

Primary implementation:

- [`crates/localpaste_core/src/db/mod.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core/src/db/mod.rs)
- [`crates/localpaste_core/src/db/paste/mod.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core/src/db/paste/mod.rs)
- [`crates/localpaste_core/src/db/folder.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core/src/db/folder.rs)

## 4) Consistency Model

Sled does not provide cross-tree atomic transactions, so LocalPaste uses:

- canonical-write-first semantics for source-of-truth rows,
- explicit rollback/compensation for folder-affecting multi-step operations,
- metadata/index health tracking and reconcile markers,
- canonical fallback on read when metadata/index drift is detected.

Core transaction helper:

- [`crates/localpaste_core/src/db/transactions.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core/src/db/transactions.rs)

Folder shared operations and reconcile:

- [`crates/localpaste_core/src/folder_ops.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core/src/folder_ops.rs)

```mermaid
flowchart TD
    W["Write request (create/update/delete/move)"] --> C["Write canonical tree(s)"]
    C --> D["Update derived indexes (meta/updated)"]
    D --> E{"Derived write succeeds?"}
    E -- yes --> OK["Return success"]
    E -- no --> M["Mark metadata state faulted / needs reconcile"]
    M --> OK

    R["Read request (list/search/meta)"] --> H{"Index healthy?"}
    H -- yes --> I["Read from metadata/index trees"]
    H -- no --> F["Fallback to canonical rows"]
    F --> Q["Optionally trigger reconcile path"]
```

## 5) Read And Write Paths

Write surfaces:

- API handlers (`localpaste_server`),
- GUI backend worker (`localpaste_gui`),
- tooling (`localpaste_tools`).

The project centralizes sensitive folder assignment/delete logic in shared core helpers so API and GUI backend paths enforce equivalent invariants.

Read behavior:

- list/search prefer metadata/index projections,
- drift/missing rows trigger canonical fallback (never trust stale derived state as authoritative),
- reconcile paths restore derived structures opportunistically.

## 6) Locking And Concurrency

Two lock layers are used:

1. DB owner lock (filesystem/process-wide): one writer process per DB path.
2. Paste edit locks (in-memory/paste-scoped): prevent API/CLI/bulk mutations on GUI-open pastes.

Canonical lock reference:

- [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)

Primary implementation:

- [`crates/localpaste_core/src/db/lock.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_core/src/db/lock.rs)
- [`crates/localpaste_server/src/locks.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/locks.rs)

## 7) HTTP Layer And Security Boundaries

Axum router and middleware live in:

- [`crates/localpaste_server/src/lib.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/lib.rs)

Current boundary rules:

- strict mode binds loopback unless public access is explicitly enabled,
- strict CORS is loopback + listener-port scoped (not any loopback origin),
- security headers are always set (`CSP`, `X-Frame-Options`, `X-Content-Type-Options`),
- server identity header (`x-localpaste-server: 1`) is set for trust checks.

## 8) GUI Save Pipeline

The GUI uses a command/event backend worker so UI rendering stays non-blocking.

Key properties:

- autosave and manual save dispatch through backend commands,
- metadata save path is separate from content save path,
- shutdown force-enqueues final dirty snapshots before backend shutdown flush.

Relevant code:

- [`crates/localpaste_gui/src/app/state_ops.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_gui/src/app/state_ops.rs)
- [`crates/localpaste_gui/src/app/shutdown.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_gui/src/app/shutdown.rs)
- [`crates/localpaste_gui/src/backend/worker.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_gui/src/backend/worker.rs)

```mermaid
sequenceDiagram
    participant UI as GUI App
    participant W as Backend Worker
    participant DB as sled DB

    UI->>UI: detect dirty content/metadata on exit
    UI->>W: enqueue final content save (forced)
    UI->>W: enqueue final metadata save (forced)
    UI->>W: send Shutdown{flush=true}
    W->>DB: process queued saves in order
    W->>DB: flush()
    W-->>UI: ShutdownComplete
```

## 9) Discovery And Trust

Embedded server discovery path:

- GUI writes `.api-addr`.
- CLI may consume it only when no explicit endpoint override is set.
- CLI validates:
  - scheme/loopback constraints,
  - LocalPaste response fingerprint (including `x-localpaste-server` header).

Relevant code:

- [`crates/localpaste_server/src/embedded.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/embedded.rs)
- [`crates/localpaste_cli/src/main.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_cli/src/main.rs)

## 10) Validation Strategy

Repository-level quality gates are defined in:

- [AGENTS.md](https://github.com/pszemraj/localpaste.rs/blob/main/AGENTS.md)
- [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md)

Core themes:

- full workspace fmt/lint/check/build loop,
- explicit smoke tests for API/core changes,
- rustdoc checks,
- line-count policy checks,
- targeted regression tests for lock/invariant/shutdown edge cases.

## 11) Active Follow-Ups

- Storage split design follow-up: [docs/dev/storage-split-plan.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/storage-split-plan.md)
- Rewrite readiness/perf gate: [docs/dev/parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md)
- Engineering backlog: [docs/dev/backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md)
