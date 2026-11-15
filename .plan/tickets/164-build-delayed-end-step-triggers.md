---
id: 164
title: Build delayed end-step triggers
type: task
mode: AFK
status: open
blocked_by: []
claimed: true
---

## Question

Build `GameState.delayed_triggers`, a list of one-shot delayed triggered abilities made by a resolving effect, with `DelayedWhen::NextEndStep` as the only timing, and the Lua `Game.Ops.AtNextEndStep(op, refers)` Chain entry that makes them (CR 603.7).

## Notes

- Origin 132. The full decision is in `closed/132-delayed-triggered-abilities.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Row: `{ id, source, source_lki, controller, when, effect, refers: Vec<(ObjectId, ZoneType)>, created_after: EventId, triggered: bool }`. Rows match only events after `created_after` (CR 603.7a, 513.2), are marked `triggered` at collection and removed at push (CR 603.7b), and are never touched by `retire_object`.
- Resolution: `targets = refers` filtered by "same id, same zone" at resolution (CR 603.7c, 400.7), `target_requirement: None`. A `Sacrifice` row drops objects its controller does not control (CR 701.21a). Allowed ops: `Exile`, `Sacrifice`, `Destroy`, `ReturnToHand`, `ReturnToBattlefield` from exile.
- `refers` is `"targets"`, `"self"` or `"entered"`. `"entered"` reads the ledger since the resolution began, so `AtNextEndStep` is a non-synchronous Chain entry and `ModalContinuation` carries `since_ledger`.
- Players leaving: `sba::remove_players_objects` drops the leaver's rows, and the push skips a row whose controller has left (CR 800.4a, 800.4d).
- The list joins the `loop_watch.rs` fingerprint without `id` and `created_after`. No wire change. `BEHAVIOUR_REV` moves only if the replay pin moves.
- TODOs: other timings (your next upkeep, your next end step, end of combat, dies or leaves this turn), stated durations and the CR 603.7b simultaneous-event choice, CR 603.7f, 603.7g, 603.7h, a chain acting on only one of two entered groups, showing pending rows to players, the general `Sacrifice` control check in `resolve_sacrifice_effect`, and encore, dash, blitz and mobilize.
- Lua API change: update `docs/lua/game-constants.md`, `triggered-abilities.md`, `not-available-yet.md` and `docs/lua/stubs/game.lua`.
- Tests are listed in the decision. Scenario fixtures, no proof deck change.

## Resolution

<Filled in at close.>

## Handoff

Branch `phase5/delayed-end-step-triggers-2` (the old worktree still holds `phase5/delayed-end-step-triggers`), work at `598b41e`. Requirements report `.plan/assets/164-build-delayed-end-step-triggers-requirements.md` approved by the haiku auditor in round 2. Implementation audit has not started (round 0).

Done:
- Implementation from `fcb5e00` (see its message).
- `crates/engine/tests/delayed_triggers.rs`, 18 passing tests covering requirements tests 1-16, except the "as a mode" load error.
- `resolve/delayed.rs`: a spell whose earlier chain entry paused is already in its graveyard when the row is made (its stack-to-graveyard `ZoneChange` lands before the reanimated bear's), so `source_lki` falls back to that move's last known info (`left_since`). Mention this in the decision report; consider surfacing a ticket for the spell moving before its later chain entries finish (CR 608.2n).

Left, in order:
1. Test 16 "as a mode": modes decode through `decode_operation_value`, which rejects `DelayedTag`. Add one test.
2. Test 17 in `engine/loop_watch_tests.rs`: rows with different `id` and `created_after` fingerprint equal; no row, or `triggered: true`, differs.
3. TOML fixtures under `tests/scenarios/delayed/` (Whip-style, flicker), if `docs/scenario_format.md` has a step to reach the end step. Otherwise state why in the report.
4. Docs: `docs/lua/game-constants.md`, `triggered-abilities.md`, `not-available-yet.md`, `stubs/game.lua`, `docs/LUA_API.md`, `docs/DESIGN.md` data model, new `docs/implementation/42-delayed-triggers.md` plus index line, one line in `99-backlog.md`.
5. `tools/check.sh --full`, then the sonnet implementation audit (CLAUDE.md step 5).
6. Decision report `.plan/assets/164-build-delayed-end-step-triggers-report.md`, unslop, commit, push, return PUSHED.

Read first: the requirements report, `tests/delayed_triggers.rs`, `resolve/delayed.rs`.
