---
id: 107
title: remap_abilities purges cost modifiers and player modifier lists
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
---

## Question

Build the purge that `remap_abilities` lacks. A cost modifier or player modifier that functions from hand outlives its source's cast or land play, because `remap_abilities` drops timing and zone permissions but not these lists (CR 611.3b).

## Notes

- Origin: the Zone permissions entry in `99-backlog.md`, the TODO at the end of `remap_abilities` in `commands/attach.rs`, and 081, which fixed the `retire_object` half.
- Write the failing test first: a cost reducer in hand, cast it, and check the reduction no longer applies. Use the `tdd` skill. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Check that a script re-registering the modifier for its new zone is not purged too (the same rule `timing_permissions` follows through `source_bound`).
- Update `docs/implementation/` and remove the TODO.

## Resolution

<Filled in at close.>
