---
id: 074
title: Proof cards for ordered choices
type: task
mode: HITL
status: open
blocked_by: [073]
claimed: true
---

## Question

Add proof or fixture cards that reach Devour, Amplify, Clone and Equipment attach from a search or token, and add them to the seed loop in `the_same_seeds_give_a_byte_identical_game`.

## Notes

- Origin 028. The proof set has no card that reaches the 4 formerly unsorted walks, so a regression there is invisible to the 500-seed run.
- Moves the proof game: re-pin the hash at `crates/host/tests/replay.rs:371` and bump `BEHAVIOUR_REV` by hand, in its own commit.
- Blocked by 073 so the new cards prove the fixed order, not the old one.

## Resolution

<Filled in at close: the decision and the reason for it.>
