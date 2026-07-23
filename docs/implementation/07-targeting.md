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

## Slice: another-target (merged)

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

## Slice: negative-target-filters (merged)

- Branch `phase5/negative-target-filters`.
- Scope: CR 601.2c/115/205.4b negative target filters — `TargetFilter::Object`'s own long-standing
  TODO ("target creature you don't control that isn't a Wall"), left open through every prior
  targeting slice.
- Verify: 7 new tests (`phase5_targeting_negative_filter.rs`), full workspace suite green.
- Key changes:
  - `TargetFilter::Object.excluded_card_types: Option<CardTypeSet>` ("target nonland permanent")
    and `.excluded_subtypes: Option<Vec<Subtype>>` ("target creature that isn't a Wall") — the
    negation of the existing `card_types`/`subtypes` "or" matches, checked independently so a
    filter can combine a positive `card_types` with an `excluded_subtypes` (the worked example:
    `card_types: Creature, excluded_subtypes: [Wall], controller: Opponent`).
  - `commands::target_is_legal`'s `Object` arm gets two new reject checks; the "compute
    `effective_characteristics` at all" guard now also fires when either excluded field is `Some`
    (previously only the three positive fields).
  - `sba.rs`'s synthetic Equipment-attach filter and `decode_target_filter_table` (script.rs, the
    `:SetTarget`/`Game.SelectTargets` table shape) both got the two new fields — `None` for the
    former (no behavior change), real `excluded_card_types`/`excluded_subtypes` table keys for the
    latter, same decode pattern the positive keys already use.
  - `TargetRequirementSpec.excluded_card_types`/`.excluded_subtypes` (scenario TOML), threaded
    through `build_target_requirement`.
  - No `excluded_supertypes` — no real card built so far needs "nonlegendary," named as a
    follow-up rather than guessed at.
- Deviations:
  - Every existing `TargetFilter::Object { ... }` construction (~26 files, ~34 sites; patterns
    using `..` untouched) needed the two new fields added, compiler-guided/mechanical, same
    precedent every prior `Object`-field addition (`subtypes`, `supertypes`) already set — done
    with a scoped `perl` pass (guarded to skip any literal containing `..`), verified clean by
    `cargo build --workspace --tests`.
- Unresolved:
  - Negated supertypes (closed by `phase5/negated-target-supertypes`, below), multi-target-word
    spells, no Lua builder path for a spell's own `target_requirement` — the latter two still open,
    unchanged by this slice.

## Slice: spell-target-script (merged)

- Branch `phase5/spell-target-script`.
- Scope: closes the "no Lua builder path for a spell's own `CardDefinition.target_requirement`"
  note every prior targeting slice above carried forward — `ActivatedAbilityBuilder::SetTarget`
  already accepted a script function, spells had no equivalent at all. Traced first (no engine-crate
  change turned out to be needed): `TargetFilter::Script(ScriptHandle)` already reads generically
  off any `CardDefinition.target_requirement` via `commands::validate_targets`/`target_is_legal`
  regardless of whether the source is a spell or an ability, and `Engine::load_script`/
  `ScriptRuntime::load_function` already existed as exactly the raw-source-to-`ScriptHandle` loader
  needed — the entire gap was scenario-crate-only wiring.
