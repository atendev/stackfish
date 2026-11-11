---
id: 155
title: Build zone-moving costs
type: task
mode: AFK
status: open
blocked_by: [152, 153]
claimed: false
---

## Question

Build `Exile { from, subject }`, `ReturnToHand { subject }`, `PutOnLibrary { subject, top }` and `Mill(amount)` as costs.

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- `Exile` reuses `CostPayment::Exile(id)`. `ExileFromGraveyard` stays for Escape.
- `Exile { from: Graveyard, subject: Source }` makes the ability activatable only from the graveyard through the check `DiscardSelf` uses in `commands/activate.rs` and `commands/entries.rs` (CR 113.6m).
- `ReturnToHand` and `PutOnLibrary` take permanents the payer controls. `Mill` has no candidates. It moves library cards to a public zone, so commit pays it last (CR 601.2h).
- Each move is a moving cost under "One object pays one moving cost".
- Lua API change: update `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`. Add the wire arm additively, with `PROTOCOL_VERSION` left at 1.

## Resolution

<Filled in at close.>
