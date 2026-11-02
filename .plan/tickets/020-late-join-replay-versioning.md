---
id: 020
title: Late join, replay and wire versioning
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Decide the shape of the per-Seat state snapshot, the replay export and the card data version a replay records, whether `v` bumps for the `zone_change.object` change, and how the per-Seat outbox is bounded.

## Notes

- Origin 001, 006, 012, 013 and 015. Use the `grilling` and `domain-modeling` skills.
- The `(Seat, state)` snapshot must equal the fold of that Seat's events (004).
- Replay export: seed, setup and accepted Answers in order.
- Card data version: 006 left open how a replay resolves the same cards.
- `v`: 013 changed `zone_change.object`, which 007 calls breaking, and left `v` at 1. Bump now or treat it as pre-release.
- The per-Seat outbox grows without limit for a Seat that never polls. Pick a bound or a drain rule.
- The build tickets are 021, 022 and 023.

## Resolution

<Filled in at close: the decision and the reason for it.>
