# 27 Constructor-only operation and effect results

Ticket 053, first of four Lua API slices (053, 061, 062, 063) that apply ticket 052.

## What changed

- A `:SetOperation` function returns a `Game.Ops.*` value or `nil`. A raw-key table (`{ gain_life = 1 }`) is an error.
- A `:SetEffect` function returns a `Game.Effects.*` value or `nil`. `nil` is no effect this call. Any other value is an `Err` from `invoke_continuous_effect`.
- `OperationResultTag` (`script/tags.rs`) holds `ResultItem` steps. `result_items` reads one from a returned value or a `Game.Ops.Chain` argument. Plain `GainLife` and `DrawCards` tags are accepted as well.
- New constructors: `Ops.DealDamageTo(pick, amount)`, `Ops.AddCountersTo(pick, counter, amount)`, `Effects.DoublePT(power, toughness)`. `SacrificeChosen`, `DiscardChosen` and `CastSpell` are Rust functions that return `OperationResultTag`.
- `Effects.AddKeyword` takes a name or a list of names.
- `Game.Ops.Chain` stays variadic. A chain with any `OperationResultTag` argument builds an `OperationResultTag`. Otherwise it builds the declared `AbilityEffect::Chain` as before. This amends the list form written in 052.

## Rules in `handle_operation_result`

- `GainLife` applies at once.
- A returned value may hold one pending kind: zone moves (both verbs together), a draw, events (damage and counters together) or a cast.
- Two pending kinds are an error and not a silent drop. Before this slice, a table with `draw_cards` and `deal_damage` lost the damage.

## Open

- TODO: an `OperationOutcome` that carries several requests, so a draw and damage can share one return.
- `stackfish-db` scripts still use the raw keys until ticket 063.
