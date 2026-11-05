---
id: 81
title: cost_modifiers outlive their source
type: task
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Purge a source's `cost_modifiers` when the source leaves, the way `retire_object` purges `land_play_modifiers`, and add a test that pins it.

## Notes

- Origin 076 audit. `retire_object` (`commands/attach.rs:428-431`) purges the hand size, hand reveal, draw limit and land play lists. Nothing purges `cost_modifiers`, and `apply_cost_modifiers` (`types/cost.rs:285`) has no check that the source is still there.
- Check the rules first: a cost reduction from a static ability stops when its source leaves (CR 113.6 and CR 611.3). Slice `phase5/cost-modifier-purge`, CLAUDE.md implementation loop.

## Resolution

`retire_object` purges `state.cost_modifiers` by source, in the same CR 611.3b block as the land play, draw limit, hand size and timing permission lists. Every cost modifier comes from a static ability through `run_card_script`'s drain, so none should outlive its source. `apply_cost_modifiers` gets no liveness check, because the purge keeps the list clean.

Tests: `retire_object_purges_only_the_retired_source` covers cost modifiers, and `a_reduction_ends_once_its_source_leaves_the_battlefield` in `tests/cost_modifiers.rs` destroys a reducer with a real spell and checks that the next spell costs full price. The replay pin did not move, so `BEHAVIOUR_REV` stays 6.

The `remap_abilities` TODO stays: a cost modifier that functions from hand still outlives its source's cast. Built on `phase5/cost-modifier-purge`.
