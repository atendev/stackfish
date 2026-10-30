---
id: 057
title: Tighten visibility, remove dead helpers, and apply workspace lints
type: task
mode: HITL
status: open
blocked_by: [055, 056]
claimed: false
---

## Question

What concrete changes are needed to narrow `pub` visibility, remove dead helpers, centralize workspace dependencies, fix clippy errors, and run `rustfmt` across the workspace?

## Notes

- HITL task; needs user approval before committing each code change.
- Blocked by 055 (visibility/dead-helper audit) and 056 (workspace Cargo/clippy/fmt audit), both now closed.
- Expected work:
  1. Add `[workspace.dependencies]` for `serde`, `serde_json`, `rusqlite`, `bitflags`, `mlua`, `toml`, and `godot` where they repeat across crates.
  2. Fix the 77 clippy errors in `stackfish-engine`.
  3. Run `cargo fmt --all` and commit the formatting pass.
  4. Confirm `cargo test --workspace` still passes.
  5. Optionally install `cargo-machete` or `cargo-udeps` and remove any true unused dependencies.

## Resolution

<Filled in at close: the set of changes made and any deferred items.>
