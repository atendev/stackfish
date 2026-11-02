---
id: 023
title: Wire version bump and outbox bound
type: task
mode: HITL
status: closed
blocked_by: [020, 021]
claimed: false
---

## Question

Apply 020's `v` decision and its bound or drain rule for the per-Seat outbox.

## Notes

- Origin 013. The rules come from 020.
- `Session::from_replay` never drains an outbox while it plays the recorded Answers, so the cap must not lapse a Seat during that drive (022).

## Resolution

Built on `phase5/wire-version-outbox-bound` and merged.

- `v` stays 1. 007 now holds the pre-release rule: `v` stays 1 until the first tagged `crates/host` release or the first host outside this repository, whichever comes first. 007 also says `v` is checked on every request, which the code already did.
- Setup takes an optional `outbox_cap`, `DEFAULT_OUTBOX_CAP` = 4096. A Seat may hold exactly the cap. The next event drops its whole outbox and lapses it. A lapsed Seat gets empty `events` and `resync: true` on every response to it, still sees the open Decision and may still answer. `snapshot` clears the mark.
- The default comes from full bot games over 40 seeds, taking each outbox at the moment it was drained. The peaks were 58 events with 2 players, 77 with 4 in the commander preset, and 108 with 6. 4096 is far past the 4x 020 asked for.
- `Session::from_replay` lifts the cap while it plays the recording, so a loaded Session can still be polled whole. The cap applies again from the next event.
- `CONTEXT.md` gains **Lapsed Seat**. The wire flag keeps 020's name `resync`, which CONTEXT bans only as a word for a Snapshot.
- Tests are in `crates/host/tests/outbox_cap.rs` and in `session.rs` unit tests. One shows a game replayed under a cap of 1 ends with the same Answers, winner and snapshots.
