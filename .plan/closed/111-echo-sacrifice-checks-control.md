---
id: 111
title: Echo sacrifice checks control
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build the control check an unpaid Echo trigger skips. `Game.Ops.SacrificeSelf` checks that the ability's controller still controls the permanent (CR 701.21a), but `UnlessPaidConsequence::SacrificeSelf` goes through `resolve_sacrifice_effect` without it.

## Notes

- Origin: the Echo entry in `docs/lua/not-available-yet.md` and the TODO at `types/ability.rs:208`.
- Write the failing test first. An Echo permanent changes controller before the trigger resolves, and the old controller's trigger must not sacrifice it. `tests/unless_paid_consequence.rs` pushes `SacrificeSelf` directly because it has no Lua builder, so decide in the test whether to add one. Use the `tdd` skill.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch. If a Lua builder appears, update `docs/lua/stubs/game.lua`.

## Resolution

An unpaid Echo trigger makes the CR 701.21a control check that `Game.Ops.SacrificeSelf` makes. Both paths call one helper, `sacrifice_self_queue`. `AwaitingUnlessPaidChoice` keeps the ability's controller, so `decline_unless_paid` can run the check after the stack object is popped. The controller can still pay, and a declined trigger for a permanent someone else controls sacrifices nothing and still finishes resolving. No Lua builder was added, so the test still pushes the trigger by hand and its TODOs stay. `an_echo_trigger_does_not_sacrifice_a_permanent_its_controller_lost` pins the fix. The replay pin did not move. Built in auto mode, then merged.
