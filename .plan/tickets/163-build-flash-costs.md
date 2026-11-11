---
id: 163
title: Build flash costs
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Build `FlashCost { mana: Option<ManaCost>, additional: Vec<AdditionalCost> }` as `CardDefinition.flash_cost`, announced by `CastSpell.pay_flash_cost` and listed on `CastEntry.flash_cost: FlashCostOffer { None, Optional, Required }`, so a card can be cast as though it had flash when the cost is paid (CR 601.3c).

## Notes

- Origin 128. The full decision is in `closed/128-flash-bought-with-an-extra-cost.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- `can_cast_now` does not change. One shared helper decides when the cost is required (priority held, `can_cast_now` false, not timing-bypassed) for `commands/cast.rs` and `cast_entries`. `cast_options` offers `Optional` at most.
- The mana part is appended after cost modifiers and commander tax, as buyback's is, and `additional` joins the mode costs in `cost.additional` (CR 601.2f, 118.9d).
- TODOs: behold for Molten Exhale, the CR 601.2f reduction on the appended mana, and a pointer to "Optional additional costs and kicker", which folds buyback into one list and keeps this cost separate.
- Lua API change: `s.flash_cost()` returns a list of `Game.Costs.*` values through `decode_cost_component_value`, with `Game.Costs.Mana` at most once and an empty list a load error. Update `docs/lua/card-script-basics.md` beside `buyback_cost()` and `docs/lua/stubs/game.lua`.
- Wire: both fields are additive with serde defaults (`none`, false), so `PROTOCOL_VERSION` stays 1. The bot pays on `Required` only, so the replay pin should not move.
- Prove it with a Breaking Wave scenario cast on an opponent's turn, plus the unit, Lua and wire tests the decision lists.
- `Engine::new_game` loads script declarations in `load_script_declarations` (`engine/pregame.rs`). Load `s.flash_cost()` there too, with a value set in Rust winning (found by 108).

## Resolution

<Filled in at close.>
