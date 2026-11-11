---
id: 161
title: Build activation conditions
type: task
mode: HITL
status: open
blocked_by: [160]
claimed: false
---

## Question

Build "Activate only during ..." and "Activate only if ..." restrictions, as "Activate only as a sorcery on any ability" decided.

## Notes

- Read `.plan/closed/127-activate-only-as-a-sorcery.md` for the decisions.
- `ActivationRestriction` gains `condition: Option<InterveningIfSpec>`. Add one line to the `InterveningIfSpec` doc saying it is also the activation condition.
- `:SetActivationCondition(condition)` on `ActivatedAbilityBuilder`, decoded like the Timing permission condition from 125. `can_activate_now` evaluates it with `&ScriptRuntime`, and a script error counts as false.
- Checked only when activation begins, never at resolution (CR 602.5).
- Buildable shapes: during your turn (`Game.Conditions.DuringYourTurn`), during your upkeep, during combat, during the declare blockers step, before attackers are declared, N or more cards in your graveyard, and N or more creatures you control.
- Add `// TODO: card types beyond creature, subtypes and colors for activation conditions` on `card_summary_to_lua` in `script/encode.rs`.
- Tests: a "during your turn" card, an upkeep-only card, a graveyard-count and a creature-count condition, a script error counted as false, a mana ability with a condition, and a Lua round trip.
- Lua API change: update `docs/lua/index.md`, `docs/LUA_API.md`, `docs/lua/activated-abilities.md` and the stub in `docs/lua/stubs/game.lua`.
- Bump `BEHAVIOUR_REV` only if the replay pin moves.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
