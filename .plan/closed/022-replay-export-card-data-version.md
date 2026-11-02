---
id: 022
title: Replay export and card data version
type: task
mode: HITL
status: closed
blocked_by: [020]
claimed: false
---

## Question

Build the replay export and the card data version record in the shape 020 decides.

## Notes

- Origin 001, 006 and 012. The shape comes from 020.

## Resolution

Built as 020 decided. `export_replay` (no seat, empty body) returns status `exported` with a `replay` field: `replay_v`, the engine build id, a fingerprint per card key, the setup bytes as sent, and every accepted Answer as `{seat, decision, reply}`. Refused Answers and advance budgets are not recorded. `Session::from_replay` checks the version, build id and every fingerprint before any card loads and names each key that differs. It then plays the Answers and runs on to the next Decision or the end. `Replay` lets a host step one Answer at a time.

A fingerprint is FNV-1a over a length-prefixed encoding, never `Debug` text. The carddb source hashes the row, sorted keywords and active script. The fixture source hashes printed characteristics and script (TODO for `effect`, `modes` and the cost fields). The build id carries a hand-bumped `BEHAVIOUR_REV`, and a pinned proof-game hash fails until it moves.

020's open points: Lua could reach `os` and `math.random`; scripts now get only `table`, `string`, `utf8`, `math` and `coroutine`, with `math.random`, `math.randomseed`, `dofile` and `loadfile` removed. Still open: the `HashMap` audit (028), unseeded Lua `pairs` order over string keys, and an omniscient stream digest, without which a swapped legal Answer goes undetected. A halt from outside the recorded inputs does not reproduce on load. The outbox cap in 023 must not lapse a Seat during the replay drive.
