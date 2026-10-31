---
id: 065
title: Build narrow pub visibility and delete dead helpers
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Carry out the decisions of 064 on branch `phase5/narrow-visibility`.

## Notes

- Follows the `CLAUDE.md` vertical slice workflow: requirements report first, branch pushed to origin, permission before each commit, closing report, the user merges.
- Commits, in order:
  1. Delete the dead items from 064 and fix their docs (`card.rs` "Bitmask of `Keyword`", `00-overview.md` `events_added`, `19-proof-game.md` transcript).
  2. Engine: narrow the 064 set, add the `AwaitingAttachChoice` expect, make `engine_context` and `script_api` private, then narrow the compile-proven engine items.
  3. Delete or `expect` whatever `dead_code` newly flags. If more than about 20 engine items turn up, split them into a follow-up ticket.
  4. Carddb, scenario (`report::is_hard_failure` only) and testutil.
  5. Host and godot, via `unreachable_pub`.
  6. Docs (`DESIGN.md`, `28-module-layout.md`, `scenario_format.md` schema path) and the `unreachable_pub` workspace lint.
- Verify: `cargo build --workspace --all-targets` with zero warnings, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo test --workspace` (baseline 2315, dropping only for tests removed with `is_permanent` and `top`), `cargo doc --workspace --no-deps`, and `git grep -c TODO` unchanged. The user runs the Godot smoke test.

## Resolution

Built on `phase5/narrow-visibility` in five commits (the ticket's commits 2 and 3 are one, because the dead-code handling was needed to make the narrowing build). Verified: build and `clippy -D warnings` have zero warnings, `cargo fmt --check` passes, `cargo test --workspace` passes 2313 tests (2315 minus the two removed with `is_permanent` and `top`), `cargo doc` keeps its 12 existing warnings, and `git grep -c TODO` over `crates` and `docs` is unchanged at 392. The Godot smoke test is left to the user.

- Deleted: the 064 list, plus `Engine::settings` and `Zone::is_empty`, which narrowing left with no callers.
- Narrowed: the 064 set, `engine_context` and `script_api` (private modules), and the engine items and methods that no other crate or integration test names, found by narrowing a scratch copy and restoring what the compiler needed. Carddb, scenario (`is_hard_failure`) and host/godot items came from `unreachable_pub`. Testutil had nothing to narrow.
- Kept with `expect(dead_code)`: `AwaitingAttachChoice`, `invoke_cost` (TODO), `VisibilityPolicy::is_visible_to` (a doc says nothing enforces it).
- Deviation: `GameObject::new`, `PlayerState::new` and `Emblem::new` have only test callers. Deleting them would drop six tests of production methods, so they are `#[cfg(test)]`.
- `ManaUnit::from_snow_source` stays `pub`: narrowing it trips clippy's `wrong_self_convention`.
- `unreachable_pub = "warn"` is a workspace lint for host, godot, carddb and scenario.
