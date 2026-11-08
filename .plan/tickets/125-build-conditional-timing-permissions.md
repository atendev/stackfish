---
id: 125
title: Build conditional Timing permissions
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Build conditional flash as "Timing permission follow-ups" decided: a condition on a Timing permission and a card's own flash from hand.

## Notes

- Read `.plan/closed/098-timing-permission-follow-ups.md` for the decisions.
- `TimingPermission` gains `condition` and `own_card_only`. Add both to `loop_watch.rs`, `testutil/src/atomic.rs` and every struct literal.
- `can_cast_now` takes `&ScriptRuntime`, and `:SetCondition` decodes through `decode_state_condition`. Put `// CR 601.6a` at `cast.rs:142`.
- Add `Game.Conditions.DuringYourTurn`, and `active_player`, `phase`, `step`, `turn_number` and per-card `tapped` to `state_to_lua`.
- `Game.Ops.GrantTimingPermission` refuses a condition and `own_card_only`.
- Fix the citations in `timed_grant_duration` and the `types/player.rs:342` TODO: `Permanent` and `UntilSourceLeaves` are CR 611.2a, and `WhileConditionHolds` and `WhileSourceOnBattlefield` are CR 611.2b.
- Remove the TODOs at `legality.rs:42` and `:43`, and say so in the commit message. Reword `legality.rs:31` to say no printed card needs it.
- Lua API change: update `docs/LUA_API.md`, `docs/lua/player-modifiers.md`, `docs/lua/game-constants.md` and the stub in `docs/lua/stubs/game.lua`, including the snapshot fields.
- Tests: `can_cast_now` units, one `timing_permissions.rs` integration test per condition kind, a Lua round trip and an Illusion Spinners scenario. No proof-deck card unless the replay pin moves.

## Resolution

<Filled in at close.>
