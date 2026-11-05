---
id: 078
title: Build the hash-walk lint and clear its sites
type: task
mode: AFK
status: closed
blocked_by: [075]
claimed: false
---

## Question

Add the lint decided in 075 and clear the 9 sites it fires on today, with the replay pin unchanged.

## Notes

- Origin 075. Slice `phase5/hash-walk-lint`, CLAUDE.md implementation loop. The user approved the three config changes in 075.
- Config: `iter_over_hash_type = "warn"` in a new `[workspace.lints.clippy]` in the root `Cargo.toml`; a `[lints.clippy]` table with the same lint in `crates/engine`, `crates/cli` and `crates/testutil` (not `workspace = true`, which would turn on `unreachable_pub` in engine); a root `clippy.toml` with `disallowed-methods` for `std::collections::HashMap` and `HashSet` `iter`, `iter_mut`, `keys`, `values`, `values_mut`, `into_keys`, `into_values`, `drain`, `retain` (only the methods each type has). Leave out `extend`, `into_iter`, `from_iter`.
- Combat damage: convert the `SplitPlan` hash fields (`commands/combat/damage.rs` ~28-34) and the locals at `damage.rs:126, 137, 153, 240, 345` to `BTreeMap`/`BTreeSet`. `ObjectId` and `PlayerId` derive `Ord`.
- `entries.rs:151`: replace the `keys()` collect with a `contains_key` check on `pending_damage_splits`, which stays a `HashMap`.
- Scenario `card_ids` becomes `BTreeMap<String, ObjectId>`: `actions.rs:9, 20, 28`, `build_new_game.rs:51, 59`, `build.rs:450, 466`, `report.rs:81, 102`, `assertions.rs:17`. Drop `HashMap` imports that go unused.
- Test site `engine/tests/hand_library_script_registration.rs:189`: make the local map a `BTreeMap`.
- `#[allow(lint, reason = "...")]` only where a conversion fails; the reason follows the comment rules. No `#[expect]`.
- Trim the TODO at `crates/host/src/session.rs:190` to the Lua `pairs` half. Keep the TODO.
- Confirm `clippy.toml` reaches engine without an opt-in, and that a `for` over a hash map in engine fails.
- Acceptance: `tools/check.sh --full` green; the pin at `crates/host/tests/replay.rs:369` and `BEHAVIOUR_REV` unchanged. Update `docs/implementation/28-module-layout.md` and add a line on ordered containers to `docs/DESIGN.md`.

## Resolution

Built. `iter_over_hash_type` is `warn` in the workspace table and in the engine, cli and testutil tables. A root `clippy.toml` disallows the HashMap and HashSet methods that walk, and a probe showed it reaches every crate with no opt-in. The 9 sites became `BTreeMap` or `BTreeSet`, except `entries.rs`, which now checks `pending_damage_splits` by key. No `#[allow]` was needed. The replay pin and `BEHAVIOUR_REV` did not move, so no walk order had reached the event stream. `into_iter`, `extend`, `from_iter` and Lua `pairs` are still not caught. Notes in `docs/implementation/28-module-layout.md`.
