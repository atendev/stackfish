# 11-ops

## Slice: script-ops-damage (merged)

- Branch `phase5/script-ops-damage`.
- Scope: `AbilityEffect::Damage(u32)` / `Game.Ops.Damage(n)`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Damage(u32)`.
  - `CardDefinition.effect: Option<AbilityEffect>`; `CastSpell` bakes it onto `StackObject`.
  - `ResolveStackTop` effect match widened to instant/sorcery spells; `Damage` arm pushes `DamageDealt` event into pipeline.
  - Reuses `apply_damage_event` (prevention, Lifelink/Deathtouch/etc.) for free.
- Unresolved:
  - `GainLife` still bypasses pipeline (pre-existing).

## Slice: script-ops-destroy (merged)

- Branch `phase5/script-ops-destroy`.
- Scope: `AbilityEffect::Destroy` / `Game.Ops.Destroy()`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Destroy`.
  - `commands::effective_keywords` made `pub(crate)`.
  - `ResolveStackTop` arm checks Indestructible, proposes `ZoneChange`, runs `apply_replacements`, calls `commit_death`.
- Unresolved:
  - CR 616.1 ordering choice for scripted Destroy (fixed later by destroy-ordering-choice).

## Slice: script-ops-counters (merged)

- Branch `phase5/script-ops-counters`.
- Scope: `AbilityEffect::Counters { kind, amount }` / `Game.Ops.Counters(kind, n)`.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Counters { kind: CounterKind, amount: i32 }`.
  - `ResolveStackTop` pushes `CounterAdded` event; `CommitEvents` applies it.
  - `parse_counter_kind_name` maps known names, custom otherwise.
- Unresolved:
  - Player-target counters.
  - No replacement for counter placement.
  - +1/+1 vs -1/-1 annihilation SBA missing.

## Slice: script-ops-drawcards (merged)

- Branch `phase5/script-ops-drawcards`.
- Scope: `AbilityEffect::DrawCards(u32)` / `Game.Ops.DrawCards(n)`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::DrawCards(u32)`.
  - `draw_one_card(player)` extracted from `draw_step`; called in loop by `ResolveStackTop`.
- Unresolved:
  - Target player draws deferred.
  - `Game.Ops.DrawCards(n)` itself still direct-mutation (the harder remaining draw gap, not the turn-based draw fixed later).

## Slice: script-ops-exile (merged)

- Branch `phase5/script-ops-exile`.
- Scope: `AbilityEffect::Exile` / `Game.Ops.Exile()`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Exile`.
  - `ResolveStackTop` arm inlines Battlefield→Exile move with `zone_mut(Exile, None)` (shared zone).
  - Bypasses indestructibility.

## Slice: script-ops-sacrifice (merged)

- Branch `phase5/script-ops-sacrifice`.
- Scope: `AbilityEffect::Sacrifice` / `Game.Ops.Sacrifice()`.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Sacrifice`.
  - `ResolveStackTop` arm identical to `Destroy` minus indestructibility check.
  - Reuses `commit_death`.

## Slice: script-ops-create-token (merged)

- Branch `phase5/script-ops-create-token`.
- Scope: `AbilityEffect::CreateToken` / `Game.Tokens.*` / `Game.Ops.CreateToken(spec, count?)`.
- Verify: 13 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::CreateToken { spec: TokenSpec, count }`.
  - `NamedToken::{Clue, Food}`, `CreatureTokenSpec`, `TokenSpec`.
  - `GameObject.is_token: bool` field; minted tokens set true, all other construction sites set false.
  - `build_token_definition`, `register_token_abilities` helpers in `engine.rs`.
  - Clue/Food abilities registered directly in `state.abilities`.
- Deviations:
  - 45+ `GameObject` struct literal sites needed `is_token: false`.
- Unresolved:
  - CR 111.7 token cessation (later fixed by token-cessation).
  - Treasure token blocked on coroutines (later fixed by `phase5/treasure-token` — the
    "blocked on coroutines" premise was stale by the time it was checked, `AddManaChoice`
    already resolves synchronously off `chosen_colors`).
  - FilterSpec identity gap on Clue/Food sacrifice costs (later fixed by
    `phase5/sacrifice-self-cost` — `AdditionalCost::SacrificeSelf`).

## Slice: script-ops-shuffle (merged)

- Branch `phase5/script-ops-shuffle`.
- Scope: `AbilityEffect::Shuffle` / `Game.Ops.Shuffle()`.
- Verify: 4 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Shuffle`.
  - Reused existing private `Engine::shuffle_library`; exposed via `ResolveStackTop`.
