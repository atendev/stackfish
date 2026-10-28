---
id: 049
title: Add unit tests for engine helpers that need a small GameState or layer stubs
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Add local `#[cfg(test)]` unit tests for the next batch of engine helpers that are deterministic but need a small `GameState` or a layer stub to exercise.

## Notes

- This follows ticket 048. Pick helpers from the ticket 047 audit that were skipped because they need a controlled state or a fake layer result.
- Good starting candidates:
  - `FilterSpec::matches` for `FilterController::You`/`Opponent` and multi-field combinations.
  - `ReplacementEffect::matches` and `ReplacementEffect::apply` with a minimal state.
  - `ContinuousEffect` query helpers (`active_for`, `sort_by_timestamp`).
  - `commands::combat` predicates that take a `GameState`.
  - `commands::legality` mode-combination and targeting helpers.
  - `script::decode` chain-word helpers that need a stub runtime.
- Keep each test deterministic and avoid `Engine` mutation or I/O.
- Do not alter production code; `comment_diff.py` should show only test-module additions.

## Resolution

<Filled in at close: the modules covered, the tests added, and any remaining gaps moved to a follow-up ticket.>
