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
  - Ability index positional fragility (the `PendingTrigger` half fixed by `phase5/ability-stable-id`
    — see `10-triggers.md`; `ActivateAbility`'s own index deliberately kept positional, confirmed
    never actually at risk).

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
  - Multi-ability stable indexing still positional (see `phase5/ability-stable-id` note above).
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

## Slice: cost-modifier-order (merged)

- Branch `phase5/cost-modifier-order`.
- Scope: CR 601.2f — `cost::apply_cost_modifiers` no longer lets `state.cost_modifiers`'
  registration order silently change a spell's resolved cost.
- Verify: 2 new tests (order-independence regression, an adjacent commander-tax edge case caught
  along the way) + all 7 pre-existing `phase5_cost_modifiers.rs` tests unchanged, full workspace
  suite (115 binaries) green.
- Key finding that reshaped the fix: the backlog TODO assumed this needed real CR 616-style
  player-choice pause machinery. It doesn't — CR 601.2f's own text ("total cost is... plus
  increases, minus reductions... If multiple cost reductions apply, the player may apply them in
  any order") is a single arithmetic formula, floored at `{0}` **once**. The old code called
  `ManaCost::apply_cost_delta` once per matching modifier, flooring after *every* delta — provably
  not equivalent to summing first: base `{1}` with deltas `-2` then `+1` floors to `{1}`; the same
  two deltas in the opposite order floor to `{0}`. Real Magic's own "order can matter among
  multiple reductions" carve-out is about CR 118.7's colored-vs-generic-pip interactions, which
  `CostModifier` already doesn't model (generic-only, flat amounts) — for what this engine actually
  implements, sum-then-floor-once is provably order-independent (addition commutes), so no new
  pause/choice UI was needed at all.
- Key changes:
  - `cost::apply_cost_modifiers` sums every matching modifier's `amount` into one net delta, calls
    `ManaCost::apply_cost_delta` once instead of once per modifier.
  - Guards against calling `apply_cost_delta` at all when the net delta is `0` (see bug below).
  - `ManaCost::apply_cost_delta`'s own doc comment corrected — it previously claimed stepwise
    application "still gets the common case... right, since clamping is idempotent past zero,"
    which this slice's own counterexample disproves.
- Bug caught mid-implementation (see `99-backlog.md`'s "Mistakes caught"): `apply_cost_delta(0)`
  isn't a true no-op — it unconditionally drops an *explicit* `Generic(0)` symbol (e.g. an `{X}`
  cost resolved with X=0), since a bare `0` never satisfies its own "adjusted > 0, push it back"
  condition. The old `apply_cost_modifiers` never called it at all when `state.cost_modifiers` was
  empty, so this never showed up; summing-then-always-calling-once exposed it immediately
  (`phase5_x_costs.rs`'s `x_only_cost_with_zero_value_needs_an_explicit_empty_mana_entry`
  regressed). Fixed by skipping the call when the net delta is `0`. Found and fixed the identical
  latent shape at commander tax's own `apply_cost_delta(2 * prior_casts)` call
  (`commands.rs`, CR 903.8) — a first cast (`prior_casts == 0`) called it unconditionally too; an
  `{X}` commander cast for the first time with X=0 would have silently lost its explicit `{0}`.
  Not previously caught by any test (no existing commander test used an `{X}` cost) — new
  regression test added (`phase5_commander_tax.rs`).
- Unresolved:
  - CR 118.7 colored-pip-specific cost reduction — still unmodeled, out of scope (pre-existing
    `CostModifier` boundary).
  - `Game.CreateCostModifier` Lua builder — real now, see `cost-modifier-builder` below.
  - Activated-ability costs — `CostModifier` stays spells-only (pre-existing scope boundary).

## Slice: cost-modifier-builder (merged)

- Branch `phase5/cost-modifier-builder`.
- Scope: `Game.CreateCostModifier(c)` (CR 601.2f) — the missing Lua construction path
  `CostModifier`'s own doc comment named as a TODO since `cost-modifier-order` landed. A direct
  Rust struct literal remained the only way to build one until this slice.
- Verify: 4 new tests (`phase5_cost_modifier_builder.rs`: unit-level self-registration, two
  `Register()`-without-required-field error cases, one full `Engine`-driven end-to-end PlayLand →
  CastSpell test proving the reduced cost is actually honored at payment time), full workspace
  suite green.
- Key changes:
  - `CostModifierBuilder` (script.rs) — mirrors `StaticAbilityBuilder`/`ReplacementEffectBuilder`'s
    queue-and-drain shape exactly. `:SetFilter(table)` reuses `decode_filter_table` verbatim (same
    option-bag `Game.Costs.Sacrifice`/`Discard`/`TapPermanents` already decode through — no new
    filter-table shape). `:SetAmount(n)` sets the flat generic delta.
  - `ScriptRegistry.pending_cost_modifiers: Vec<CostModifier>` — `:Register()` pushes with a
    placeholder `controller: PlayerId(0)`; `run_card_script`'s drain step resolves the real value
    (this effect's source's own live controller) from `state.object(source).controller`, since the
    builder has no `&GameState` inside a Lua callback to read it from — same "placeholder now,
    patch at drain" pattern `AbilityId`/`timestamp` already use for `pending_triggered`/
    `pending_static`, just for a new field kind (a live lookup, not a freshly-minted id).
  - `Game.CreateCostModifier` registered in `bind_game_globals` alongside the other three
    `Game.Create*` builders.
- Unresolved: none new — this closes the builder gap outright at `CostModifier`'s existing scope
  (spells only, flat generic amount, no CR 118.7 colored-pip modeling — all pre-existing
  boundaries, untouched by this slice).
