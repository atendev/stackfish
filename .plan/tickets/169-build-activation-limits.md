---
id: 169
title: Build activation limits
type: task
mode: AFK
status: open
blocked_by: [160, 161]
claimed: false
---

## Question

Build per-turn and once-only use limits on activated abilities, as decided in [162](../closed/162-activation-limits-per-turn.md).

## Notes

- `ActivationRestriction` gains `limit: Option<ActivationLimit { uses: u8, period: Turn | ObjectLifetime }>`. `can_activate_now` checks it last.
- Stores: `GameState.activations_this_turn: BTreeMap<(ObjectId, AbilityId), u8>`, cleared in `engine/turn.rs` beside `once_per_turn_triggered` (CR 602.5b), and `activated_once: BTreeSet<(ObjectId, AbilityId)>`, never cleared. `AbilityId` gains `PartialOrd, Ord`. Add both to `engine/loop_watch.rs`.
- A use counts when its activation begins: the stack push, a costed mana ability's own apply, or the `FreeMana` apply. `unwind_payment` refunds only that activation's key.
- Lua: `:SetActivationsPerTurn(n)` and `:SetActivateOnlyOnce()`. Refuse n = 0 and a second limit call. Update `docs/lua/index.md`, `docs/LUA_API.md`, `docs/lua/activated-abilities.md` and `docs/lua/stubs/game.lua`.
- TODOs: loyalty limits (CR 606.3), exhaust, Mairsil's granted limits, and a permanent that becomes a copy in place getting a fresh limit (CR 707.2, 602.5b).
- Tests and one fixture proof card are listed in 162's Resolution. Confirm that no existing fixture sets a limit, so `BEHAVIOUR_REV` stays.

## Resolution

<Filled in at close: the decision and the reason for it.>