---
id: 158
title: Build X-tied cost counts
type: task
mode: AFK
status: open
blocked_by: [153]
claimed: false
---

## Question

Build `CostAmount::X` on the counted shapes, using the one X announced with the spell or ability (CR 107.3a, 601.2b, 602.2b).

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Entry's X flag is set when the mana cost or any additional cost uses X.
- Lua accepts `"X"` wherever an amount is taken, and refuses it on a shape with no amount.
- Cover a spell whose X sits only in an additional cost and an activation like "Remove X counters".
- Lua API change: update `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`. Add the wire field additively, with `PROTOCOL_VERSION` left at 1.

## Resolution

<Filled in at close.>
