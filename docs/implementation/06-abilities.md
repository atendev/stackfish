# 06-abilities

## Slice: activate-ability-mana (merged)

- Branch `phase5/activate-ability-mana`.
- Scope: first real activated ability — mana ability taps source and adds mana (CR 605).
- Verify: 8 new tests, 48 total, 0 failed.
- Key changes:
  - `AbilityEffect::AddMana(Vec<ManaUnit>)`; `RegisteredAbility.effect: Option<AbilityEffect>`.
  - `ManaPool::add_unit` real.
  - `GameEventKind::Tapped { object }`, `ManaAdded` retargeted to `Vec<ManaUnit>`.
  - `ActivateAbility` arm validates actor priority/control/kind, taps source, adds mana; no stack, no payment pause.
  - `collect_triggers` no-op guard widened to "no triggered ability".
- Deviations:
  - Scenario crate gained `CardSpec.mana_ability`, `ActionSpec::activate_ability`, `AssertionSpec::Tapped`.
- Unresolved:
  - Summoning sickness for tap-cost abilities (later fixed by combat-declarations slice).
  - Non-mana activated abilities.
  - Ability index positional fragility.

## Slice: activated-ability-costs (merged)

- Branch `phase5/activated-ability-costs`.
- Scope: general activated ability costs (CR 602.2b) — Sacrifice/Discard/TapPermanents/LifePayment plus `Game.CreateActivatedAbility`.
- Verify: 3 unit tests + 9 integration + 5 self-registration, 236 total, 0 failed.
- Key changes:
  - Deleted `AbilityCost` enum; `RegisteredAbility.cost: Option<CostSpec>`.
  - `AdditionalCost::TapSelf` added.
  - `FilterSpec::matches(object, viewer)` real.
  - `PaymentPlan::validate(spec, state, payer, source)` real.
  - `commands::commit_payment` performs side effects (tap/move/lose life).
  - Non-mana `ActivateAbility` routes through `AwaitingPayment`.
  - `Game.Costs = { TapSelf, Sacrifice(filter), Discard(filter), TapPermanents(filter,count), Life(n) }`.
  - `ActivatedAbilityBuilder` with `:AddCost`/`:SetOperation`/`:Register()`.
- Deviations:
  - `PendingPaymentRequest.source: ObjectId` added.
- Unresolved:
  - TOML shape for non-mana payments.
  - Multi-ability stable indexing still positional.
  - `{X}`/alternative/hybrid/snow/Phyrexian costs on abilities.
  - `:SetTarget` not bound.

## Slice: ability-mana-cost-strings (merged)

- Branch `phase5/ability-mana-cost-strings`.
- Scope: parse `"{1}{R}"` for activated ability costs.
- Verify: 9 unit tests + 5 integration, 0 failed.
- Key changes:
  - `ManaCost::parse(s) -> Result<ManaCost, ManaCostParseError>`.
  - Handles generic, colored, colorless, snow, X, Phyrexian, hybrid, hybrid-Phyrexian.
  - `script.rs` generalized `CostTag` to `CostComponent { Additional(AdditionalCost), Mana(ManaCost) }`.
  - `Game.Costs.Mana("{1}{R}")` binding; second call on one builder errors.
- Unresolved:
  - `{C}` parses but unpayable.
  - `{X}` on activated abilities has no `x_value` announcement slot.

## Slice: mana-add-choice (merged)

- Branch `phase5/mana-add-choice`.
- Scope: "add one mana of any color" / `Game.Ops.AddMana`.
- Verify: 9 new tests, 562 total, 0 failed.
- Key changes:
  - `AbilityEffect::AddManaChoice { count, choices: ColorSet }`.
  - `PlayerActionKind::ActivateAbility.chosen_colors: Vec<Color>`.
  - Color choice locked in at activation (CR 602.2b), no pause.
  - `ColorSet::contains_color`.
  - `Game.Ops.AddMana(letter, count?)` and `Game.Ops.AddManaChoice(count, colors)`.
  - `mana::parse_color_letter` made public for scenario TOML.
- Unresolved:
  - Mixed-color single call, snow, restricted mana, paired triggered abilities.

## Slice: static-ability-self-registration (merged)

- Branch `phase5/static-ability-self-registration`.
- Scope: `Game.CreateStaticAbility(c)` builder self-registers `ContinuousEffect`.
- Verify: 8 new tests, 219 total, 0 failed.
- Key changes:
  - `StaticAbilityBuilder` mirrors triggered builder; `ScriptRegistry.pending_static` queue.
  - Timestamp allocated at drain time.
  - `retire_object` also purges `state.continuous_effects`.
  - `Game.Layers`/`Game.Effects` tag tables; `validate_layer_effect_pairing`.
- Unresolved:
  - Only 5 real layers exposed.
  - `AffectedFilter::Script`/`EffectPayload::Script` dead (later fixed).
  - Always `Permanent` duration.
  - No conditional gating.

## Slice: static-ability-duration (merged)

- Branch `phase5/static-ability-duration`.
- Scope: `StaticAbilityBuilder::SetDuration(Game.Durations.*)`.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `StaticAbilityBuilder.duration: EffectDuration` (default `Permanent`).
  - `:SetDuration` decodes `UntilEndOfTurn`/`UntilYourNextTurn`/`UntilEndOfCombat`/`Permanent`.
- Unresolved:
  - `WhileConditionHolds`/`Custom` durations unexposed.

## Slice: conditional-continuous-effects (merged)

- Branch `phase5/conditional-continuous-effects`.
- Scope: CR 611.2c — continuous effect conditional on controller controls at least N matching permanents.
- Verify: 10 tests (4 unit + 6 integration + 1 TOML), 669 total, 0 failed.
- Key changes:
  - `ContinuousEffect.condition: Option<InterveningIfSpec>`.
  - `InterveningIfPrimitive::ControllerControlsAtLeast { filter: FilterSpec, count }`.
  - `applicable_effects_in_layer` checks condition before affected-filter.
  - `StaticAbilityBuilder` gained `:SetCondition(Game.InterveningIf.* | function)`.
  - Scenario `ContinuousEffectSpec.condition_card_types`/`condition_count`.
- Deviations:
  - `InterveningIfPrimitive`/`InterveningIfSpec` dropped `Copy` (due to `FilterSpec`); call sites `.clone()`.
  - Reads raw `GameObject.controller`, not `effective_controller` (deliberate, avoids recursion).
- Unresolved:
  - `UntilYourNextTurn`/`UntilEndOfCombat` drains already real (prior slices).

## Slice: lose-all-abilities (merged)

- Branch `phase5/lose-all-abilities`.
- Scope: CR 613.1f `EffectPrimitive::RemoveAllAbilities` (keyword half).
- Verify: 7 new tests, 0 failed.
- Key changes:
  - `EffectPrimitive::RemoveAllAbilities` (unit variant).
  - Layer-6 fold resets `keywords` accumulator at effect position.
  - `Game.Effects.RemoveAllAbilities`.
- Unresolved:
  - Does not strip `RegisteredAbility` entries (custom triggered/activated abilities).
