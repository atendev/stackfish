---
id: 81
title: cost_modifiers outlive their source
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Purge a source's `cost_modifiers` when the source leaves, the way `retire_object` purges `land_play_modifiers`, and add a test that pins it.

## Notes

- Origin 076 audit. `retire_object` (`commands/attach.rs:428-431`) purges the hand size, hand reveal, draw limit and land play lists. Nothing purges `cost_modifiers`, and `apply_cost_modifiers` (`types/cost.rs:285`) has no check that the source is still there.
- Check the rules first: a cost reduction from a static ability stops when its source leaves (CR 113.6 and CR 611.3). Slice `phase5/cost-modifier-purge`, CLAUDE.md implementation loop.

## Resolution

<Filled in at close: the decision and the reason for it.>
