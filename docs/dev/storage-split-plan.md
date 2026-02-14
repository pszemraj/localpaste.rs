# Metadata/Content Storage Split Plan

Status: design-complete follow-up plan. Not implemented in this hardening pass.

Purpose:
- Eliminate repeat classes of OOM regressions caused by canonical full-body scans.
- Make metadata-first reads the default without relying on call-site discipline.

## Problem Statement

Current canonical storage keeps full `Paste` bodies in one tree.
Any path that iterates canonical rows must deserialize bodies, even when only metadata is needed.
This repeatedly creates risk in:
- list/search endpoints
- fallback/reconcile flows
- tooling verification paths

Tactical bounded-scan fixes are in place, but the structural footgun remains.

## Target Data Model

Introduce a split model with explicit trees:

1. `pastes_meta_v1` (primary browse index)
- key: `paste_id`
- value: serialized `PasteMetaCanonical` (id, name, language, tags, folder_id, created_at, updated_at, content_len, content_ref)

2. `pastes_content_v1` (payload store)
- key: `content_ref` (hash or UUID indirection)
- value: paste content bytes/string

3. `pastes_by_updated_v1` (recency index)
- key: reverse-time + `paste_id`
- value: `paste_id`

4. Optional `content_refcounts_v1`
- key: `content_ref`
- value: reference count (needed only if content dedup is enabled)

Notes:
- `content_ref` can start as per-paste UUID (simple), then move to hash-addressed dedup later.
- `PasteMeta` API shape remains unchanged.

## Read/Write Path Mapping

Writes:
- `create`:
  - write content row
  - write meta row (with `content_ref` + `content_len`)
  - write recency row
- `update` content:
  - write new content row
  - update meta row (`content_ref`, `content_len`, `updated_at`, etc.)
  - update recency row
  - best-effort old-content cleanup (or deferred GC)
- `delete`:
  - delete meta row
  - delete recency row
  - best-effort content cleanup/decrement refcount

Reads:
- list/search/meta:
  - read `pastes_meta_v1` + recency index only
  - never deserialize full content
- get by id:
  - read meta row, then resolve `content_ref` in content tree
  - compose full `Paste` response

## Migration Strategy

Phase 1: dual-write, single-read
- Keep existing canonical `pastes` tree as source of truth.
- On each mutation, write both legacy canonical row and split trees.
- Add consistency checker metrics/logging.

Phase 2: backfill
- Offline/online scanner reads legacy canonical tree and backfills split rows.
- Idempotent batches with resumable checkpoints.

Phase 3: read cutover
- list/search/meta/get paths switch to split trees.
- Legacy canonical reads remain fallback behind feature flag.

Phase 4: finalize
- Freeze legacy writes.
- Keep read-only fallback for one release window.
- Remove legacy tree usage after validation window.

Rollback:
- During phases 1-3, rollback is a read-path switch back to legacy tree.
- No destructive migration until split reads are proven stable.

## Consistency and Recovery

- Keep current `meta_state` fault/in-progress markers for derived index health.
- Add split-specific reconcile:
  - detect missing content rows for existing meta rows
  - detect orphan content rows (garbage collectable)
  - rebuild recency index from meta rows
- Startup behavior:
  - split reconcile best-effort for derived structures
  - hard-fail only on canonical integrity violations (if legacy is still authoritative)

## API Compatibility

- External HTTP/API payloads remain unchanged.
- `GET /api/pastes` and `/api/search` remain metadata-returning.
- `GET /api/paste/:id` still returns full body.

## Test Strategy

Unit:
- meta/content create-update-delete invariants
- recency index correctness
- reconcile recovery for missing/orphan rows

Integration:
- list/search memory bound under large content fixtures
- restart persistence and reconcile behavior
- dual-write consistency checks during migration phases

Failure injection:
- content write success + meta write failure
- meta write success + recency write failure
- startup reconcile partial failures

Performance gates:
- list/search memory profile independent of paste body size
- no `Vec<Paste>` accumulation in metadata/read-repair paths

## Decision Log

- Chosen: split metadata/content trees with metadata-first reads.
- Deferred: full content-addressable dedup until split baseline is stable.
- Deferred: removing legacy canonical tree until post-cutover soak and rollback confidence.
