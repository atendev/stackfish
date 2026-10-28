---
id: 046
title: Consolidate engine test binaries and rename phase5_* files by feature
type: task
mode: HITL
status: closed
blocked_by: [044, 045]
claimed: false
---

## Question

Consolidate the 224 engine integration test files into a single test binary (`crates/engine/tests/main.rs` with modules) and strip the `phase5_` prefix from the 229 `phase5_*` test files across `crates/engine/tests/` (223 files) and `crates/host/tests/` (6 files) so they are named by feature, keeping the same tests passing before and after.

## Notes

- Map: `map-cleanup.md`, section B. Blockers 044 and 045 are closed.
- Branch `phase5/consolidate-engine-tests`, cut from `master`.
- In `crates/engine/Cargo.toml`, set `autotests = false` and declare a single `[[test]]` target (`name = "main"`, `path = "tests/main.rs"`) so Cargo does not also build each module file in `crates/engine/tests/` as its own binary.
- Rename all 223 `crates/engine/tests/phase5_<feature>.rs` files to `crates/engine/tests/<feature>.rs` and wire them (plus `scenarios.rs`) as `mod <feature>;` in `crates/engine/tests/main.rs`.
- Rename the 6 `crates/host/tests/phase5_<feature>.rs` files to `crates/host/tests/<feature>.rs`.
- Leave `crates/engine/tests/scenarios/phase5/phase5_*.toml` as-is for the post-046 TOML scenario review in `map-cleanup.md`.
- Update stale `phase5_<feature>.rs` file references in comments and `#[ignore]` strings.
- Run `python3 tools/comment_diff.py` where applicable and verify the exact test counts before and after in `stackfish-engine` and `stackfish-host`.

## Resolution

Done on `phase5/consolidate-engine-tests` (`4014576`), merged to `master`.

- Set `autotests = false` and declared a single `[[test]]` target (`name = "main"`, `path = "tests/main.rs"`) in `crates/engine/Cargo.toml`.
- Added `crates/engine/tests/main.rs` wiring all 224 engine integration test modules (`223` feature modules plus `scenarios`).
- Renamed all 223 `crates/engine/tests/phase5_<feature>.rs` files and all 6 `crates/host/tests/phase5_<feature>.rs` files to `<feature>.rs`, and updated `phase5_<feature>.rs` references in comments and test strings.
- Left `crates/engine/tests/scenarios/phase5/phase5_*.toml` as-is for ticket 059.
- All 2,024 `stackfish-engine` tests (166 unit + 1,858 integration) and 76 `stackfish-host` tests pass with zero warnings.
- Incremental `cargo test -p stackfish-engine --no-run` after touching `crates/engine/src/lib.rs` dropped from 7.65s wall / 1m 53s CPU (linking 225 executables) to 2.43s wall / 4.64s CPU (linking 2 executables).
