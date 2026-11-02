---
id: 028
title: Audit unordered HashMap walks over game state
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Find every walk over an unordered `HashMap` in game state whose order can reach a host or the ledger, and make each one deterministic.

## Notes

- Origin 014.
- 014 fixed three. `0df02f7` (merged with 025) sorted five state-based action collectors in `sba.rs`.
- The 500-seed proof run found no more, but the proof cards cover only part of the engine.

## Resolution

Resolved. The audit found 26 non-test walks over `GameState::objects` and no other hash-ordered walk that reaches a host or the ledger. Every other `HashMap` and `HashSet` in `crates/*/src` is a lookup, a membership set or sorted.

Of the 26, 17 are already sorted (the 8 in `sba.rs` and 9 elsewhere), 3 are order-irrelevant, 2 are unique by the `ObjectLineage` invariant (`madness.rs`, `reanimation.rs`) and 4 reach a Decision candidate list unsorted: `pending_devour_choice`, `pending_amplify_choice`, `pending_copy_choice` in `replacement.rs`, and `legal_entry_attach_candidates` in `commands/attach.rs`. The proof cards reach none of them, which is why the 500-seed run found nothing.

Pick Answers name the candidate by identity, so recorded replays stay valid. The bot picks by list position, so the same seed can play a different game across processes. No shuffle reads hash order.

Decision: change `GameState::objects` to `BTreeMap<ObjectId, GameObject>`, so every walk is ascending `ObjectId`, and pin the order for the 4 walks with unit tests. Sorting at each site has missed new walks in three rounds (3 in 014, 5 in 0df02f7, 4 here). The type change fixes the class with no new dependency. The other maps stay `HashMap` because none is walked. No `BEHAVIOUR_REV` bump is expected. The ObjectId order is a determinism convention: CR 101.4 and 603.3b order players only, CR 704.3 prescribes no object order and CR 613.7 timestamps are for layers.

Lua `pairs` order stays deferred under the existing TODOs (`docs/DESIGN.md:212`, `crates/host/src/session.rs:190`). Build work is 073 and 074, and 075 decides the lint config.

Lint probe (clippy 0.1.98): `clippy::iter_over_hash_type` catches `for x in &map` but no method form; `disallowed-methods` catches `.iter()`, `.values()`, `.keys()`, `.drain()` but needs a new `clippy.toml`; neither catches `.into_iter()` method calls, `extend` or `from_iter`.
