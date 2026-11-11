---
id: 154
title: Build energy costs
type: task
mode: AFK
status: open
blocked_by: [152]
claimed: false
---

## Question

Build `AdditionalCost::PayEnergy(n)` paid with `CostPayment::Energy(n)` from the payer's energy counters (CR 107.14).

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Follows the `LifePayment` shape: no candidates, and validation refuses more energy than the player has (CR 118.3).
- Check how a player gets energy today. If no op adds it, add `Game.Ops` support only if a test card needs it.
- Lua API change: update `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`. Add the wire arm additively, with `PROTOCOL_VERSION` left at 1.

## Resolution

<Filled in at close.>
