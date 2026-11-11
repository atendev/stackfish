---
id: 157
title: Build A-or-B cost choices
type: task
mode: AFK
status: open
blocked_by: [151]
claimed: false
---

## Question

Build `AdditionalCost::OneOf(Vec<AdditionalCost>)` with a branch index announced on `CastSpell` and `ActivateAbility` and listed on the Entry.

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The cost narrows to the chosen branch before the Payment Decision opens (CR 601.2b, 601.2f). It does not share the alternative-cost path (CR 118.9).
- A cost with no `OneOf` sends no index. Decide in the requirements report how the action names a branch when a spell has two `OneOf` costs.
- Teach the bot to pick a branch.
- Lua API change: update `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`. Add the wire arm additively, with `PROTOCOL_VERSION` left at 1.

## Resolution

<Filled in at close.>