- Unresolved:
  - Target player shuffles deferred.

## Slice: scry-surveil (merged)

- Branch `phase5/scry-surveil`.
- Scope: CR 701.22/701.25 Scry/Surveil.
- Verify: 11 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::{Scry(u32), Surveil(u32)}`.
  - `ProcessState::AwaitingScryChoice`/`AwaitingSurveilChoice`; `StepStatus` variants; `Engine::submit_scry_choice`/`submit_surveil_choice`.
  - `is_exact_partition` validation.
  - `GameEventKind::{CardsScried, CardsSurveiled}`.
- Unresolved:
  - "Whenever you scry/surveil" trigger conditions (later fixed by trigger-completeness-cleanup).

## Slice: search-tutor (merged)

- Branch `phase5/search-tutor`.
- Scope: CR 701.23 Search.
- Verify: 2 unit + 10 integration tests, 0 failed.
- Key changes:
  - `FilterSpec` gained `supertypes`/`subtypes` fields (shared benefit for costs too).
  - `AbilityEffect::Search { filter, count, destination, shuffle_after }`.
  - `SearchDestination::{Hand, Battlefield { tapped }}`.
  - `ProcessState::AwaitingSearchChoice`; `Engine::submit_search_choice`.
- Unresolved:
  - Mandatory quantity with no quality filter; reveal; top-N redirection; multiplayer APNAP; graveyard/exile destinations.

## Slice: mass-destroy-effect (merged)

- Branch `phase5/mass-destroy-effect`.
- Scope: CR 701.8/601.2c mass destruction - "destroy all X," which doesn't target at all (unlike
  `AbilityEffect::Destroy`, which only ever destroys the resolving spell/ability's own already-
  chosen target). Closes the gap `phase5/register-abilities-and-script-operation`'s own Armageddon
  example named as still-missing (`s.operation()` gave a plain spell a script-declared effect at
  all, but no primitive existed yet for "destroy every matching permanent").
- Key finding that shaped the design: the actual destruction logic (CR 702.12b indestructibility,
  CR 616.1 ordering-choice pauses, the CR 614.13 `WouldDie` replacement check) already lives
  entirely in a shared pipeline - `Destroy`'s own `ResolveStackTop` arm doesn't implement any of it
  inline, it just collects target ids and calls `self.continue_scripted_zone_move(
  ScriptedZoneMoveKind::Destroy, ids, ...)`. The new `DestroyAll` arm only needed to collect ids
  differently (every matching battlefield object instead of chosen targets) and hand the same list
  to the same function - no new destruction logic anywhere. The one genuinely new piece was filter
  matching, and that already existed too, just embedded inside `ContinuousEffect::applies_to` -
  extracted into a free `matches_effect_filter(filter, object, source_id, source_controller)`
  function so both call sites share it instead of duplicating the controller/card_types checks.
- Verify: 3 new engine-level tests (`phase5_mass_destroy_effect.rs` - decode, a mixed battlefield
  proving indestructible/non-land are spared, a `controller = "you"` variant proving only the
  caster's own permanents die) plus a new scenario TOML fixture running a complete, real
  Armageddon script end to end, full workspace suite green.
- Key changes:
  - `types/effect.rs`: `matches_effect_filter` extracted from `ContinuousEffect::applies_to`
    (`pub(crate)`, zero behavior change for the existing caller).
  - `types/ability.rs`: `AbilityEffect::DestroyAll(EffectFilter)` - reuses the exact
    `controller`/`card_types`/`self_only` shape `Game.CreateStaticAbility`'s own `:SetAffected`
    already uses, rather than inventing a second filter vocabulary.
  - `engine.rs`: new `ResolveStackTop` arm - collects every `ZoneKind::Battlefield` object matching
    the filter (relative to `top.source`/`top.controller`) into a `VecDeque<ObjectId>`, then calls
    the same `continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, ...)` the single-target
    `Destroy` arm already uses.
  - `commands.rs`: `DestroyAll` added to `ActivateAbility`'s real usable-effects list (not an
    exhaustiveness-only arm - nothing stops an activated ability from saying "destroy all Walls"
    either, even though no real card built this engine needs that yet).
  - `script.rs`: `decode_effect_filter_table` extracted from `decode_affected_value` (same reuse
    shape as `matches_effect_filter`) and reused by the new `Game.Ops.DestroyAll(filter)` factory.
- Unresolved: `EffectFilter` has no `subtypes`/exclusion keys - "destroy all Zombies"/"destroy all
  nonland permanents" aren't expressible yet, only a plain type/controller restriction. Not added
  to `is_modal_resolvable`'s whitelist - no real modal card needs "destroy all X" as one mode yet
  (already silently no-ops there via the existing wildcard, not a new gap).

## Slice: token-enters-attached (merged)

- Branch `phase5/token-enters-attached`.
- Scope: CR 303.4f/g/301.5e/301.6 pre-attached battlefield entry, extended to token creation
  (`AbilityEffect::CreateToken`) - the one real non-casting, non-Search battlefield-entry producer
  that had no attach machinery at all (`resolve_create_token_effect` always hard-coded
  `attached_to`/`attached_to_player: None`, never paused). Closes a gap named twice already, in
  `phase5/aura-enchants-a-player` and `phase5/aura-enters-attached-player`'s own backlog notes.
  Only `TokenSpec::CopyOf` can realistically produce an Aura/Equipment/Fortification token (`Named`/
  `Creature` specs always include `CREATURE`) - real cards: Estrid, the Masked / Bruna, the Fading
  Light.
- Design call made without a new opt-in field (unlike `SearchDestination::Battlefield::
  enters_attached`): the attach-candidate dispatch always runs, gated purely by `commands::
  is_aura`/`is_equipment`/`is_fortification` on the token's own resolved `CardDefinition`. CR 303.4f
  has no "effect must specify" carve-out in its own text, and this keeps every pre-existing
  `CreateToken` call site (none of which ever produces an Aura/Equipment-shaped token) completely
  untouched - zero ripple, confirmed via a full-repo grep before implementing.
- Real finding: `commands::legal_entry_attach_candidates`/`legal_aura_entry_candidates`/
  `attach_permanent`/`attach_permanent_to_player`/`target_is_legal` needed **no changes** - already
  fully `Target`-generic (post `phase5/aura-enters-attached-player`) and directly callable. Token
  creation is actually simpler than Search here: the token's real final `ObjectId` is already
  allocated before `state.objects` insertion (`tokens.rs`), so the candidate scan can use it
  directly as `source` with no placeholder-id indirection (unlike Search, which scans with the
  pre-move library id since its permanent's real id doesn't exist yet at scan time).
- Key changes:
  - `ProcessState`/`StepStatus::AwaitingTokenAttachChoice` - a new sibling to `AwaitingEntryAttachChoice`,
    not a shared variant: that pause's own resume always re-enters Search's `VecDeque<ObjectId>`-based
    library loop, which token creation's `count`-driven loop has no equivalent of. Carries `spec`/
    `copy_snapshot`/`remaining_count` instead of `remaining`/`destination`/`shuffle_after`.
  - `resolve_create_token_effect`'s old `for _ in 0..count` loop became `continue_create_token`
    (`pub(in crate::engine)`, `engine/resolve/tokens.rs`), taking `remaining: u32` instead - the same
    "loop body factored out of the fresh-entry effect resolver, called by both the fresh start and
    the resume path" shape `continue_search_battlefield_entry` already established. Per-token order:
    resolve `token_def`, allocate `token_id`, run the attach-candidate scan *before* building/
    inserting the `GameObject` at all (the one real ordering divergence from Search) - this is what
    makes CR 303.4g's token-specific second sentence ("if the Aura is a token, it isn't created")
    cheap to honor exactly: a 0-candidate Aura just `continue`s before anything is built or inserted,
    rather than needing to build-then-undo.
  - `Engine::submit_token_attach_choice(chosen: Target)` (`choice_answers.rs`) - same object/player
    two-armed `attach_permanent`/`attach_permanent_to_player` dispatch `submit_entry_attach_choice`
    already established, but its tail differs: since this pause is reached mid-`ResolveStackTop` (not
    via a separate library pre-phase), *this* method must push `StackObjectResolved`/transition to
    `ReplacementCheck` itself when the resumed loop finishes without a further pause - the same
    "bypassing the normal `ControlFlow::Continue` caller" tail shape `submit_amplify_choice` already
    established, for the identical reason.
  - `crates/scenario/src/lib.rs`'s `StepOutcome` dispatch gained the new `StepStatus` variant to its
    existing generic "any unhandled pause returns `Ran`" arm (mechanical, one line).
- Verify: 7 new tests (`phase5_token_enters_attached.rs`): 1-candidate auto-attach, 2-candidate
  pause/resume, wrong-candidate-shape rejection, 0-candidate Aura not-created, 0-candidate Equipment
  still-created-unattached, a `count: 2` case where both tokens independently pause and attach to
  different targets (validates the resumable-loop design, the one genuinely new mechanism this slice
  built), and a player-target case. Full workspace suite green.
- A real modeling discovery made while writing the 0-candidate Aura test, not an implementation bug:
  a *live, on-battlefield* Aura currently offering zero legal targets is a structural impossibility in
  this engine (and in real Magic) - if it currently has zero legal targets, its own existing
  attachment must already be illegal per that identical filter, and CR 704.5m's state-based action
  would already have destroyed it before any copy effect could ever see it. The test's `CopyOf`
  source therefore sits in the graveyard, not the battlefield (`sba::collect_illegally_attached_
  auras`/`legal_entry_attach_candidates` both only ever consider `ZoneKind::Battlefield` objects, so
  a graveyard Aura is exempt from CR 704.5m entirely) - not a workaround, a more accurate fixture
  (mirrors Bruna, the Fading Light's own reanimation-adjacent shape).
- Unresolved: mixed object-and-player candidates in one choice (no `TargetFilter` variant expresses
  "object or player," so no producer - Search's or this one - can create a mixed set); CR 303.4i
  (explicit illegal target) - still genuinely unreached, no `CreateToken`/`Search` shape in this
  engine can specify a target at all; CR 303.4m enchanted-player continuous effects - unrelated,
  unchanged.

## Slice: effect-filter-subtypes (merged)

- Branch `phase5/effect-filter-subtypes`.
- Scope: closes the `mass-destroy-effect` slice's own named gap - `EffectFilter` had no
  `subtypes`/`excluded_subtypes` keys, so "destroy all Zombies" (CR 701.8/205.4b) wasn't
  expressible, only a plain card-type/controller restriction. `EffectFilter` gains
  `subtypes: Option<Vec<Subtype>>`/`excluded_subtypes: Option<Vec<Subtype>>`, same positive/negated
  "or, not and" pair `TargetFilter::Object` already has for its own `subtypes`/`excluded_subtypes`.
  Since `EffectFilter` is shared by both `AbilityEffect::DestroyAll` and `ContinuousEffect`'s own
  `AffectedFilter::Primitive`, this closes the gap for both at once (`Game.Ops.DestroyAll({ subtypes
  = {"Zombie"} })` and a static ability's `:SetAffected({ subtypes = {"Wall"} })`).
- Design call, matching existing precedent: the new fields are checked against the raw
  `GameObject.definition.subtypes`, not `layers::effective_characteristics.subtypes` - same
  deliberate "raw field, not layer-resolved" stance `matches_effect_filter`'s pre-existing
  `card_types`/`controller` checks already take (see backlog's "Layer 2 internal filtering" note):
  `ContinuousEffect::applies_to` is itself invoked from inside `layers::
  applicable_effects_in_layer`, the function that computes `effective_characteristics` in the first
  place, so reading the effective view here would recurse.
- Real finding, not anticipated in the requirements report: `EffectFilter`/`EventFilter` both
  derived `Copy`, which `Vec<Subtype>` can't support - both derives had to drop to `Clone`-only,
  which surfaced 3 real move-out-of-`&mut self` sites in `ReplacementEffectBuilder::Register`
  (`builders.rs`) needing an explicit `.clone()` each; no behavior change, just ownership.
- Second real finding, caught only by the new subtype tests actually failing (not by the compiler):
  `types/effect.rs`'s `event_target_matches` - the function that checks an `EffectFilter` against
  an *event's* target (`WouldMoveZone`/`DamageDealt` replacement matching) - is a separate,
  layers-aware near-duplicate of `matches_effect_filter` (it resolves `controller` via
  `layers::effective_controller` rather than a raw field), not a caller of it. It had its own
  `card_types`-only check with no subtype equivalent, so a subtype-filtered replacement effect (e.g.
  "opponent's Swamps enter tapped") silently matched *every* object regardless of subtype until this
  slice added the same `subtypes`/`excluded_subtypes` check here too - a second near-identical block,
  not shared, since the two functions differ in how they resolve `controller` and in their
  `Target::Player`/`Target::Object` split. Also tightened `event_target_matches`'s `Target::Player`
  arm to reject a `subtypes`-bearing filter the same way it already rejected `card_types` (a player
  has neither); `excluded_subtypes` deliberately does *not* join that reject list - a player
  trivially has none of the listed subtypes, so a negated filter is vacuously satisfied rather than
  a non-match, the same asymmetry the pre-existing `card_types`-only (no `excluded_card_types` on
  this type) reject list already implies.
- Refactor folded in: `ReplacementEffectBuilder::SetAffected` (`builders.rs`) had its own ~25-line
  inline re-decode of `controller`/`card_types`/`self_only` predating `decode_effect_filter_table`'s
  extraction in the `mass-destroy-effect` slice - replaced with a direct call to
  `decode_effect_filter_table`, the smallest-footprint way to give this builder `subtypes`/
  `excluded_subtypes` support too without a third hand-written decode of the same shape.
- Ripple: `EffectFilter` has no `Default` impl - confirmed with the user before starting (same
  ripple class `modal-multi-mode`/`bands-with-other-effect-blocked` already flagged and paid) - 131
  struct-literal construction sites across 59 files needed the two new fields added, entirely
  mechanical/compiler-driven (`cargo build --workspace --tests`'s `E0063` errors located every site).
  `crates/scenario/src/lib.rs`'s `build_continuous_effect` (TOML `continuous_effect` fixture support)
  is the one site that just defaults both new fields to `None` - `ContinuousEffectSpec` has no
  `filter_subtypes`/`filter_excluded_subtypes` TOML field yet, named as a TODO there rather than
  built, since no scenario fixture needs it and the requirements report scoped this slice to the
  engine field/Lua decode/`Game.Ops.DestroyAll`, not the TOML schema.
- Verify: unit test on `matches_effect_filter` itself (subtypes/excluded_subtypes, `types/effect.rs`);
  2 new tests in `phase5_mass_destroy_effect.rs` (decode, end-to-end "destroy all Zombies" sparing a
  non-Zombie creature); 2 new tests in `phase5_etb_replacement_modifiers.rs` (a subtype-filtered
  externally-granted `EntersTapped` affecting a matching Swamp and sparing a non-matching Forest -
  this pair is what caught the `event_target_matches` gap above). Full workspace suite green (160
  test-result blocks, zero failures) after both real findings were fixed.
- Unresolved: `EffectFilter` still has no `supertypes`/`excluded_supertypes` (e.g. "destroy all
  legendary permanents") - out of scope for this slice, next follow-up if a real card needs it.
  Scenario TOML `continuous_effect` fixtures still can't express a subtype filter (see the TODO
  left in `crates/scenario/src/lib.rs` above).

## Slice: effect-filter-supertypes (merged)

- Branch `phase5/effect-filter-supertypes`.
- Scope: closes the `effect-filter-subtypes` slice's own named follow-up - `EffectFilter` had no
  `supertypes`/`excluded_supertypes` keys, so "destroy all legendary permanents" (CR 701.8/205.4a)
  wasn't expressible. `EffectFilter` gains `supertypes: Option<SupertypeSet>`/`excluded_supertypes:
  Option<SupertypeSet>` - a bitset like `card_types`, not a `Vec` like `subtypes` (`CardDefinition.
  supertypes`/`TargetFilter::Object::supertypes` are both already bitsets, so no new type shape was
  needed). Checked in both `matches_effect_filter` and `event_target_matches` from the start this
  time - the prior slice's own backlog note named these as two separate hand-written
  implementations that don't share code, so both got the check together rather than one now and one
  discovered later by a failing test.
- Real finding, a second instance of the exact mistake the prior slice's own backlog note warned
  about: the mass find-and-patch script (`perl -pi`, one substitution per matched line, checked
  `unless /supertypes/` on that same line only) doesn't understand Rust syntax, so it also matched
  `TargetFilter::Object` struct literals - a *different* type that happens to share the exact field
  names `subtypes`/`excluded_subtypes`/`supertypes`/`excluded_supertypes` with `EffectFilter`, and
  already had real `supertypes`/`excluded_supertypes` fields from a much earlier slice. Since those
  pre-existing fields lived on a later line than `excluded_subtypes: None,` (not caught by the
  same-line `unless` check), the blanket insertion created genuine duplicate-field `E0062` errors in
  every `TargetFilter::Object` literal it touched - 15+ files, only surfaced once `cargo build --tests
  -p stackfish-engine` was run to force each integration test binary to compile independently (a
  `cargo build --workspace --tests` invocation stops attempting further targets once an earlier
  package fails, so most of these were invisible until that narrower command ran). Fixed with a
  second, context-aware pass: only strip the inline-inserted fields where a genuine duplicate
  followed within the next few lines (i.e. only the `TargetFilter::Object` sites), leaving every real
  `EffectFilter` insertion (which has no such duplicate) untouched. Two of these bad insertions had
  been made *manually* rather than by the mass script (`script/globals.rs`'s two builder defaults,
  `scenario/src/lib.rs`'s `build_continuous_effect`) - the same blanket regex pass re-matched those
  files too and duplicated what had already been added by hand, same root cause either way: a
  same-line-only "already has this field" check can't see a field the literal will have three lines
  down.
- Verify: unit test on `matches_effect_filter` itself (supertypes/excluded_supertypes,
  `types/effect.rs`); a `Game.Ops.DestroyAll` decode test plus an end-to-end "destroy all legendary
  permanents" test sparing a non-legendary creature (`phase5_mass_destroy_effect.rs`); 2 new tests in
  `phase5_etb_replacement_modifiers.rs` (a supertype-filtered externally-granted `EntersTapped`
  affecting a matching legendary land and sparing a non-legendary one) exercising
  `event_target_matches`'s supertype check directly, closing the exact gap the prior slice's tests
  only caught after the fact. Full workspace suite green (160 test-result blocks, zero failures).
- Unresolved: same as the prior slice - scenario TOML `continuous_effect` fixtures still can't
  express a supertype filter either (the same TODO in `crates/scenario/src/lib.rs` now also names
  `filter_supertypes`/`filter_excluded_supertypes`).

## Slice: scenario-continuous-effect-filter-subtypes (merged)

- Branch `phase5/scenario-continuous-effect-filter-subtypes`.
- Scope: closes the TODO the prior two engine-side slices both left in `crates/scenario/src/lib.rs`'s
  `build_continuous_effect` - a scenario TOML fixture had no way to express a subtype- or
  supertype-filtered `continuous_effect`, only a hand-authored Lua script could. `ContinuousEffectSpec`
  gains `filter_subtypes`/`filter_excluded_subtypes`/`filter_supertypes`/`filter_excluded_supertypes:
  Vec<String>` (all `#[serde(default)]`, empty means no filter - same convention `filter_card_types`
  already uses), wired into `build_continuous_effect`'s `EffectFilter` construction in place of the
  `None`/TODO placeholders.
