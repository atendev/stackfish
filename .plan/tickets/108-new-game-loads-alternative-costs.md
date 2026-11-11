---
id: 108
title: new_game loads alternative costs and buyback
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
---

## Question

Build the load step `Engine::new_game` lacks. It loads a script's `operation()` and `target_requirement()` but not `alternative_costs()` or `buyback_cost()`, so a host that builds definitions through `new_game` gets neither.

## Notes

- Origin: the Proof game entry in `99-backlog.md` and the TODO at `engine/pregame.rs:217`. `Engine::build_alternative_costs` and `build_buyback_cost` already exist in `engine/mod.rs`.
- Write the failing test first, with a flashback card loaded through `new_game`. Use the `tdd` skill. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Check whether the replay pin moves. If it does, bump `BEHAVIOUR_REV`.
- Removing the gap also lets the proof game offer `may_cast` at the byte level, which ticket 109 can use.
- If "Build flash costs" lands first, load `flash_cost()` here as well (decided in 128).

## Resolution

<Filled in at close.>
