---
id: 107
title: remap_abilities purges cost modifiers and player modifier lists
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build the purge that `remap_abilities` lacks. A cost modifier or player modifier that functions from hand outlives its source's cast or land play, because `remap_abilities` drops timing and zone permissions but not these lists (CR 611.3b).

## Notes

- Origin: the Zone permissions entry in `99-backlog.md`, the TODO at the end of `remap_abilities` in `commands/attach.rs`, and 081, which fixed the `retire_object` half.
- Write the failing test first: a cost reducer in hand, cast it, and check the reduction no longer applies. Use the `tdd` skill. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Check that a script re-registering the modifier for its new zone is not purged too (the same rule `timing_permissions` follows through `source_bound`).
- Update `docs/implementation/` and remove the TODO.

## Resolution

Built on `phase5/remap-abilities-purge-cost-modifiers`. `remap_abilities` drops the cost, hand size, hand reveal, draw limit and land play modifiers keyed to the old id, the same purge `retire_object` does (CR 611.3b, 400.7). None of these modifiers has a duration, so there is no Timed exception. Every call site re-runs the card's script under the new id after the purge, so a modifier the script registers for the new zone stays. A cast fixes its cost before the move, so a card's own hand-time reduction still applies to it. Tests: a unit test over all five lists in `commands/tests.rs`, and land-play and cast tests in `cost_modifier_builder.rs`. The TODO in `attach.rs` and the backlog item are gone.
