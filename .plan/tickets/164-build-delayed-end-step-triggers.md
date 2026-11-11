---
id: 164
title: Build delayed end-step triggers
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
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
