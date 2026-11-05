---
id: 075
title: Decide a lint against hash-ordered walks
type: grilling
mode: HITL
status: closed
blocked_by: [073]
claimed: false
---

## Question

Should the repo add a clippy rule against walking hash containers, and with which config?

## Notes

- Origin 028. Probe results (clippy 0.1.98, rustc 1.98.0): `clippy::iter_over_hash_type` catches `for x in &map` but no method form; `disallowed-methods` catches `.iter()`, `.values()`, `.keys()`, `.drain()`, `.retain()` but needs a new `clippy.toml`; neither catches `.into_iter()` method calls, `extend` or `from_iter`. `disallowed-types` flags every mention and so forces a decision at each declaration.
- After 073 about 8 order-irrelevant hash walks remain (`commands/combat/damage.rs`, `commands/combat/entries.rs`, `scenario/src/report.rs`); each would need an `#[allow]` with a reason.
- A new config file needs the user's approval. `tools/check.sh` lints tests too (`--all-targets -D warnings`).
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Add the lint. Questions were answered by a subagent with probes on clippy 0.1.98, audited, and approved by the user.

- Config: `iter_over_hash_type = "warn"` in a new `[workspace.lints.clippy]` (reaches carddb, godot, host, scenario). Engine, cli and testutil get their own `[lints.clippy]` table with the same lint instead of `workspace = true`, so the engine keeps its `unreachable_pub` exemption (`docs/implementation/28-module-layout.md:28`). A new root `clippy.toml` sets `disallowed-methods` on `HashMap`/`HashSet` `iter`, `iter_mut`, `keys`, `values`, `values_mut`, `into_keys`, `into_values`, `drain`, `retain`, and reaches every crate. `-D warnings` in `tools/check.sh` makes both fail CI.
- Why: an in-process experiment showed fresh `HashMap`s with identical inserts iterate differently (557/1000 at 2 keys, 1000/1000 at 8). Same-seed replay catches a hash-order bug only on a walk the proof cards reach, and 028 found four they never reached. The lint is the static backstop.
- 9 sites fire today, all order-free: `damage.rs:138, 162, 241, 346, 354`, `entries.rs:151, 154`, `scenario/src/report.rs:91`, `engine/tests/hand_library_script_registration.rs:196`. Clear them by conversion to `BTreeMap`/`BTreeSet` (the `SplitPlan` fields and damage locals, `card_ids`, the test local) or a rewrite (`entries.rs:151` uses `contains_key`). `#[allow(..., reason)]` is the fallback only; no `#[expect]`.
- `pending_damage_splits` stays a `HashMap`: a type change touches 82 sites (80 fixture lines in 29 test files plus 2 constructors) and nothing walks it after the rewrite.
- Rejected: `disallowed-types` flags every mention (564 lines in 149 files by grep; its clippy count was not measured).
- Accepted gaps: `into_iter`, `extend` and `from_iter` on hash containers (clippy cannot name trait methods in `clippy.toml`), and Lua `pairs` order, which stays under the TODOs at `docs/DESIGN.md:212` and `crates/host/src/session.rs:190`. The lint settles the HashMap half of the `session.rs:190` TODO, so the build trims it to Lua `pairs`.
- No ADR; the choice is easy to reverse. Build is ticket 078.
