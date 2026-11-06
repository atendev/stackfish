---
id: 94
title: Reveal event for a library top and the stand-alone reveal effect
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How does an event-only client learn that a library top became revealed or stopped being the top? And does the stand-alone "play with the top card of your library revealed" effect (CR 401.5) get built?

## Notes

- Origin: `map.md` Not yet specified (found by 086) and the Zone permissions TODO in `99-backlog.md`. A revealed top shows in snapshots and Decisions today, but no event announces it.
- Decide the event name and payload, who produces it, how it interacts with the Seat visibility rule in `crates/host/src/view.rs` (ticket 004), and whether `reveals_top` on a permission and the stand-alone effect share one representation.
- Read `docs/implementation/36-zone-permissions.md` and `31-snapshot-fold-events.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
