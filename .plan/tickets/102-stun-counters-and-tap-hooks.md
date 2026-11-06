---
id: 102
title: Stun counters and tap or untap replacement hooks
type: grilling
mode: HITL
status: open
blocked_by: [89]
claimed: false
---

## Question

How do stun counters (CR 122.1d) and "would become tapped or untapped" replacements work, so an untap no longer always succeeds?

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (No becomes tapped condition). `Game.Ops.Tap` and `Untap` already write `Tapped` and `Untapped` events on a real flip (CR 603.2e).
- Blocked by the event visibility decision, which settles how a condition sees a tap event. That decision owns the "becomes tapped" condition (`Game.Conditions.BecomesTapped`), so it is not part of this ticket.
- Decide whether stun is a counter kind with a built-in replacement or a general replacement hook, and where the untap step checks it.
- Read `docs/implementation/08-replacements.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
