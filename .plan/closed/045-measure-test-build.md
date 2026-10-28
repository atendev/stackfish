---
id: 045
title: Measure test build and decide on consolidating test binaries
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Cargo builds each file in `tests/` as its own test executable, and `crates/engine/tests/` contains 224 separate test files. How long do clean and incremental test builds take (`cargo test --no-run`), and should we consolidate the engine test binaries into a single runner (e.g. `tests/main.rs` with modules) or leave them as individual test files?

## Notes

- Map: `map-cleanup.md`, section B.
- Measure:
  1. Incremental baseline: `cargo test -p stackfish-engine --no-run`
  2. Incremental after touching one core engine file (e.g., adding/removing a newline in `crates/engine/src/lib.rs` or `crates/engine/src/engine/mod.rs`): `cargo test -p stackfish-engine --no-run`
  3. Clean test compile time (or package-clean: `cargo clean -p stackfish-engine && cargo test -p stackfish-engine --no-run`).
- If compile and link times are acceptable, resolve with "leave as separate test files" and adjust ticket 046 to only perform renaming/reorganizing rather than binary consolidation. If linking 224 binaries causes substantial developer friction, consolidate into `tests/main.rs`.

## Resolution

Consolidate engine test binaries into a single runner (`tests/main.rs` with modules) in ticket 046.

### Measurements

- Incremental test build with no code change: **0.10s** (0.16s wall clock).
- Incremental test build touching a single engine source file (`crates/engine/src/engine/mod.rs` or `src/lib.rs`): **7.65s** wall clock, but **1m 53s** of CPU time across cores (80s user + 32s sys) linking 224 individual executables.
- Clean engine test compile (`cargo clean -p stackfish-engine && cargo test -p stackfish-engine --no-run`): **16.94s** wall clock, **3m 30s** of CPU time across cores.
- Disk usage: `target/debug/deps` reaches **16 GiB** due to 224 separate debug ELF executables.
- Targeted single test run during active feature development (`--test <name>`): **1.01s**.

### Rationale

While wall-clock time on a fast multicore system is tolerable (~7.7s), linking 224 separate test executables costs 16 GiB of disk space and significant CPU thrash on every incremental rebuild. Consolidating into `tests/main.rs` reduces the link targets from 224 to 1, eliminates ~15 GiB of duplicate debug symbols and binaries, and speeds up full incremental test rebuilds to ~1-2s.

Ticket 046 will proceed with both halves: consolidating test binaries under `tests/main.rs` and renaming/organizing `phase5_*` test files by feature.
