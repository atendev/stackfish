---
id: 021
title: Seat state snapshot
type: task
mode: HITL
status: closed
blocked_by: [020]
claimed: false
---

## Question

Build the `(Seat, state)` snapshot in the shape 020 decides, so a host can late-join a Seat.

## Notes

- Origin 013 and 015. The shape comes from 020.

## Resolution

Built as 020 decided. A `snapshot` request (seat required, empty body) returns the `poll` response with a `snapshot` field and an empty `events` list, and drops the Seat's outbox. `view::snapshot_for` applies the same visibility rule as events: turn, phase, step, priority, per-player life, counters, commander damage and hand and library counts, every zone the Seat may see (hands and libraries sorted by id, libraries for omniscient Seats only), and the stack with hidden sources and targets left out. The open Decision rides in `decision`.

Fold tests rebuild zones, counts, tapped, turn, phase and step from a first snapshot plus the Seat's events. They found that life gain, poison from infect, priority changes and a leaving player's removed objects write no events, so the fold skips life and priority. These gaps are TODOs in the engine and ticket 072. A revealed identity in a hidden zone is not kept in a snapshot (TODO, also in 072). 020's open point on libraries: omniscient Seats get them, sorted by id.
