# 058 — Update `docs/DESIGN.md` to reflect the new module layout

## Decision

`docs/DESIGN.md` now describes the post-cleanup module layout. The monolithic files from early Phase 1 (`crates/engine/src/engine/mod.rs`, `crates/engine/src/types.rs`, `crates/engine/src/script/globals.rs`) are no longer described as single files; instead the "Core modules" section points to the current submodules and helper modules. Broken references to the old `docs/implementation.md` single-file decision log are replaced with numbered `docs/implementation/*.md` notes, and a new `docs/implementation/28-module-layout.md` records the cleanup rationale.

## Rationale

The cleanup map (038-041) split the largest engine files by subsystem. `DESIGN.md` is the landing doc for anyone reading the codebase, so it must match the actual directory tree. Updating it here avoids directing new readers to files that no longer exist in their old shape.

## Changes

- `docs/DESIGN.md`:
  - Updated "Event log granularity" to point at `crates/engine/src/types/event.rs`.
  - Rewrote "Core modules" to list current owning modules for Game Instance, Object Store, Zone Containers, Rules Processor, Command Executor, Event Ledger, Replacement/Prevention Engine, Continuous Layer Engine, Trigger Engine, Script Runtime, Decision Facade, Protocol Emitter, and Godot Adapter.
  - Added note pointing to `docs/implementation/28-module-layout.md`.
  - Updated "Data model" and "Scripting model" references from `docs/implementation.md` to `docs/implementation/index.md`/`docs/implementation/01-pipeline.md` and `docs/LUA_API.md`.
  - Updated "Verification" and "Status" references from `docs/implementation.md` to `docs/implementation/00-overview.md` and `docs/implementation/index.md`.
- `docs/implementation/index.md`: added entry 28.
- `docs/implementation/28-module-layout.md`: new file summarizing the file splits for `crates/engine/src/types/`, `crates/engine/src/engine/`, `crates/engine/src/script/`, and `crates/scenario/src/`.
- `docs/scenario_format.md` and three `crates/engine/tests/scenarios/phase5/*.toml` files: replaced stale `docs/implementation.md` references with `docs/implementation/09-layers.md` or `docs/implementation/04-sba.md`.

## Tests

- `git diff --check`: no whitespace errors.
- `cargo test -p stackfish-engine`: 1908 passed, 0 failed.

## Audit

Subagent audit (fork of current model): APPROVE. All referenced filesystem paths exist, references are accurate, and the new layout note matches the actual directory tree.
