---
id: 138
title: Move Persist and Undying onto the Reanimate mover
type: task
mode: HITL
status: open
blocked_by: [134, 135]
claimed: false
---

## Question

Route `ReturnSelfWithCounter` through `Reanimate`, as "Reanimation options" decided.

## Notes

- Read `closed/100-reanimation-options.md`.
- The lineage lookup finds the graveyard object, and if it fails nothing happens (CR 702.79a, 702.93a). `control = owner` plus the counter option.
- Removes the second mover in `resolve/reanimation.rs` and its `TODO`.
- Bump `BEHAVIOUR_REV` only if the replay pin moves.

## Resolution

<Filled in at close.>
