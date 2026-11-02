---
id: 020
title: Late join, replay and wire versioning
type: grilling
mode: HITL
status: closed
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

Six decisions, approved in chat.

1. Snapshot shape. The snapshot is the Seat's view of current state with `through_sequence_no`. It holds turn, phase and priority holder, per-player life, counters, commander damage and zone counts, the Seat's own hand with identities, every public zone, the stack and the open Decision (built by `view::decision_for`). Omniscient spectators also get hands and library contents sorted by id. The fold equality is tested with a test-only reducer over the fields events carry, since filtered events leave `sequence_no` gaps.
2. Delivery. New request kind `snapshot` (seat required, empty body). The response has the poll shape plus a `snapshot` field and clears that Seat's outbox. The Seat resumes from events after `through_sequence_no`. Late spectators are pre-declared Seats. 007 must name `poll` and `snapshot`; a new kind does not bump `v`.
3. Replay export. `replay_v`, the setup request bytes verbatim, accepted Answers as `{seat, decision, reply}`, the card data version, an engine build id, and optionally a digest of the omniscient stream. Advance budgets and rejected Answers are not recorded. Export is a seatless host request, `export_replay`, and its bytes are omniscient. This amends 001: the seed lives inside the setup.
4. Card data version. A per-card fingerprint for each key in the setup, from a new `CardSource::fingerprint(key)` (row fields, sorted keywords, active script source and version). No schema change. The replay loader refuses before creating the Session on a mismatch, names the keys and returns `setup_invalid`. The engine build id is checked the same way.
5. Wire `v`. No bump. Amend 007 with an explicit pre-release rule: `v` stays 1 until the first release or first external host. `v` is checked on every request, as 012 and the code do; amend 007's "checked at setup" wording.
6. Outbox bound. Optional setup field `outbox_cap`, counted in events per Seat. A push past the cap drops the whole outbox and marks the Seat lapsed. A lapsed Seat gets empty `events` and `resync: true` until it sends `snapshot`. No auto-drain, no new error code. The `session.rs` TODO stays until 023 lands.

Build order: 021 builds the snapshot; 022 the replay export and fingerprint; 023 the `v` doc amendment and the outbox cap. 023 now waits on 021 as well as 020.

Open points left for the builds:
- Confirm Lua scripts cannot reach `math.random` or `os.time`; replay needs it.
- Replay determinism depends on the open `HashMap` walk audit (028).
- Fingerprint input: `Debug` text depends on the toolchain pin, so choose a canonical encoding.
- Name what counts as the first release for the `v` rule.
- Pick the `outbox_cap` default: measure peak queued events in a full bot Commander game and take at least 4x.
- Decide whether the omniscient snapshot lists libraries at all.

