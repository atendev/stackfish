---
id: 92
title: Effect-owned exile links for impulse draw from a spell
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Who owns an exile link when a spell exiles a card and lets its controller play it this turn? `ExileLinked` records a link only under a battlefield source and the link ends when the source moves, so a Timed grant from a spell matches nothing.

## Notes

- Origin: `map.md` Not yet specified (found by 087), the TODO in `docs/implementation/99-backlog.md` under Zone permissions (CR 607.2a), and 087's Resolution.
- Decide whether the link belongs to the resolving effect or the stack object, when it ends (end of the permission's duration, not the source leaving), and whether `GameState.exiled_with` changes shape.
- Decide whether `ExileLinked` gets an untargeted or library form at the same time. Today it has none.
- Read `docs/implementation/36-zone-permissions.md` and `37-expiring-permissions.md` first.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