- Verify: 1 new TOML fixture (`tests/scenarios/phase5_spell_target_script.toml`, auto-picked up by
  `scenarios.rs`'s directory scan) exercising a scripted "power 4 or more" spell target requirement
  against both a legal and an illegal candidate, full workspace suite green.
- Key changes:
  - `TargetRequirementSpec.filter = "script"` (scenario TOML) plus a new
    `TargetRequirementSpec.script: Option<String>` field carrying raw Lua source
    (`function(object, source, controller, state) -> bool`, same shape
    `ActivatedAbilityBuilder::SetTarget`'s function form already uses). `build_target_requirement`
    builds a `TargetFilter::Script(ScriptHandle(0))` placeholder at this point — no
    `Engine`/`ScriptRuntime` exists yet when `CardDefinition`s are built, same timing problem
    `trigger_condition`/`trigger_operation` already solved for triggered abilities.
  - `PendingTargetScript { object, script }` — `build_initial_state`'s collected-for-later list,
    `PendingTriggerScript`'s exact sibling. Returned alongside it (`build_initial_state`'s return
    type grew a 4th element); `run()`'s no-`[game]` branch drains it the same way it already drains
    `trigger_scripts`.
  - `register_target_script(engine, object, source)` — loads `source` into the real `Engine` once
    it exists and patches the placeholder handle on that object's own
    `CardDefinition.target_requirement` in place (`GameObject.definition` is owned per-object, not
    interned, so this is a direct mutation, not a lookup-and-clone). Panics if the target
    requirement isn't the placeholder it expects, same "trust the caller wired this up correctly"
    posture `register_trigger_ability` already takes.
  - `build_new_game_engine`'s `[game]`-driven path calls `register_target_script` directly inside
    its existing per-library-card post-`Engine::new_game` loop (the same loop that already calls
    `register_trigger_ability` for `trigger_operation`) — no separate pending-list threading needed
    there, since `Engine` already exists by that point in that code path.
- Unresolved:
  - Modal spells (`SpellMode.target_requirement`) — TOML has no modal-spell schema at all yet
    (pre-existing gap, unrelated to this slice).
  - No caching/dedup of a loaded script across multiple instances of the same scripted-target card —
    each object gets its own freshly-loaded `ScriptHandle`, consistent with `trigger_operation`'s
    existing per-instance-load convention, not a new gap.
  - `build_new_game_engine`'s per-instance loop only wires commander-zone cards' library
    counterparts (`gp.library`), not `gp.commanders` — pre-existing scope boundary
    `trigger_operation`/`mana_ability` already have in that function, not touched by this slice.

## Slice: negated-target-supertypes (merged)

- Branch `phase5/negated-target-supertypes`.
- Scope: CR 601.2c/115/205.4a's negated supertype form ("target nonlegendary creature") —
  `TargetFilter::Object`'s last remaining negation gap, named as a follow-up by
  `negative-target-filters` rather than guessed at (no real card built at the time needed it).
- Verify: 5 new tests (`phase5_targeting_negative_supertype_filter.rs`), full workspace suite green.
- Key changes:
  - `TargetFilter::Object.excluded_supertypes: Option<SupertypeSet>` ("target nonlegendary
    creature") — the negation of the existing `supertypes` intersection match, checked
    independently so a filter can combine a positive `card_types` with an `excluded_supertypes`
    (the worked example: `card_types: Creature, excluded_supertypes: [Legendary], controller:
    Opponent`). Exact same shape `excluded_card_types`/`excluded_subtypes` already established.
  - `commands::target_is_legal`'s `Object` arm gets one new reject check, mirroring the existing
    `excluded_subtypes` check exactly (intersection test against `effective.supertypes`).
  - `sba.rs`'s synthetic Equipment-attach filter and `decode_target_filter_table` (script.rs, the
    `:SetTarget`/`Game.SelectTargets` table shape) both got the new field — `None` for the former
    (no behavior change), a real `excluded_supertypes` table key for the latter, same decode
    pattern the positive `supertypes` key already uses.
  - `TargetRequirementSpec.excluded_supertypes` (scenario TOML), threaded through
    `build_target_requirement`.
- Deviations:
  - Every existing `TargetFilter::Object { ... }` construction (~26 files, ~35 sites) needed the new
    field added, compiler-guided/mechanical, same precedent every prior `Object`-field addition
    already set — done with a scoped `perl` pass keyed off `supertypes: None,` immediately
    preceding `controller:` (safe against `cost.rs`'s unrelated `FilterSpec`, whose own field order
    puts `subtypes` between them), plus one manual fix for the sole site with a real `supertypes:
    Some(...)` value. Verified clean by `cargo build --workspace --tests`.
- Unresolved:
  - Multi-target-word spells (closed by `phase5/multi-target-word-spells`, below), no Lua builder
    path for a spell's own `target_requirement` (closed by `phase5/spell-target-script`, above, for
    the scenario side — TOML modal spells still don't exist) — unchanged by this slice.

## Slice: modal-spell-toml (merged)

- Branch `phase5/modal-spell-toml`.
- Scope: closes the "TOML modal spells still don't exist" note carried forward by every prior
  targeting slice above. Engine-side CR 700.2a mode-selection (`CardDefinition.modes`/`SpellMode`,
  `commands.rs`'s "choose exactly one" validation) was already real and exhaustively tested via
  direct `CardDefinition` struct literals (`phase5_script_modal_spells.rs`) — the entire gap was
  scenario-crate-only: `CardSpec` had no `modes` field, so a modal card was never constructible from
  a TOML fixture.
- Verify: 1 new TOML fixture (`tests/scenarios/phase5_modal_spell_toml.toml`, auto-picked up by
  `scenarios.rs`'s directory scan) casting two modal instants — one choosing the no-target mode,
  one choosing the Creature-target mode — full workspace suite green.
- Key changes:
  - `SpellModeSpec { target_requirement: Option<TargetRequirementSpec> }` — one entry per mode, same
    shape the top-level `CardSpec.target_requirement` already uses. No `effect` field: same
    pre-existing gap `CardSpec` itself has (no TOML representation for a Rust-native `AbilityEffect`
    yet, only via `script`) — a TOML-built mode always resolves with `effect: None`, same as any
    other TOML-built spell today.
  - `CardSpec.modes: Vec<SpellModeSpec>` (`#[serde(default)]`, empty = non-modal, matching
    `CardDefinition.modes`'s own convention).
  - `build_spell_modes(&[SpellModeSpec]) -> Vec<SpellMode>` — maps each entry through the existing
    `build_target_requirement` helper.
  - Wired into all 3 sites that previously hardcoded `modes: Vec::new()`: `build_initial_state`'s
    `[[setup]]` loop, and `build_new_game_engine`'s per-player library and commander loops.
- Unresolved:
  - No TOML field for a mode's own effect (pre-existing `CardSpec.effect` gap, unrelated to this
    slice). CR 700.2d/e/h/i (choose 2+, another player chooses, per-mode additional cost, pawprint
    budgets) — still unmodeled engine-side, unchanged by this slice. Multi-target-word spells — still
    open.

## Slice: multi-target-word-spells (merged)

- Branch `phase5/multi-target-word-spells`.
- Scope: CR 601.2c/608.2b — a spell/ability that uses the word "target" in more than one place (CR
  608.2b's own worked example, Plague Spores: "Destroy target nonblack creature and target land").
  Every prior targeting slice carried this forward as still open — only one `TargetRequirement` per
  spell/ability was representable at all.
- Verify: 6 new tests (`phase5_multi_target_word_spells.rs`), full workspace suite green.
- Key finding that reshaped the plan: no `CardDefinition` field change needed (a past slice found
  adding one prohibitively wide-ripple, ~403 struct-literal sites with no `Default` impl) — the whole
  gap was a missing `TargetSpec` variant, touching only the two real places that already consume
  `TargetSpec` (`commands::validate_targets`, the CR 608.2b resolution recheck in `engine.rs`). Also:
  for the common real shape (one effect verb applied uniformly across every word — the rules' own
  Plague Spores example), zero changes were needed to effect-resolution code at all — `AbilityEffect::
  Destroy`/`Damage`/etc. already just iterate a flat `legal_targets` list, and concatenating each
  word's own legal subset into that list (in order) produces the correct result for free. A
  role-differentiated multi-word spell (different words feeding different verbs) has no matching
  `AbilityEffect` primitive either way and would need a `Script` effect regardless — not a new gap.
- Key changes:
  - `TargetSpec::Multiple(Vec<TargetRequirement>)` — one entry per distinct target word, in order.
    Every entry must use an exact count (`up_to`/`at_least` both `false`) — there's no way to split a
    flat submitted-targets list unambiguously by a variable-length chunk, so that combination is
    rejected rather than guessed at.
  - `commands::split_targets_by_requirement(reqs, targets) -> Option<Vec<&[Target]>>` — splits a flat
    list into one contiguous chunk per requirement; `None` on any exact-count violation, a too-short
    list, or a leftover remainder.
  - `commands::validate_one_target_requirement` — the pre-existing single-requirement validation body
    (count check, CR 115.3/601.2c per-instance dedup, per-target legality), factored out so
    `validate_targets`'s `Primitive` arm (whole list) and new `Multiple` arm (one chunk per
    requirement, each with its own independent `seen` dedup set) share it. Confirms CR 601.2c's own
    text directly: dedup is per word, not across words — the same object can legally be chosen for
    two different target words.
  - `engine.rs`'s CR 608.2b recheck gained a matching `Multiple` arm: concatenates each word's own
    `target_is_legal`-filtered subset into `legal_targets`; fizzle is gated on the *total* union being
    empty (CR 608.2b: "if all its targets, for every instance of the word target, are now illegal").
- Unresolved:
  - `up_to`/`at_least` combined with multiple target words — named, unsupported combination (see
    `TargetSpec::Multiple`'s own doc comment for why).
  - No Lua builder path for constructing `TargetSpec::Multiple` — same "direct struct literal only"
    limit `alternative_costs`/`effect` have for TOML/script-built cards; not attempted this slice.
