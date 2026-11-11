---
id: 156
title: Build reveal costs
type: task
mode: AFK
status: open
blocked_by: [153]
claimed: false
---

## Question

Build `Reveal { subject, count }` from hand, paid with `CostPayment::Reveal(id)`, writing the existing `Revealed` event.

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The card stays in hand, and the reveal is not a moving cost.
- Check that the host's view rule shows the revealed card to every Seat and that a later snapshot does not keep it revealed.
- Lua API change: update `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`. Add the wire arm additively, with `PROTOCOL_VERSION` left at 1.

## Resolution

<Filled in at close.>
