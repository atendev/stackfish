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
