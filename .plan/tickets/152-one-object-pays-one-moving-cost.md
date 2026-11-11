---
id: 152
title: One object pays one moving cost
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

`PaymentPlan::validate` refuses an object id used by two moving costs (sacrifice, discard, exile), and commit pays the costs that move nothing first. CR 601.2h allows any order among them, so this is the engine's choice.

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Tap and the other non-moving costs may share an object with each other and with one moving cost, so "{T}, Sacrifice this" stays legal.
- Write the failing test first: one card offered for both a discard and an exile cost, or one permanent for two sacrifices.
- Remove the matching open item under Host Session in `docs/implementation/99-backlog.md`.

## Resolution

<Filled in at close.>
