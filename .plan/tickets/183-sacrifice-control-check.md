---
id: 183
title: Add the general control check to resolve_sacrifice_effect
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

`resolve_sacrifice_effect` has no general CR 701.21a check that the sacrificing player controls the object. Delayed triggers, Echo and `SacrificeSelf` each check it themselves. Move the check into the shared path.

## Notes

- Found by 164. Its TODO marks the spot. See `closed/164-build-delayed-end-step-triggers.md`, and 111 for the shared Echo helper.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
