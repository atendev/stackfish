---
id: 068
title: Rename the phase5 TOML scenarios into feature folders and fix their references
type: task
mode: HITL
status: open
blocked_by: [060]
claimed: false
---

## Question

Move the 39 files in `crates/engine/tests/scenarios/phase5/` into feature folders under `crates/engine/tests/scenarios/` without the `phase5_` prefix, and update every reference to them.

## Notes

- Layout and example names are in `closed/060-rust-tests-to-toml-scenarios.md`. `mechanics/banding/` and the four top-level files stay.
- `crates/engine/tests/scenarios.rs` walks the directory recursively, so no Rust code changes.
- Update the cross-reference comments inside the TOML files and the paths in `docs/scenario_format.md`. Fix the stale ones there too (paths missing the `phase5/` folder, old `phase5_*.rs` names).
- Reword comments that name a branch (`phase5/ward` and similar) rather than retargeting them.
- Use `git mv` so history follows the files. `tools/check.sh --full` must pass.

## Resolution

<Filled in at close: the decision and the reason for it.>
