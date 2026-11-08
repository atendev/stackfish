---
id: 140
title: Build granted ward
type: task
mode: HITL
status: open
blocked_by: [139]
claimed: false
---

## Question

Grant Ward through a layer-6 effect and move printed Ward onto the same path, as "Ward with non-mana costs, player hexproof and hexproof from" decided.

## Notes

- Read `closed/101-ward-costs-and-player-hexproof.md`.
- A layer-6 `EffectPrimitive` holds the cost and collects into `EffectiveCharacteristics`. `RemoveAllAbilities` clears it (CR 613.1f).
- On `BecameTarget`, `collect_triggers` emits one `PendingTrigger` per instance, with `source` the warded permanent and `controller` its effective controller. `ability` is one template carrying only the effect, target declaration and cost. Confirm how `collect_triggers` fills those fields for a granted instance before building.
- `Game.RegisterWard` keeps its signature and becomes a self-only static. Test that printed Ward stops under `RemoveAllAbilities`, that two instances trigger separately (CR 113.2c, 603.2c), and that an Aura with a different controller grants correctly.
- Correct the "(CR 702.21b)" citation on instances in `script/globals.rs`.
- Update the game stub in `docs/lua/stubs/game.lua`, and `docs/lua/not-available-yet.md`.

## Resolution

<Filled in at close.>
