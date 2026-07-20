# 09-layers

## Slice: layer-engine-basics (merged)

- Branch `phase5/layer-engine-basics`.
- Scope: CR 613 layers 6 (abilities) and 7b/7c (P/T set/modify); timestamp ordering only.
- Verify: 11 new tests, 189 total, 0 failed.
- Key changes:
  - `Layer` derives + TODO comments for unbuilt layers.
  - `EffectPrimitive::{SetPT, ModifyPT, AddKeyword}`.
  - `EffectPayload::{Primitive, Script}`; `EffectFilter`; `AffectedFilter::{Primitive, Script}`.
  - `ContinuousEffect.affected` + `effect` replace placeholder `affected_predicate`.
  - `ContinuousEffect::applies_to` real for `Primitive`.
  - `EffectiveCharacteristics` gained `keywords`.
  - `layers::effective_characteristics` real; starts from base P/T netted with counters, applies layers 6/7b/7c.
  - `sba::apply_creature_death` uses effective toughness and Indestructible keyword.
- Unresolved:
  - Layers 1-5, 7a, 7d(as real layer step), 7e, CR 613.6-8 dependency system.
  - `AffectedFilter::Script`/`EffectPayload::Script` dead.

## Slice: layer-engine-consumers (merged)

- Branch `phase5/layer-engine-consumers`.
- Scope: wire combat-damage power and Flying/Reach/Menace blocking legality into layer engine.
- Verify: 4 new tests, 193 total, 0 failed.
- Key changes:
  - `commands::effective_power(state, id)` and `effective_keywords(state, id)` helpers.
  - `DeclareBlockers` and `AssignCombatDamage` use layer engine.
  - Also fixed Lifelink/Wither/Infect/Deathtouch keyword reads in damage application.
- Unresolved:
  - All other layer gaps from layer-engine-basics.

## Slice: layer-engine-types-colors (merged)

- Branch `phase5/layer-engine-types-colors`.
- Scope: CR 613 layers 4/5, additive only (`AddCardType`, `AddColor`).
- Verify: 6 new tests, 195 total, 0 failed.
- Key changes:
  - `EffectPrimitive::{AddCardType(CardTypeSet), AddColor(ColorSet)}`.
  - `effective_characteristics` applies layer 4 then 5.
  - `sba::apply_creature_death` checks effective card type (animated land dies to damage).
- Unresolved:
  - `SetCardTypes`/`SetColors` overwrite shapes; supertypes/subtypes; `CardDefinition::colors()` still TODO at that point (later fixed).

## Slice: layer-engine-cda (merged)

- Branch `phase5/layer-engine-cda`.
- Scope: CR 613 layer 7a characteristic-defining P/T.
- Verify: 8 new tests, 203 total, 0 failed.
- Key changes:
  - `PTValue::CharacteristicDefining { display, script: Option<ScriptReference> }`.
  - `ScriptRuntime::invoke_cda(source_code, source, controller)`.
  - `effective_characteristics` gained `&ScriptRuntime` param; `resolve_base_pt` resolves CDA before 7b/7c.
  - Shared `net_counters` helper for counter netting.
- Ripple: `execute_command`, `effective_power`, `check_state_based_actions` all gained `&ScriptRuntime`.
- Unresolved:
  - `AffectedFilter::Script`/`EffectPayload::Script` still dead.

## Slice: layer-engine-switching (merged)

- Branch `phase5/layer-engine-switching`.
- Scope: CR 613.4d layer 7e power/toughness switching.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `EffectPrimitive::SwitchPT`.
  - New loop after 7c in `effective_characteristics`.
  - `Game.Layers.PowerToughnessSwitching` / `Game.Effects.SwitchPT`.

## Slice: layer-engine-dependency (merged)

- Branch `phase5/layer-engine-dependency`.
- Scope: CR 613.6-8 dependency system within a layer/sublayer.
- Verify: 5 new tests, 614 total, 0 failed.
- Key changes:
  - Added `EffectPrimitive::DoublePT { power, toughness }` to create a real dependency (reads running value).
  - `layers::order_by_dependency` + `depends_on` predicate.
  - `applicable_effects_in_layer` calls ordering instead of plain timestamp sort.
  - Dependency re-evaluation falls out because query is recomputed from scratch each call.
- Unresolved:
  - `Script`-payload effects treated as independent.
  - Cross-layer dependency not modeled (CR 613.8 same-layer only).

## Slice: layer-engine-control (merged)

- Branch `phase5/layer-engine-control`.
- Scope: CR 613.1b/108.4 control-changing effects — query half only.
- Verify: 4 new tests, 618 total, 0 failed.
- Key changes:
  - `EffectPrimitive::GainControl(PlayerId)` (layer 2, override semantics).
  - `layers::effective_controller(state, script_runtime, object)`.

## Slice: layer-engine-control-consumers (merged)

- Branch `phase5/layer-engine-control-consumers`.
- Scope: migrate real `GameObject.controller` read-sites to `effective_controller`.
- Verify: 8 new tests, 626 total, 0 failed.
- Key changes:
  - Migrated `target_is_legal`, `apply_damage_event` Lifelink destination, `ActivateAbility` "you control", combat declaration controllers, `has_summoning_sickness`, `untap_step`, `find_legend_rule_group`.
  - `FilterSpec::matches` gained `object_controller: PlayerId` param; `PaymentPlan::validate` gained `&ScriptRuntime`.
  - `ReplacementEffect::matches` gained `&ScriptRuntime` param.
- Excluded (deliberate):
  - `layers.rs` internal filtering (`applicable_effects_in_layer`/`ContinuousEffect::applies_to`) stays on raw field to avoid recursion.
  - Hand/Graveyard/Command/Library/stack-zone controller checks stay raw (no `GainControl` can reach there).

## Slice: layer-engine-text (merged)

- Branch `phase5/layer-engine-text`.
- Scope: CR 613.1c/612.8 name-setting only.
- Verify: 5 new tests, 638 total, 0 failed.
- Key changes:
  - `EffectPrimitive::SetName(String)`; `EffectiveCharacteristics.name` already existed and `find_legend_rule_group` was already using it — renamed legendary participates in legend rule for free.
  - `EffectPrimitive` dropped `Copy` derive.
- Unresolved:
  - General CR 612.2 text substitution, single-card rules (612.5-612.10).

## Fix: layer 5 base color (CR 105.2a/202.2)

- `effective_characteristics` now seeds layer 5 from `CardDefinition::colors()` instead of raw `color_indicator`.
- 2 regression tests added.

## General layer notes

- Layer order in `effective_characteristics`: 2 control, 4 type, 5 color, 6 abilities, 7a CDA, 7b set P/T, 7c modify P/T, 7d counters folded into base, 7e switch.
- Dependency ordering only active in 7c due to `DoublePT`; other layers use timestamp order.
