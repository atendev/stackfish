# 07-targeting

## Slice: targeting-basics (merged)

- Branch `phase5/targeting-basics`.
- Scope: CR 115/601.2c-e/608.2b — fixed-count primitive target requirements.
- Verify: 8 new tests, 211 total, 0 failed.
- Key changes:
  - `types/target.rs`: `TargetFilter` (`Object { card_types, controller, zone }`, `Player`, `AnyTarget`), `TargetRequirement { filter, count }`, `TargetSpec { Primitive, Script }`.
  - `CardDefinition.target_requirement: Option<TargetSpec>`; `RegisteredAbility.target` retyped to `Option<TargetSpec>`.
  - `StackObject.target_requirement` baked at push.
  - `commands::target_is_legal`/`validate_targets` validate count, dedup, filter (zone, card types, controller, "any target").
  - `ResolveStackTop` rechecks targets; if all illegal, fizzles and logs `Fizzled`.
  - `move_resolving_spell` helper extracted.
- Unresolved:
  - No "up to N", no multiple target words, no partial fizzle, `TargetSpec::Script` dead, no LKI for target source leaving.

## Slice: multi-target-resolution (merged)

- Branch `phase5/multi-target-resolution`.
- Scope: CR 601.2d/608.2b partial fizzle — effects apply to each legal target independently.
- Verify: 5 new tests, 652 total, 0 failed.
- Key changes:
  - `ResolveStackTop` computes `legal_targets` once; fizzle = `legal_targets.is_empty()` for real requirements.
  - `Damage`/`Sacrifice`/`Destroy`/`Exile`/`Counters` arms iterate all `legal_targets`.
  - Partial fizzle falls out for free: illegal targets absent from `legal_targets`.
- Unresolved:
  - Damage division/distribution among targets.

## Slice: targeting-up-to-n (merged)

- Branch `phase5/targeting-up-to-n`.
- Scope: CR 601.2c/115.6 "up to N" target counts.
- Verify: 7 new tests, 659 total, 0 failed.
- Key changes:
  - `TargetRequirement.up_to: bool`.
  - `validate_targets` accepts `targets.len() <= count` when `up_to`.
  - Fizzle gate changed to `!top.targets.is_empty()` (was `req.count > 0`).
  - Scenario `TargetRequirementSpec.up_to: bool`.
- Unresolved:
  - "N or more" counts.

## Slice: targeting-subtype-filter (merged)

- Branch `phase5/targeting-subtype-filter`.
- Scope: CR 205.4b subtype restrictions on targets.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `TargetFilter::Object.subtypes: Option<Vec<Subtype>>`.
  - `target_is_legal` checks subtypes via `effective_characteristics`.
  - `decode_target_filter_table` supports `subtypes` list.
  - Scenario `TargetRequirementSpec.subtypes`.
- Deviations:
  - `TargetFilter`/`TargetRequirement`/`TargetSpec` lost `Copy` (Vec field); 21 test files updated with `subtypes: None`/`.`.
- Unresolved:
  - Supertype restrictions, "another target", negative filters.

## Slice: targeting-supertype-filter (merged)

- Branch `phase5/targeting-supertype-filter`.
- Scope: CR 205.4a supertype restrictions on targets.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `TargetFilter::Object.supertypes: Option<SupertypeSet>` (no `Copy` loss because `SupertypeSet` is bitflag).
  - `target_is_legal` checks supertypes via `effective_characteristics`.
  - `decode_target_filter_table` supports `supertypes` list using existing `parse_supertype_name`.
  - Scenario `TargetRequirementSpec.supertypes` + `parse_supertypes` helper.
- Unresolved:
  - "Another target", negative filters, "N or more" counts, multi-target-word spells.

## Slice: scripted-target-filter (merged)

- Branch `phase5/scripted-target-filter`.
- Scope: CR 601.2c/602.2b — a custom Lua legality predicate for a target requirement, for filter
  shapes `TargetFilter::Object`'s flat option-bag can't express (e.g. "target creature with power
  4 or greater").
- Verify: 6 new tests (`phase5_scripted_target_filter.rs`), full workspace suite green.
- Key finding that reshaped the fix: the old top-level `TargetSpec::Script(ScriptHandle)` variant
  (stubbed in `targeting-basics`) was never constructed by anything across 5+ targeting slices
  since, and had no `count`/`up_to` of its own — a real scripted predicate still needs exactly-1 or
  up-to-N counting like any other requirement. Deleted it outright rather than implementing it as
  designed; the real predicate now lives at `TargetFilter::Script(ScriptHandle)`, nested inside
  `TargetRequirement` alongside `Object`/`Player`/`AnyTarget`, so `count`/`up_to` apply normally.
  Also stale: `invoke_target`'s old doc comment blamed the gap on "blocked on Lua coroutine
  support, not designed yet" — that support was already built (for the unrelated
  `Game.SelectTargets` mid-resolution mechanism) by the time this slice started. The two are
  genuinely different concepts: `Game.SelectTargets` asks the player mid-resolution (needs
  suspend/resume); a target *legality check* is a synchronous per-candidate predicate over targets
  the player already chose, evaluated once at cast/activation time — the exact shape
  `AffectedFilter::Script`/`invoke_continuous_affects` already solved for continuous effects. No
  coroutine needed.
- Key changes:
  - `TargetFilter::Script(ScriptHandle)` new variant; `TargetSpec::Script` removed.
  - `ScriptRuntime::invoke_target_legal(handle, state, object, source, controller) -> Result<bool,
    ScriptError>` — same `function(object, source, controller, state) -> bool` call shape
    `invoke_continuous_affects` uses. Replaces the old `todo!()` `invoke_target`.
  - `commands::target_is_legal`'s new `Script` arm: candidate must be `Target::Object` and must
    exist before the script ever runs, then must still clear CR 702.16e protection
    (`target_has_protection_from`) same as every other arm, then asks the script.
  - `decode_target_filter_value` (script.rs) — table-or-function fallback mirroring
    `decode_affected_value`'s own convention for `:SetAffected`. `ActivatedAbilityBuilder::SetTarget`
    widened from `Table` to `Value` to accept it.
- Unresolved:
  - Candidate restricted to `Target::Object` — a player-target script predicate is unmodeled.
  - No zone default for `Script` filter (unlike `Object`'s battlefield default) — the script sees
    full public state and decides zone itself if it cares. Deliberate, not a gap.
  - Spells (`CardDefinition.target_requirement`) still have no Lua builder path at all — this
    slice only wires the general `TargetFilter` machinery plus `ActivatedAbilityBuilder::SetTarget`
    (pre-existing gap, untouched).
  - "N or more" counts, "another target", negative filters — unchanged, still open.
