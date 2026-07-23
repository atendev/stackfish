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

## Slice: copy-exceptions (merged)

- Branch `phase5/copy-exceptions`.
- Scope: CR 707.9's copy-effect exceptions/modifications - closes the "no CR 707.9 exception support,
  always a bare copy" TODO the layer-engine-copy work left on both real copy-producing sites
  (`AbilityEffect::BecomeCopy`, `TokenSpec::CopyOf`). CR 707.9's real shapes are open-ended (arbitrary
  card text), so narrowed to the two overwhelmingly common ones - matching this codebase's own
  precedent everywhere else of modeling a fixed, parametrized primitive rather than a general
  text-modification system.
- Verify: 13 tests in `phase5_layer_engine_copy.rs` (7 pre-existing regression + 6 new), full
  workspace suite green.
- Key changes:
  - `CopyExceptions { set_power_toughness: Option<(i32, i32)>, add_keywords: KeywordSet }`
    (`types/ability.rs`, co-located with `BecomeCopy`/`CopyOf`) - 707.9b (base P/T override, the
    single most common real exception on Clone-variant cards) and 707.9a (gains an ability as part of
    copying, union with whatever the copy already grants). `Default` is a bare copy, unaffected.
    `CopyExceptions::apply(&mut CardDefinition)` mutates a snapshot in place, shared by both real call
    sites so they can't drift.
  - `AbilityEffect::BecomeCopy`/`TokenSpec::CopyOf` each gained a `CopyExceptions` field - small,
    contained ripple (a handful of real match sites, not the `CardDefinition`-scale problem a past
    slice found for a *new* field on that particular struct).
  - `Game.Ops.BecomeCopy(opts?)` / `Game.Tokens.CopyOf(objectId, opts?)` - `decode_copy_exceptions`
    shared by both, `{ power = N, toughness = N }` (both required together - a lone `power` or
    `toughness` is rejected at load time, not silently ignored) and/or `{ keywords = { "Flying", ... } }`.
- Unresolved:
  - CR 707.9c (characteristics specifically *not* copied, original retained) - the inverse of 9a/9b,
    no real card needed it yet.
  - CR 707.9e/9g (an exception tied to "was this the most recent copy effect applied") - this
    snapshot model has no such history to consult; would need real design work, not a narrow add.
  - Everything `layer-engine-copy` itself already named as out of scope (ETB-replacement "enters as a
    copy," CR 707.10 stack-copy, CR 707.8 double-faced-card copying) - unchanged.

## Fix: layer 5 base color (CR 105.2a/202.2)

- `effective_characteristics` now seeds layer 5 from `CardDefinition::colors()` instead of raw `color_indicator`.
- 2 regression tests added.

## Slice: layer7d-counter-interleaving (merged)

- Branch `phase5/layer7d-counter-interleaving`.
- Scope: CR 613.4c's literal text — `+1/+1`/`-1/-1` counters apply in layer 7c, timestamp-interleaved
  against `ModifyPT`/`DoublePT` effects, not pre-netted into the layer 7a base the way this engine
  used to. The old shape was a real bug, not just an approximation, caught by CR 613.5's own Gray
  Ogre worked example: a counter added before a `SetPT` ("becomes 0/1") effect got baked into a base
  that override then discarded outright, silently losing the counter — every `SetPT`-plus-counter
  combination on the board was wrong before this slice, not just a narrow edge case.
- Verify: 5 new tests (`phase5_layer7d_counter_interleaving.rs`, including CR 613.5's own worked
  example reproduced exactly), full workspace suite green — zero pre-existing tests broke, confirming
  the doc comment's own prior admission that "no test exercises" the interaction.
- Key finding that reshaped the plan: `CounterSet`'s own backing field (`Vec<(CounterKind, i32)>`) is
  *private* — nothing outside `types/counter.rs` ever constructs it directly (every real caller goes
  through `CounterSet::new()`/`.add()`/`.count()`), so redesigning its internals to track one
  timestamped batch per `add` call instead of one coalesced running total touched zero `GameObject`/
  `PlayerState` construction sites — only `CounterSet::add`'s own 3 real call sites (plus 8 mechanical
  test-file updates) needed a new `SequenceNo` parameter. A different shape entirely from the
  `CardDefinition`-scale ripple problem past slices have flagged for *new struct fields*.
- Key changes:
  - `CounterSet.counters: Vec<(CounterKind, i32, SequenceNo)>` — one batch per `add` call.
    `count(kind)` sums matching batches (unchanged behavior for every caller). New
    `pub(crate) batches(kind) -> impl Iterator<Item = (i32, SequenceNo)>` for layer 7c to read back.
  - `CounterSet::add` gained a `timestamp: SequenceNo` parameter — `commands::apply_damage_event`'s
    two real sites (Infect poison, Wither/Infect `-1/-1`) and `CommitEvents`'s `CounterAdded` handler
    each allocate a fresh one via `GameState::allocate_sequence_no` right at the point of adding.
  - `layers::resolve_base_pt` no longer touches counters at all (CDA/printed base only) — `PTValue::
    net_with_counters`/`net_counters` themselves are untouched and still used by `script.rs`'s
    separately-scoped `card_summary_to_lua` (explicitly "not layer-resolved," a different, pre-existing
    simplification this slice doesn't touch).
  - `effective_characteristics`'s layer 7c loop merges `obj.counters.batches` for both P/T kinds
    (`+1/+1` as positive deltas, `-1/-1` as negative) into the existing `applicable_effects_in_layer`
    walk, comparing each batch's timestamp against each real effect's own `timestamp` field — a
    counter has no CR 613.8a dependency of its own, so it's merged in by timestamp comparison against
    the already dependency-resolved effect order, not re-sorted into it.
- Unresolved:
  - The narrow theoretical edge case where a counter's timestamp falls *between* a `DoublePT` and the
    same-field `ModifyPT` it depends on, in a way dependency reordering has moved out of raw timestamp
    order — not exercised by any real card, same "good enough" tolerance `order_by_dependency`'s own
    dependency-loop fallback already accepts elsewhere.

## General layer notes

- Layer order in `effective_characteristics`: 2 control, 4 type, 5 color, 6 abilities, 7a CDA, 7b set P/T, 7c modify P/T interleaved with counters (CR 613.4c, `phase5/layer7d-counter-interleaving`), 7e switch.
- Dependency ordering only active in 7c due to `DoublePT`; other layers use timestamp order. Counters merge into 7c by timestamp comparison, not the dependency graph itself (they have no dependency of their own).
