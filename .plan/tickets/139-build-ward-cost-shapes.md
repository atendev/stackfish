---
id: 139
title: Build ward cost shapes
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Make `Game.RegisterWard` and `Game.Ops.CounterUnlessPaid` accept a mana string or a list of `Game.Costs.*` values, as "Ward with non-mana costs, player hexproof and hexproof from" decided.

## Notes

- Read `closed/101-ward-costs-and-player-hexproof.md`.
- One decoder into `CostSpec`. Allow mana, `LifePayment`, `Discard { filter }` and `Sacrifice { filter }`, and reject every other variant at load with an error naming it.
- Reject computed amounts at load. `// TODO: ward amounts computed at resolution (CR 702.21b)`.
- The `Unless paid` route accepts a plan with additional payments. Reword the placeholder-`source` comment in `submit_unless_paid_payment`.
- Test a sacrifice ward, a discard ward, a life ward and a ward of mana and life. The sacrifice test pins filters reading relative to the payer.
- Update the game stub in `docs/lua/stubs/game.lua`, and `docs/lua/not-available-yet.md`.

## Resolution

<Filled in at close.>
