---
id: 151
title: Build spell additional costs
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Build `CardDefinition.additional_costs: Vec<AdditionalCost>`, declared by `s.additional_costs()`, appended to `CostSpec.additional` in `commands/cast.rs` beside the mode costs (CR 118.8, 601.2f).

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Existing variants only. A real card such as Village Rites (sacrifice a creature) or a discard spell proves it in a scenario.
- The field touches every `CardDefinition` literal. Check whether a constructor or `Default` keeps the ripple small.
- The cost applies whether the mana cost or an alternative cost is paid. A free cast still pays it (CR 118.9d, 601.2f).
- Lua API change: `docs/lua/card-script-basics.md` beside `buyback_cost()`, `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`.
- `Engine::new_game` loads script declarations in `load_script_declarations` (`engine/pregame.rs`). Load `s.additional_costs()` there too, with a value set in Rust winning (found by 108).

## Resolution

<Filled in at close.>
