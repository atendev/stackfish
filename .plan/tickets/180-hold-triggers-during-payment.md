---
id: 180
title: Hold triggers fired during a payment until the cast finishes
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

An ordinary trigger (for example `BecomesTapped`) fired by a mana ability activated mid-payment goes on the stack before the spell is cast. Hold it until casting finishes, so the spell is on the stack first (CR 601.2i, 603.3).

## Notes

- Found by 176 with a probe. See `closed/176-triggered-mana-abilities.md`.
- Triggered mana abilities do not wait. They resolve at once, which 179 builds. This ticket covers only the triggers that use the stack.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
