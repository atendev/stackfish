---
id: 048
title: Add unit tests for pure engine helpers
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Add local `#[cfg(test)]` unit tests for the first batch of pure engine helpers that ticket 047 surfaced.

## Notes

- This is a build ticket. Follow the `CLAUDE.md` vertical slice workflow: write a requirements report first, create a `phase5/<name>` branch, get approval before every commit, and report when done.
- Start with helpers that need no `Engine`/`GameState`: `color_identity.rs`, `types/zone.rs`, `types/object.rs`, `types/player.rs`, `types/counter.rs`, `types/card.rs`, then move to `types/mana.rs` and `types/cost.rs`.
- Leave helpers that need a small `GameState` or layer stubs for a later ticket.
- Every `#[cfg(test)]` pass must keep `comment_diff.py` exit code 0.

## Resolution

<Filled in at close: the modules covered, the tests added, and any remaining gaps moved to a follow-up ticket.>
