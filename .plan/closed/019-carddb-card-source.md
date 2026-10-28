---
id: 019
title: Carddb-backed CardSource
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

The Godot adapter's `create` serves only the 25 proof cards. How does the host load real cards from `../stackfish-db/stackfish.db` while keeping CI independent of that path?

## Notes

- Origin 006 / 015. Branch `phase5/carddb-card-source`.
- `stackfish_carddb::CardDb::load_by_oracle_id` already exists and returns `CardDefinition`s.
- The host protocol's `SetupBody` deck entries already carry `oracle_id` UUIDs.
- CI must still not depend on `../stackfish-db`; all tests for this slice build an in-memory-schema fixture DB in a temp file.
- Godot: add a new `create_with_db` method that opens `../stackfish-db/stackfish.db`, leaving the existing `create` proof-only so the smoke test keeps working.

## Resolution

`CardDbCardSource` in `crates/host/src/carddb_source.rs` wraps `CardDb::load_by_oracle_id`. `crates/godot/src/lib.rs` exposes `create_with_db` for `../stackfish-db/stackfish.db`. Tests use temp fixture DBs; CI does not touch `../stackfish-db`. Merged in commit 671ba73.
