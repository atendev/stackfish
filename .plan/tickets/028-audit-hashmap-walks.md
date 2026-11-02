---
id: 028
title: Audit unordered HashMap walks over game state
type: task
mode: HITL
status: open
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

<Filled in at close: the decision and the reason for it.>
