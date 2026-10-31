---
id: 060
title: Decide which repetitive Rust tests become TOML scenarios and whether to rename phase5_*.toml
type: grilling
mode: HITL
status: closed
blocked_by: [046]
claimed: false
---

## Question

Which table-style or repetitive Rust integration tests in `crates/engine/tests/` should move to TOML scenarios, and should the `crates/engine/tests/scenarios/phase5/` directory and its `phase5_*.toml` files be renamed by feature, and to what layout?

## Notes

- 046 consolidated the engine tests into one binary (`crates/engine/tests/main.rs`) and stripped `phase5_` from the Rust test files, but left the TOML scenarios alone.
- `crates/engine/tests/scenarios/phase5/` holds 39 entries, beside `mechanics/` and a few top-level `.toml` files.
- `docs/scenario_format.md` is the scenario schema. A test can only move if the schema can express its setup and assertions.
- The ticket file was rebuilt from its entry in `map-cleanup.md` after the original went missing.
- After the decision, any migration or rename is a build slice for a new ticket.

## Resolution

Leave the Rust integration tests in Rust. `crates/engine/tests/` has about 1,908 tests in 230 files. About 12% of them repeat, and what repeats is setup helpers (protection-from-X, targeting, `layer_engine_*`), not data tables. The scenario schema in `crates/scenario/src/schema.rs` cannot express most of them. It has no assertion for effective characteristics, damage marked, stack contents, specific events or which error came back (`expect_error` is a boolean), and no protection-from-X primitive. Only about 20 to 30 targeting and layer tests would fit today. `crates/engine/tests/scenarios.rs` also runs every file in one `#[test]` that names only the path on failure, so a move would give vaguer failures.

Rename `crates/engine/tests/scenarios/phase5/` into feature folders under `tests/scenarios/` and drop the `phase5_` prefix, for consistency with 046, which dropped it from the Rust files. Example: `phase5/phase5_x_costs.toml` becomes `costs/x_costs.toml`, with folders such as `combat/`, `commander/`, `layers/`, `scripting/`, `targeting/`, `modal/`, `phases/`, `sba/` and `shortcuts/`. `mechanics/banding/` and the four top-level files stay. `scenarios.rs:6-17` walks the directory recursively, so no code changes. The rename updates the cross-reference comments inside the TOML files and the paths in `docs/scenario_format.md`, several of which are already stale. Comments that name a branch get reworded rather than retargeted.

Follow-ups: 068 does the rename. 069 (shared test helpers in `stackfish_testutil`), 070 (one failure per scenario file) and 071 (move the tests that fit, after 070) are optional.

Answered by a subagent from the code and docs. The ticket file was rebuilt from its entry in `map-cleanup.md` after the original went missing.
