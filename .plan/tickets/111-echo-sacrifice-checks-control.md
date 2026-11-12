---
id: 111
title: Echo sacrifice checks control
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
---

## Question

Build the control check an unpaid Echo trigger skips. `Game.Ops.SacrificeSelf` checks that the ability's controller still controls the permanent (CR 701.21a), but `UnlessPaidConsequence::SacrificeSelf` goes through `resolve_sacrifice_effect` without it.

## Notes

- Origin: the Echo entry in `docs/lua/not-available-yet.md` and the TODO at `types/ability.rs:208`.
- Write the failing test first. An Echo permanent changes controller before the trigger resolves, and the old controller's trigger must not sacrifice it. `tests/unless_paid_consequence.rs` pushes `SacrificeSelf` directly because it has no Lua builder, so decide in the test whether to add one. Use the `tdd` skill.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch. If a Lua builder appears, update `docs/lua/stubs/game.lua`.

## Resolution

<Filled in at close.>
