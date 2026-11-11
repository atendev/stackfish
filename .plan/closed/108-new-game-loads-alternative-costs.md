---
id: 108
title: new_game loads alternative costs and buyback
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
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

`Engine::new_game` loads `s.alternative_costs()` and `s.buyback_cost()` in `load_script_declarations` (`engine/pregame.rs`), cached once per distinct script with `operation()` and `target_requirement()`. A value set in Rust wins per field: a non-empty `alternative_costs` or a `Some` `buyback_cost`. A card is skipped only when all four fields are already set, so a card whose effect and target come from Rust still gets its script costs, and a broken script on such a card logs a `ScriptError`. A failed cost build is not cached, so every copy logs it. The replay pin did not move and `BEHAVIOUR_REV` stays 8. `flash_cost()`, `additional_costs()` and `optional_costs()` are not built yet, so tickets 163, 151 and 166 note that the loader must load them. Built in auto mode, then merged.
