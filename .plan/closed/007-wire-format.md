---
id: 007
title: Message envelope and wire format
type: grilling
mode: HITL
status: closed
blocked_by: [008]
claimed: false
---

## Question

Which encoding do messages use (JSON, MessagePack, or another), and what is the envelope: protocol version, handshake, message kinds in each direction, how errors and rejected Answers are reported, and how messages are framed when they cross a stream rather than a function call?

## Notes

- Must stay agnostic: no Godot-only formats such as `var_to_bytes`.
- Read 008's findings first for what `gdext` can pass cheaply.
- From 001: Answer and Advance are the two request kinds. Advance carries an optional step budget. Responses carry a status (decision open, still running, game over, error). 007 decides whether the open Decision rides in the same response as the last events. See `closed/001-session-advance.md`.

## Resolution

- Transport: direct calls only. Each call passes one request's bytes in and returns one response's bytes. No framing is needed in the Session.
- Encoding: JSON, written with `serde_json`. The serde derives live in `crates/host`. The engine has no serde dependency.
- Wire types: `crates/host` holds mirror types and converts from the engine types. Wire names may differ from Rust names.
- Field names are snake_case. Every message has `v`, `kind`, and `body`. A request from a Seat also has `seat`. `v` starts at 1.
- Requests:
  - `setup` (no `seat`, since the host creating the Session is not a Seat): the 006 setup. Its fields are the player list in turn order, a decklist per player (card keys as `oracle_id` UUID strings), optional commanders, a format preset, optional setting overrides, and a seed. The seed is a decimal string.
  - `advance`: an optional `budget` integer. A missing budget uses the Session's default cap.
  - `answer`: the id of the open Decision (`decision`), plus the Answer as the tagged reply enum from 002. Candidates use `Object(ObjectId) | Player(PlayerId)` from 005.
  - `poll` (added in 013, seat required, empty body): the Seat's queued events and its view of the open Decision.
  - `snapshot` (added by 020 and built in 021, seat required, empty body): the `poll` response with a `snapshot` field. It drops the Seat's queued events. A new kind does not bump `v`.
  - `export_replay` (added by 020 and built in 022, no `seat`, empty body): status `exported` with a `replay` field holding `replay_v`, the setup bytes, the accepted Answers, card fingerprints and the engine build. It holds every hidden choice, so it is for the host only. `replay_v` moves apart from `v`.
- Responses have one type, with `status` set to `created`, `decision_open`, `still_running`, `game_over`, `exported` (022), or `error`.
  - `events` is a list. Each event has a `sequence_no`.
  - `decision` appears only when the status is `decision_open`.
  - `error` (a `code` and a `message`) appears on a rejected Answer or on a fatal pipeline error.
  - A rejected Answer returns `decision_open` for the same Decision, with an empty `events` list and an `error`.
  - A pipeline error returns `error` with the events produced before it. The Session refuses every later message.
- Every `u64` is a decimal string, which covers the seed and `ObjectId`. `PlayerId` (`u32`) is a JSON number.
- Every `GameEvent` variant has a mirror type and a `kind` tag. A variant added later is additive and does not bump `v`.
- Error codes: `unsupported_version`, `malformed_message`, `unknown_seat`, `not_your_decision`, `no_open_decision`, `stale_decision`, `invalid_answer`, `game_over`, `setup_invalid`, `pipeline_error`. Adding a code is additive. Removing or renaming one bumps `v`. The code also has `engine_panic` (035) and `decision_pending` (012), both additive.
- `setup` takes an optional `outbox_cap` (023, a JSON number, default 4096). A Seat whose queued events pass it lapses: its events are dropped, and every response to it has empty `events` and `resync: true` until it sends `snapshot`. `resync` is absent otherwise. Both are additive.
- Unknown fields and unknown `kind` values are rejected with `malformed_message`.
- A breaking change bumps `v`. A breaking change is a change to an existing field's name, type, or meaning, or to a kind's required fields. An optional field or a new kind does not bump `v`.
- Pre-release rule (020, built in 023): `v` stays 1 until the first release, which is the first tagged release of `crates/host` or the first host outside this repository, whichever comes first. Until then a breaking change, such as 013's change to `zone_change.object`, lands without a bump. After it, the rule above applies.
- Mismatched `v` is checked on every request, `setup` included, and rejected with `unsupported_version`. There is no separate handshake.
- The rejected-Answer response shape and the `seat`-less `setup` request were derived here from the answers above. The user confirmed both in 017, and the code in `crates/host/src/session.rs` matches them.

