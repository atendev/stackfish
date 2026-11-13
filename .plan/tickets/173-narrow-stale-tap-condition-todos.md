---
id: 173
title: Narrow the stale becomes-tapped and becomes-untapped TODOs
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

The "becomes tapped/untapped condition" TODOs in `crates/engine/src/types/ability.rs` are out of date: 113 shipped the `BecomesTapped` and `BecomesUntapped` condition tags, and 147 routed every untap through `Engine::attempt_untap`. Trim each TODO to what is still missing, or show that nothing is missing.

## Notes

- Found by 147. Comment-only pass, so keep every TODO that names a real gap and run `python3 tools/comment_diff.py` for exit code 0. A TODO is removed only if the gap is closed, and the report says why.

## Resolution

<Filled in at close.>
