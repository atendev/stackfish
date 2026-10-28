---
id: 013
title: Build B4 per-Seat visibility filtering
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build per-Seat visibility filtering, including the `Revealed` event and the 005 move-event rule.

## Notes

- Branch: `phase5/seat-visibility`. Follow the CLAUDE.md slice workflow: requirements report first, approval before every commit, closing report, user merges.
- Decisions: 004 (one visibility rule over `(Seat, event)` and `(Seat, state)`; Player Seats see public zones and their own hand; spectators get an `omniscient` flag set by the host; replay of a finished game is omniscient; secret Answers stay hidden) and 005 (move event carries the old id, the new id, and the identity only where the Seat can see each one; opponents see a hand as a count; library ids never leave without identity; search lists are sorted by id).
- Mark any stubbed parts with TODO.

## Resolution

Built per-Seat visibility on `phase5/seat-visibility` and merged it (`2ebffdc`).

- Each Seat has an outbox. A response carries the requesting Seat's events, and a new `poll` request returns what a Seat missed. `advance` without a seat returns no events. Setup takes a `spectators` list, and only spectators can be omniscient.
- `view.rs` holds one rule: a hand is visible to its owner, a library to nobody, every other zone to all. `zone_change` sends the new id, old id, owner, identity and last known info only where the Seat could see them. The Session reads the old id and owner from the state after each step and hides the event's ids if the lookup fails.
- New `Revealed` event, scry and surveil results to the controller, `rng_drawn` to omniscient Seats only, opaque Decisions for non-choosers, and identities for library and scry candidates. Search lists are sorted by id.
- Left open: the `(Seat, state)` snapshot, engine producers for `Revealed` and `face_down`, outbox growth, and the wire `v` still at 1 despite the breaking `zone_change.object` change. Notes are in `docs/implementation/18-host-session.md`.