- No new Rust-side logic needed - `parse_supertypes` already existed (built for
  `TargetRequirementSpec.supertypes`) and the `Vec<String> -> Vec<Subtype>` map-and-collect pattern
  was already established at three other call sites (`TargetRequirementSpec.subtypes`/
  `excluded_subtypes`). Purely a TOML-schema/wiring gap, not an engine gap.
- Verify: a new scenario fixture
  (`crates/engine/tests/scenarios/phase5/phase5_continuous_effect_filter_subtype_supertype.toml`,
  auto-picked-up by `scenarios.rs`'s generic runner, no bespoke Rust test file needed) - a Wall
  creature and a Legendary creature each lethally reduced to 0 toughness by their own
  subtype-/supertype-filtered `-0/-1` enchantment, a plain Grizzly Bears left alive by both, same
  "SBA-death via `damage_marked`/toughness modifier" assertion convention
  `phase5_conditional_continuous_effects.toml` already established. Sanity-checked the fixture itself
  isn't a false positive by temporarily mismatching both filter values (`"NotAWall"`/`"Snow"`) and
  confirming the run then fails - restored before committing. Full workspace suite green (160
  test-result blocks, zero failures).
- Unresolved: nothing new - this was purely closing an already-named gap. `docs/scenario_format.md`'s
  own `continuous_effect` reference section updated with the four new TOML keys.

## Slice: mass-choice-effects (merged) - new Game.Ops.* entries

- Branch `phase5/mass-choice-effects` - `Game.Ops.SacrificeChosen`/`DiscardChosen`/`SacrificeRandom`/
  `DiscardRandom`/`ForEachPlayer` all landed this slice, alongside the coroutine-side `Game.ChooseN`.
  Full account (including `AbilityEffect`/`ScriptedZoneMoveKind` changes) lives in
  [12-coroutines.md](12-coroutines.md#slice-mass-choice-effects-merged), grouped with the coroutine
  machinery they build on rather than duplicated here.

## Slice: counter-target-spell (branch `phase5/counter-target-spell`)

- Scope: CR 701.6a - "counter target spell." New `AbilityEffect::Counter` / `Game.Ops.Counter`.
- Key finding that shaped the design: no new targeting plumbing was needed. A spell on the stack
  already has a real `GameObject` at `zone: Stack` sharing its own `StackObject::stack_id`
  (`commands::cast::execute_cast_spell`), and `TargetFilter::Object.zone` plus its Lua decode
  (`decode_target_filter_table`, reused by `s.target_requirement()` via `decode_target_requirement_table`)
  already parsed a `"Stack"` zone name end to end before this slice - `docs/lua/targeting-and-choices.md`
  claimed otherwise ("`:SetTarget`'s own target-requirement filter has no such key"), which was stale,
  not accurate; fixed as part of this slice. CR 608.2b's existing recheck-at-resolution in
  `resolve_stack_top` also covers "the targeted spell already left the stack" for free - the target's
  `zone` check fails once it's no longer `Stack` (every zone change mints a fresh `ObjectId`), so
  `legal_targets` comes back empty and the *Counterspell itself* fizzles via the pre-existing path.
- What's genuinely new: `engine/resolve/counter.rs`'s `resolve_counter_effect` - unlike every other
  single-target `AbilityEffect`, the affected object isn't reached through `continue_scripted_zone_move`'s
  battlefield-object path; it's a *different*, still-present `StackObject` in `state.stack` (a plain
  `Vec`, no prior removal-by-id primitive existed - only `resolve_stack_top`'s own top-of-stack `pop()`).
  Found by `stack_id`, removed via `Vec::remove`, then (only if it was a `Spell`, not an ability - an
  ability "just stops existing," no `ZoneChange`) its card's own zone move reuses `Engine::
  move_resolving_spell` unchanged - the exact helper the CR 608.2b fizzle branch already calls for the
  identical "move this `stack_id`'s `GameObject` to graveyard/exile, no replacement check, no CR 616.1
  ordering pause" shape. New `GameEventKind::Countered { stack_id }`, distinct from `Fizzled` (real
  cards care about the difference - "whenever a spell is countered this way" - not modeled this slice,
  no `ConditionPrimitive` matches it yet, same "primitive ships before its own trigger condition"
  precedent `Damage`/`Scry` already set).
- Verify: 4 new tests (`phase5_counter_target_spell.rs`) - a countered creature spell lands in its
  owner's graveyard and never resolves; the Counterspell itself fizzles (not panics) when its own
  target already left the stack; a `excluded_card_types`-restricted "counter target noncreature spell"
  correctly rejects a creature-spell target at cast time (via `drain()`, not synchronously in
  `submit_action` - target validation is queued, a real test-writing trap hit and fixed while building
  this); a `Game.Ops.Counter` decode test (a bare value like `Destroy`/`Sacrifice`, not a called
  function - `Game.Ops.Counter`, not `Game.Ops.Counter()`, another trap hit and fixed). Full workspace
  suite green, zero new warnings.
- Unresolved, named rather than guessed at: "counter target activated or triggered ability" (Stifle,
  Trickbind) - an ability's `StackObject` has no matching `GameObject` for a `TargetFilter::Object` to
  find, and no ability-shaped target vocabulary exists yet. "Counter unless [that player] pays {X}"
  (Mana Leak, Condescend) - needs a real mid-resolution payment ask, not built.

## Slice: mana-value-target-filter (same branch, `phase5/counter-target-spell`)

- Scope: closes this slice's own just-named "mana value N or less" gap - `TargetFilter::Object` gains
  `mana_value: Option<ManaValueRequirement>` (`types/target.rs`) - `ManaValueRequirement { comparison:
  Equal | AtMost | AtLeast, bound: Fixed(u32) | X }`, `ManaValueBound::X`/`Fixed` mirroring
  `CounterAmount`'s own `Fixed(u32) | X` shape (ability.rs). Covers both Convolute's "mana value 3 or
  less" (`Fixed`) and the user's own ask, "counter target spell with mana value X" (`X`, resolved
  against *this casting's own* announced X) - the same mechanism generalizes to any `TargetFilter::
  Object` consumer, not just `Counter`.
- Key design call: considered exposing this purely as a `TargetFilter::Script` predicate (the stack and
  mana costs aren't in the Lua state snapshot at all - a real, separate, pre-existing gap named in
  `state_to_lua`'s own doc comment) instead of a new `TargetFilter::Object` field - rejected as the
  bigger, messier lift (would need to build stack/mana-cost Lua exposure first); the flat-field route
  needed only a ~76-site mechanical struct-literal ripple (`mana_value: None` on every existing
  `TargetFilter::Object` literal - the exact same shape/size ripple `effect-filter-subtypes` already
  took once), done via a small brace-counting Python script (perl's own past false-positive trap on
  this exact ripple class - see `effect-filter-supertypes`'s backlog note - avoided by anchoring the
  match on the fully-qualified `TargetFilter::Object {` text, not a bare field-name heuristic). Real
  finding: the same blind text-substitution approach also "fixed" two doc comments that happened to
  contain literal `TargetFilter::Object { ... }` example text (ability.rs, this slice's own test file's
  module doc) - caught by a targeted comment-line grep after the fact and reverted by hand; not caught
  by the compiler since a doc comment isn't type-checked.
- `commands::target_is_legal` gains a `caster_x_value: Option<u32>` parameter (the casting/resolving
  spell's own announced X, threaded from `StackObject::x_value`/`CastSpell::x_value` through
  `validate_targets`/`validate_one_target_requirement`/`split_targets_by_requirement`/
  `assign_targets_by_filter`) - `None` from every call site with no live X context (an activated
  ability, a triggered ability's own CR 603.3d candidate scan, an SBA illegal-attachment recheck, an
  Aura's own entry-attach legality, a `Game.SelectTargets` mid-resolution ask) resolves a `mana_value`
  filter's own `X` bound as `0`, same as CR 202.3b's real off-the-stack default. `cast.rs`'s own
  `validate_targets` call passes the real `x_value` cast-time parameter; `engine/resolve/mod.rs`'s
  CR 608.2b recheck and `misc_effects.rs`'s `ExchangeTextBoxes` recheck pass `top.x_value`.
- The candidate's *own* mana value (not the caster's) is computed via `ManaCost::resolve_x` against
  whatever `StackObject::x_value` that candidate itself carries if it's currently in `state.stack`
  (`None`/off-stack defaults to 0) - CR 202.3b's own "only a live-on-the-stack object gets its real X"
  rule, so a countered X-cost spell's own mana value is computed correctly regardless of which side of
  the interaction announced which X.
- Lua decode: `decode_target_filter_table` (shared by `Game.SelectTargets`/`:SetTarget`/
  `s.target_requirement()`) gains `mana_value_equals`/`mana_value_at_most`/`mana_value_at_least`, each
  a plain number or the literal string `"X"` (`decode_mana_value_bound`) - flat independent keys
  matching `up_to`/`at_least`'s own "no nested table" convention, not a single nested comparison table.
- Verify: 4 new tests (`phase5_counter_target_spell.rs`) - an `{X}` Counterspell with X=3 counters a
  target whose own mana value is 3; the same card with X=2 against a mana-value-3 target is rejected at
  cast time (`InvalidTarget`, before the cast even reaches the stack); a `mana_value_equals = "X"`
  decode test; a `mana_value_at_most = 3` decode test. Full workspace suite green (unchanged pass count
  plus these 4), zero new warnings.
- Unresolved: "counter target spell with mana value N or less" (Convolute) is real now via
  `mana_value_at_most`, but nothing yet combines `mana_value` with `TargetFilter::Script` (a scripted
  predicate can't currently also reference the casting's own X - not needed by anything built this
  slice). Scenario TOML `[cards.target_requirement]` has no `mana_value_equals`/`mana_value_at_most`/
  `mana_value_at_least` keys either (`crates/scenario/src/lib.rs`'s `build_target_requirement` defaults
  the new field to `None`, same "engine/Lua first, TOML follow-up if a fixture ever needs it" posture
  the subtypes/supertypes filter fields took before their own scenario-side follow-up slice).
