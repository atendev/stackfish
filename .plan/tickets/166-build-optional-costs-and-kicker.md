---
id: 166
title: Build optional costs and kicker
type: task
mode: AFK
status: open
blocked_by: [151]
claimed: false
---

## Question

Build `CardDefinition.optional_costs: Vec<OptionalCost>`, announced by `CastSpell.optional_costs: Vec<u32>` and recorded as `StackObject.optional_paid`, so a spell can be kicked, multikicked or have a "you may" additional cost paid, and its effect can read that (CR 118.8b, 702.33a to 702.33f).

## Notes

- Origin 159. The full decision is in `closed/159-optional-additional-costs-and-kicker.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- `OptionalCost { kind: OptionalCostKind { Kicker, Optional, Buyback }, mana, additional, repeatable }`. The loader rejects `repeatable` with `additional` costs and two Buyback entries. The cast rejects an out-of-range index and a count above 1 on a non-repeatable entry.
- Optional mana is concatenated once per count before `cost::apply_cost_modifiers`, and buyback moves there too (CR 601.2f). Add a test that a cost reduction lowers a kicker and a buyback cost. `additional` costs go through the 123 payment-choice path. The mana value does not change (CR 118.8d).
- Buyback folds into the list: `s.buyback_cost()` loads as a Buyback entry, and `bought_back` becomes a query. The wire keeps `pay_buyback` and the Entry's `buyback`, mapped by the host facade, and rejects a cast that sets both. Give the legacy fields a TODO for removal at the next protocol bump.
- Lua: `s.optional_costs()` with `Game.OptionalCosts.Kicker`, `Multikicker` and `Optional`. A fourth `cast` argument to operation functions for spells, and `Game.Ops.IfPaid(op_if, op_else?, which?)`. Update `card-script-basics.md`, `game-constants.md`, `not-available-yet.md` and `docs/lua/stubs/game.lua`.
- Wire: `optional_costs` on the cast action, offers on the Entry, and paid counts on the public stack object, all additive and defaulted. `PROTOCOL_VERSION` stays 1.
- TODOs: a spell copy keeps `optional_paid` (CR 707.10), and repeatable non-mana costs (CR 702.33c).
- Prove it with a kicker spell (Vines of Vastwood with `IfPaid`), a multikicker count, a "you may" sacrifice cost, and buyback through the new path.
- `Engine::new_game` loads `s.buyback_cost()` in `load_script_declarations` (`engine/pregame.rs`). Load `s.optional_costs()` there in its place when buyback moves into the list (found by 108).

## Resolution

<Filled in at close.>
