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

## Slice: modal-multi-mode (merged)

- Branch `phase5/modal-multi-mode`.
- Scope: CR 700.2d — choosing two or more modes of a modal spell. `commands.rs`'s "choose exactly
  one" validation (`modal-spell-toml`'s own note above) is now "choose however many
  `CardDefinition.mode_choice` names."
- Key finding that reshaped the plan: unlike `multi-target-word-spells` (which found a zero-
  `CardDefinition`-ripple path via a new `TargetSpec` variant alone), this genuinely needed new
  per-card data with nowhere smaller to live — a card has to declare its own mode-count requirement
  somewhere. Confirmed with the user before proceeding: added `CardDefinition.mode_choice:
  ModeChoice` despite this being exactly the "~403 struct-literal sites, no `Default` impl"
  ripple class `multi-target-word-spells`'s own notes flagged as prohibitive and avoided — no
  smaller-footprint design existed this time. Mitigated by fixing `testutil::minimal_card_definition`
  first (the shared struct-update-syntax helper most tests already build on), which absorbed the
  overwhelming majority of call sites for free; the remaining ~40 full-literal sites were fixed via
  a compiler-error-driven loop (add the field, rebuild, patch every reported site, repeat until
  clean) rather than a blind text search, since a same-text false positive
  (`PlayerActionKind::CastSpell`'s own unrelated `modes: Vec<u32>` field) made blind regex genuinely
  unsafe here.
- Verify: 10 new tests (`phase5_modal_multi_mode.rs`), full workspace suite green
  (`cargo test --workspace`, 141 test binaries, 0 failed).
- Key changes:
  - `ModeChoice { Exactly(u32), UpTo(u32) }` (`types/card.rs`) — `CardDefinition.mode_choice`,
    ignored (but still required, no `Default` impl) when `modes` is empty.
  - `commands::execute_command`'s `CastSpell` arm: validates `modes.len()` against `mode_choice`'s
    min/max, rejects a repeated mode index (CR 700.2d's default case; the reminder-text-gated "may
    choose the same mode more than once" exception stays unmodeled), sorts chosen indices into
    card-print order (matching Wizards' own ruling that a modal spell's chosen effects happen in the
    order printed on the card, not the order chosen). A single chosen mode is unchanged from before
    this slice (bare `target_requirement`/`effect`, no wrapping). 2+ chosen modes combine into one
    `TargetSpec::Multiple` (reusing `multi-target-word-spells`'s own machinery — one
    `TargetRequirement` per chosen mode, a synthetic `count: 0` placeholder for a mode with no
    `target_requirement` of its own, CR 700.2c) and one new `AbilityEffect::Modal(Vec<Option<
    AbilityEffect>>)` (one slot per chosen mode, same card-print order, `None` for a mode with no
    effect of its own).
  - `commands::is_modal_resolvable` — cast-time gate rejecting a mode combination if any chosen
    mode's effect isn't in the synchronous, never-pausing subset `ResolveStackTop`'s new `Modal` arm
    actually knows how to resolve (`GainLife`/`Damage`/`Counters`/`Pump`/`GainControl`/`Shuffle`).
    Every excluded kind (`Destroy`/`Sacrifice`/`Exile`/`DrawCards`/`Search`/`BecomeCopy`/
    `CreateToken`/`Attach`/`ExchangeTextBoxes`) either pauses mid-resolution or has its own bespoke
    per-target-pairing shape that a generic per-mode loop can't drive — combining 2+ such modes would
    need real coordination between multiple simultaneous pauses, out of scope for this slice.
  - `ResolveStackTop`'s new `Some(AbilityEffect::Modal(effects))` match arm: re-derives per-mode
    target chunks from `top.target_requirement` (always `TargetSpec::Multiple` by construction when
    `Modal` is used) via the existing `commands::split_targets_by_requirement`, since the pre-existing
    `legal_targets` local is deliberately flattened (correct for a uniform-verb multi-word spell,
    wrong once each chunk belongs to a *different* mode's own effect). The per-mode legality
    filtering (`modal_chunk_legals`) is computed once, right next to `legal_targets` itself near the
    top of `ResolveStackTop` — not inside the `Modal` arm — because the `is_legal_now` closure
    borrows `&self.state`, which can't coexist with the `Modal` arm's own `self.state` mutations
    (`player_mut`/`allocate_sequence_no`/`continuous_effects.push`) if the closure's last use were
    inside that same arm; computing it up front keeps its borrow's lifetime exactly where it already
    was before this slice.
  - Scenario TOML: `CardSpec.mode_count`/`mode_up_to` (`#[serde(default = "default_mode_count")]`
    defaulting to `1`/`false, so every pre-existing modal-spell fixture parses unchanged),
    `build_mode_choice(&CardSpec) -> ModeChoice`, wired into all 3 `CardDefinition`-construction
    sites `modal-spell-toml` already touched.
- Unresolved: combining a mode using a paused/bespoke effect kind (see `is_modal_resolvable` above).
  A mode whose own `target_requirement` is itself `TargetSpec::Multiple` (a mode independently
  needing several differently-filtered targets) can't be combined with other chosen modes either -
  rejected outright rather than silently flattened. CR 700.2d's "same mode twice" exception, CR
  700.2e (another player chooses), and 700.2i (pawprint budgets) stay unmodeled. CR 700.2h (a mode's
  own additional cost) is real now too - see `modal-mode-additional-cost` below.

## Slice: modal-mode-additional-cost (merged)

- Branch `phase5/modal-mode-additional-cost`.
- Scope: CR 700.2h — a modal spell mode with its own additional cost, paid alongside the mana cost
  only if that mode is chosen.
- Key finding that shrank the plan: `commands.rs`'s own comment claiming "no additional-cost support
  yet either way" for `CastSpell` was stale. `PaymentPlan::validate`/`Engine::submit_payment`
  (`types/cost.rs`/`engine.rs`) are already fully generic over `CostSpec.additional: Vec<AdditionalCost>`
  — built for activated abilities, but never special-cased by caller. The real gap was narrower than
  the comment suggested: `CastSpell` simply never *populated* `cost.additional` with anything. No
  payment-engine changes were needed at all, only new population of an already-generic field.
- Verify: 5 new tests (`phase5_modal_mode_additional_cost.rs` — a mode with no cost pays with an
  empty plan, a mode with a cost requires paying it, an empty plan against a costed mode is rejected,
  2+ chosen modes still only require the one costed mode's own payment, insufficient life to pay is
  rejected, non-modal cards unaffected), full workspace suite green (143 test binaries, 0 failed).
- Key changes:
  - `SpellMode.additional_cost: Option<AdditionalCost>` — small ripple (6 struct-literal sites across
    `types/card.rs`, two test files, and `scenario/src/lib.rs`'s own `build_spell_modes`, unlike
    `CardDefinition`'s own much larger field-addition ripple `modal-multi-mode` already took on).
  - `commands::execute_command`'s `CastSpell` arm collects every chosen mode's own `additional_cost`
    (`mode_additional_costs: Vec<AdditionalCost>`, `filter_map`'d regardless of whether one or
    several modes were chosen) alongside the existing `target_requirement`/`modal_effect`
    computation, then appends it to `cost.additional` once `cost` itself is finalized — for both the
    normal-mana-cost and alternative-cost branches, since CR 700.2h doesn't exclude either.
- Unresolved: no scenario TOML field for a mode's own additional cost — same pre-existing gap
  `CardSpec` itself has for a card's own top-level additional costs (no TOML representation at all
  yet, only via direct struct literals in a Rust integration test). CR 700.2d's "same mode twice"
  exception, CR 700.2e (another player chooses), and 700.2i (pawprint budgets) all stay unmodeled.

## Slice: targeting-multi-word-variable-count (merged)

- Branch `phase5/targeting-multi-word-variable-count`.
- Scope: CR 601.2c/608.2b — `up_to`/`at_least` combined with multiple target words, the combination
  `multi-target-word-spells` itself named as unsupported ("no way to split a flat list unambiguously
  by a variable-length entry").
- Key finding that shaped the plan: the ambiguity only holds when *two or more* words are variable at
  once (a shared remainder between them has no unique split). With at most one variable word, the
  split is unambiguous regardless of where that word sits in card order: subtract every exact word's
  own fixed count from the total submitted, and whatever's left is the one variable word's count.
- Verify: 7 new tests (`phase5_multi_target_word_variable_count.rs` — variable word first, variable
  word last, `up_to` exceeding its own bound rejected, `at_least` below its own bound rejected, `up_to`
  legally claiming zero, two variable words still rejected as ambiguous), full workspace suite green
  (142 test binaries, 0 failed). No struct changes.
- Key changes:
  - `commands::split_targets_by_requirement` gained a two-pass computation: reject outright if 2+
    requirements are variable (unchanged ambiguity, still named); otherwise derive the one variable
    requirement's count as `submitted.len() - sum(exact counts)`, validate it against that
    requirement's own `up_to`/`at_least` bound, then chunk in original word order using each word's
    own fixed count or the derived remainder for the one variable word. Transparently reaches every
    existing caller unchanged (`validate_targets`'s `Multiple` arm, the CR 608.2b resolution recheck
    in `engine.rs`, and `modal-multi-mode`'s own per-mode chunking) — none needed edits.
- Unresolved: two or more variable words in one spell/ability stays a real, unmodeled ambiguity - no
  smaller-footprint resolution exists without a `PlayerAction` shape that announces each variable
  word's own chosen count separately, a larger change than this slice's own scope.

## Slice: script-target-requirement (merged)

- Branch `phase5/script-target-requirement`.
- Scope: CR 702.5a/303.4a - let a card's own script declare its base `target_requirement` (an
  Aura's "Enchant [quality]" line, or any other targeted spell's targeting), instead of requiring
  it as separate, hand-authored data outside the script (TOML today; the external card database
  eventually). Prompted by the user correcting an earlier wrong claim of mine that `target_requirement`
  is naturally "database metadata" - CR 702.5a says Enchant *is* a static ability, same status as
  any other printed ability, so it belongs in the same place other abilities are declared.
- Key finding that shaped the design: `register_abilities(c)` can't be the place this lives.
  `run_card_script` only ever runs once a real `ObjectId` exists for a battlefield/token/copy entry
  - but an Aura's target is chosen while it's still in hand, before it's ever a battlefield object,
  so `register_abilities` would simply run too late. Also, `CardDefinition` is explicitly documented as
  "immutable, shared... sourced from the external card database project - this engine consumes it,
  does not author it," which the original "self-register it like an ability" framing would have
  violated.
- Design: an optional, argument-less `s.target_requirement()` function in the same script module as
  `s.register_abilities(c)` - a sibling, not a replacement. Genuinely static (no `GameState`/`ObjectId`
  needed, since an Enchant restriction doesn't vary by game state or which object ends up carrying
  it), so it's called once per `CardDefinition` build via a new `ScriptRuntime::build_target_requirement`,
  decoupled entirely from `run_card_script`'s per-object timing. Returns the same flat filter table
  `Game.SelectTargets`/`ActivatedAbilityBuilder::SetTarget` already accept
  (`decode_target_filter_table`, reused unchanged), plus `count`/`up_to`/`at_least`/`exclude_source`
  as sibling keys (`decode_target_requirement_table`, new) - identical shape to the scenario crate's
  own TOML `[cards.target_requirement]` table, just Lua-authored. A card with no
  `target_requirement()` function returns `None`, same "requires no targets" default as before.
- Verify: 5 new engine-level tests (`phase5_script_target_requirement.rs` - decode with/without the
  function, missing-`count` load error, casting a script-declared "Enchant Wall"-style Aura at a
  legal Wall creature attaches it, at an illegal creature is rejected) plus a new scenario TOML
  fixture proving the scenario-crate wiring specifically (`card_types`-only filter, not
  subtype-gated - see below), full workspace suite green, 0 failed.
- Key changes:
  - `ScriptRuntime::build_target_requirement(source: &str) -> Result<Option<TargetSpec>, ScriptError>`
    (script.rs) and a thin `Engine::build_target_requirement` delegate (engine.rs).
  - Scenario crate: new `PendingScriptTargetRequirement { object, source }`, collected by
    `build_initial_state` only when `CardSpec.target_requirement` is unset *and* `CardSpec.script`
    is set (an explicit TOML table always wins - zero behavior change for every pre-existing
    fixture), same "no `Engine`/`ScriptRuntime` exists yet at that point" timing reason
    `PendingTargetScript` already has. Patched in by a new `register_script_target_requirement`
    once an `Engine` exists - overwrites the whole `target_requirement` field (not just a `.filter`
    the way `register_target_script` patches a script-predicate placeholder), since there's no
    TOML-declared shape to patch here at all. `build_new_game_engine`'s library/commander loops
    call it inline instead (an `Engine` already exists there by the time cards are built).
- Unresolved / named TODOs: only the flat filter-table shape was supported from
  `target_requirement()` at the time (no real card needed the scripted-predicate form this slice;
  `docs/lua/not-available-yet.md` named it) - closed by `phase5/script-target-requirement-predicate`,
  below. `CardSpec` still has no TOML field for a card's own subtypes at all (pre-existing, separately
  logged in `99-backlog.md`'s Enchant/Equip note) - this slice's own scenario-TOML fixture proves the
  wiring with a `card_types`-only filter rather than a real subtype-gated one for that reason, not
  because subtype filters themselves don't work (they do - proven directly in the Rust-level test via
  a hand-built `CardDefinition`, same as `phase5_aura_attach.rs`'s existing pattern).

## Slice: script-target-requirement-predicate (merged)

- Branch `phase5/script-target-requirement-predicate`.
- Scope: closes `script-target-requirement`'s own named TODO above - `s.target_requirement()` can now
  express a scripted legality predicate (e.g. "Enchant creature with power 4 or greater"), not just
  the flat option-bag table.
- Key finding: no engine-side change was needed at all - `TargetFilter::Script`/`PlayerScript` and
  their evaluation (`commands::target_is_legal`, `ScriptRuntime::invoke_target_legal`/
  `invoke_player_target_legal`) already read generically off any `CardDefinition.target_requirement`,
  regardless of where it came from. The only real gap was `target_requirement()`'s own decode entry
  point (`decode_target_requirement_table`), which unconditionally called the table-only
  `decode_target_filter_table` and never looked for a function.
- Verify: 6 new tests (`phase5_script_target_requirement.rs`: 3 decode-level - a `filter` key holding
  a function decodes to `TargetFilter::Script`, a `{type="player", filter=fn}` sub-table decodes to
  `PlayerScript`, a table with no `filter` key still decodes the original flat shape unchanged - plus
  2 end-to-end casting tests reusing `phase5_scripted_target_filter.rs`'s own "power 4 or greater"
  worked example, and 1 doc-comment/import adjustment test unaffected), full workspace suite green.
- Key changes:
  - `decode_target_requirement_table` (script/decode.rs) checks the returned table for an explicit
    `filter` key first: if present, hands its value to `decode_target_filter_value` (the same
    table-or-function-or-`{type="player",filter=fn}` decoder `ActivatedAbilityBuilder::SetTarget`
    already uses); if absent, falls back to the original `decode_target_filter_table(table)` call on
    the same table unchanged - every pre-existing script/TOML fixture that never sets `filter` keeps
    working exactly as before.
  - Signature change: `decode_target_requirement_table` now takes `&Lua`/`&mut ScriptRegistry` (needed
    to register a scripted predicate's function), sourced in `ScriptRuntime::build_target_requirement`
    from its own private `self.lua`/`self.registry` fields - `self.registry.borrow_mut()` through the
    existing `Rc<RefCell<ScriptRegistry>>` interior mutability every other builder method already
    relies on, so `build_target_requirement`'s own public signature (still `&self`) is unchanged.
  - `Engine::build_target_requirement`'s doc comment (which claimed "this doesn't touch the script
    registry at all") was stale as of this slice and corrected, along with a note that any
    `ScriptHandle` it returns is only valid against that same `Engine`'s own `script_runtime` - the
    same cross-runtime caller trap `docs/implementation/05-script-bridge.md` already names for a
    custom-function handle in general. Caught the hard way: the first draft of this slice's own
    end-to-end test built the scripted `target_requirement` with a throwaway `ScriptRuntime::new()`
    and fed the result into a separately-constructed `Engine::new(state)` (safe for the pre-existing
    flat-table tests, which carry no `ScriptHandle` at all) - every cast was silently rejected
    (`invoke_target_legal`'s `.unwrap_or(false)` swallowed the resulting "unknown script handle"
    error). Fixed by building the requirement from the real `Engine`'s own runtime
    (`Engine::build_target_requirement`) and patching it onto the already-inserted card object, the
    same placeholder-then-patch timing the scenario crate's own `register_target_script` established.
- Unresolved: none new for this shape.
