---
id: 99
title: Riders beyond leaving the stack
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How are the riders 085 left out carried? Those are "if it would die or leave exile this way", battlefield-side exile such as unearth, and a rider on a permanent spell's battlefield entry.

## Notes

- Origin: 085 and 088, and the three TODOs in `legality.rs` and `types/stack.rs`. Escape counters (CR 702.138c) were also deferred.
- 085 put the rider on the `StackObject` because a spell-leaving rider is a per-cast fact. A rider that outlives the stack needs a different carrier, probably a permanent-side or replacement-effect record. Decide whether the two share a type.
- Read `docs/implementation/38-zone-permission-riders.md` and `closed/085-zone-permission-riders.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
