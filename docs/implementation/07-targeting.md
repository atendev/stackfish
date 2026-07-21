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

## Slice: another-target (implemented, not yet merged)

- Branch `phase5/another-target`.
- Scope: CR 115.4/601.2c "another target" (e.g. Olivia Voldaren's "deals 1 damage to another target
  creature") — a spell/ability's own source is excluded from an otherwise-matching filter. A
  separate mechanism from CR 601.2c/115.3's same-instance dedup (`validate_targets`'s `seen`
  `HashSet`, already implemented since `targeting-basics`, untouched here) — that rejects choosing
  the *same target twice*; this excludes one specific object (the source) from ever being a legal
  candidate at all, even alone.
- Verify: 3 new tests (`phase5_another_target.rs`), full workspace suite green.
- Key changes:
  - `TargetRequirement.exclude_source: bool` — 42 existing struct literals across 30 files needed
    the field added, mechanical/compiler-guided, not scope creep.
  - `commands::target_is_legal` gained an `exclude_source: bool` parameter, checked once ahead of
    every filter variant (`target == Target::Object(source)` -> illegal). Threaded through every
    real call site: `validate_targets` (cast/activate-time), the CR 608.2b resolution recheck
    (`engine.rs`), and the Aura SBA recheck (`sba.rs`). The Equipment SBA recheck and `Game.
    SelectTargets`'s mid-resolution script choice both pass a hardcoded `false` — the former
    because its filter is a synthetic CR 301.5 "must be a creature" check with no real
    `TargetRequirement` behind it, the latter because `ScriptChoiceRequest::SelectTarget` carries a
    bare `TargetFilter` with nowhere to read the flag from (named TODO, not silently narrowed).
  - Scenario `TargetRequirementSpec.exclude_source: bool` (`#[serde(default)]`), threaded through
    for symmetry with the other `TargetRequirement` fields the TOML format already exposes.
  - Found and fixed alongside this feature: the CR 608.2b resolution recheck was passing
    `top.stack_id` as `target_is_legal`'s `source` parameter instead of `top.source`. Harmless for a
    spell (the two coincide — see `StackObject`'s own construction sites), but wrong for an
    activated/triggered ability, where `stack_id` is a bare id never inserted into `state.objects` -
    `target_has_protection_from`'s `state.object(source)` lookup would silently miss and return
    `false`, meaning protection-from-source-quality was never actually re-checked at resolution time
    for any ability, only spells. Every other resolution site already used `top.source` correctly
    (e.g. the `Damage` effect arm) — only this one recheck call had the mismatch. Fixed at the same
    call site this slice needed to touch anyway, not filed as a separate slice.
- Unresolved:
  - "N or more" counts, negative filters, multi-target-word spells, no Lua builder path for a
    spell's own `target_requirement` — all still open, unchanged by this slice.
  - `Game.SelectTargets` mid-resolution choices can't express "another target" (see above).

## Slice: player-target-script-filter (merged)

- Branch `phase5/player-target-script-filter`.
- Scope: CR 601.2c/602.2b — `TargetFilter::Script`'s player-candidate twin. A custom Lua legality
  predicate for a *player* target requirement, for a shape plain `TargetFilter::Player` can't
  express (e.g. "target player with 10 or less life").
- Verify: 4 new tests (`phase5_player_target_script_filter.rs`), full workspace suite green.
- Key changes:
  - `TargetFilter::PlayerScript(ScriptHandle)` new sibling variant to `Object`/`Player`/
    `AnyTarget`/`Script` — kept separate rather than widening `Script` to accept either target
    kind, since `Script`'s Lua contract already passes a bare object id number as its first call
    argument; folding players in would mean either breaking that contract for every existing
    object-script card or tagging every call with a kind, neither of which any real card built so
    far needs.
  - `ScriptRuntime::invoke_player_target_legal(handle, state, player, source, controller) ->
    Result<bool, ScriptError>` — same call shape as `invoke_target_legal`, `player.0` in place of
    `object.0`.
  - `commands::target_is_legal`'s new `PlayerScript` arm: candidate must be `Target::Player` and
    the player must exist, then asks the script. No protection check — same posture plain
    `TargetFilter::Player` already has (this engine doesn't model player-side protection).
  - `decode_target_filter_value` (script.rs): `{ type = "player", filter = function(player,
    source, controller, state) -> bool }` decodes to `PlayerScript`, checked ahead of the plain
    flat-option-bag decode so a bare `{type="player"}` (no `filter` function) still falls through
    to `TargetFilter::Player` unchanged.
- Unresolved:
  - "N or more" counts, negative filters, multi-target-word spells, no Lua builder path for a
    spell's own `target_requirement` — all still open, unchanged by this slice.

## Slice: target-count-at-least (merged)

- Branch `phase5/target-count-at-least`.
- Scope: CR 601.2c/115.6 "N or more" target counts — `TargetRequirement`'s third count shape
  alongside exact-count and "up to N" (e.g. "target two or more creatures," "any number of target
  creatures" as `count: 0`).
- Verify: 6 new tests (`phase5_targeting_at_least_n.rs`), full workspace suite green.
- Key changes:
  - `TargetRequirement.at_least: bool` — mutually exclusive with `up_to`. `count` becomes the
    minimum with no upper bound of its own when set, same "caller submits the list, engine never
    enumerates all legal candidates itself" posture every other count shape already has.
  - `commands::validate_targets`'s count check gets a third arm: `targets.len() >= req.count`.
  - `TargetRequirementSpec.at_least` (scenario TOML), threaded through `build_target_requirement`.
  - No resolution-time fizzle-logic change needed — already keyed off `!top.targets.is_empty()`,
    not `count`/`up_to`, since `targeting-up-to-n` fixed that; shape-agnostic for free.
- Deviations:
  - ~41 existing `TargetRequirement { ... }` literals across ~30 test files needed the new field
    added, compiler-guided/mechanical (same precedent `exclude_source`'s 42-site and `subtypes`'
    21-site additions already set) — done with a scoped `sed` pass rather than by hand, verified
    clean by `cargo build --workspace --tests`.
- Unresolved:
  - Negative filters, multi-target-word spells, no Lua builder path for a spell's own
    `target_requirement` — all still open, unchanged by this slice.
