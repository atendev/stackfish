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
    CR 612.5 (Exchange of Words) is real now — see `exchange-of-words` below.
    612.6 (Volrath's Shapeshifter, "full text" — name/mana cost/color/P&T
    included, not just the text box) and 612.7 (Spy Kit, "all names of
    nonlegendary creature cards") stay unmodeled — each is its own one-off
    shape, not a shared mechanic the way a keyword ability is.

## Slice: exchange-of-words (merged)

- Branch `phase5/exchange-of-words`.
- Scope: CR 612.5 — "exchange the text boxes of two objects." Picked as the
  most tractable of the three single-card CR 612.5-612.7 rules: CR 612.1
  scopes text-changing effects to "rules text ... and/or ... the type line,"
  and 612.5 itself says only "text boxes" — name, mana cost, color, P/T, and
  card types/supertypes/subtypes all stay exactly as printed on each
  permanent. Explicitly not a CR 707 copy effect (separate rule family, no
  `CopyExceptions` interaction).
- Key finding that shaped the plan: a permanent's *functional* abilities
  aren't layer-computed at all in this engine — `RegisteredAbility`/
  `ContinuousEffect`/`ReplacementEffect` entries are one-time registrations
  `run_card_script` drops into flat `GameState` lists at battlefield entry,
  keyed only by `source: ObjectId`, never re-derived from `CardDefinition`.
  `BecomeCopy` is the only existing precedent for changing what abilities a
  *live* permanent has (`purge_granted_abilities` + re-run `run_card_script`
  against a new snapshot) — this slice reuses that machinery, but
  symmetrically (once per target) and scoped to only the text-box fields.
- Verify: 4 new tests (`phase5_exchange_of_words.rs`), full workspace suite
  green.
- Key changes:
  - `AbilityEffect::ExchangeTextBoxes` — no fields, same toggle shape as
    `BecomeCopy`/`Attach`; the two targets live on the `StackObject`.
  - `ability::apply_text_box(snapshot, other)` — overwrites just
    `oracle_text`/`keywords`/`script_ref` on `snapshot` from `other`, unlike
    `CopyExceptions`' full-copy-by-default shape.
  - `ResolveStackTop`'s dispatch: captures both targets' current snapshot
    (`copiable_override` if present, else `definition`) *before* mutating
    either (order-of-operations hazard flagged in the code — mutating one
    first would corrupt the source the other reads from), builds two
    synthetic definitions, sets `copiable_override` on both, then
    `purge_granted_abilities` + `run_card_script` for each against the
    other's text box.
  - Inherently pairwise, unlike a uniform multi-word spell (Plague Spores):
    rechecks both targets directly against their own `TargetRequirement`
    (a *fresh* `is_legal_now` closure, scoped to this match arm only — reusing
    the outer one would extend its `&self.state` capture across every
    `&mut self.state` use in every other match arm, since the borrow checker
    can't see match arms are mutually exclusive at runtime) rather than
    consuming the flattened `legal_targets` list, which loses positional
    pairing. Anything short of two still-legal `Target::Object` targets is a
    silent no-op — no partner to exchange with, not a one-sided change.
  - `GameEventKind::TextBoxesExchanged { a, b }` — pure ledger record, same
    "already mutated, log it" shape `BecameCopy` uses.
  - `Game.Ops.ExchangeTextBoxes` — bare toggle value, no Lua constructor
    function (same as `Game.Ops.Attach`).
- Unresolved: no duration/reversion (permanent until another effect changes
  it, matching `BecomeCopy`'s own precedent, not a new gap). `target_requirement`/
  `effect`/`modes`/`alternative_costs` aren't part of the swap — those only
  matter while a card is being cast as a spell, not to a live permanent's
  ongoing behavior, so they're irrelevant post-resolution.

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

## Slice: clone-enters-as-copy (merged)

- Branch `phase5/clone-enters-as-copy`.
- Scope: CR 707.6/614.12a — "you may have this creature enter as a copy of any creature on the
  battlefield" (Clone itself). The third real "become a copy" entry point, after `BecomeCopy` (an
  existing permanent re-copying something) and `TokenSpec::CopyOf` (a token copying something) —
  explicitly named as a gap since `copy-exceptions`/`layer-engine-copy`'s own notes ("no
  ETB-replacement 'enters as copy' path (Clone itself)").
- Verify: 9 new tests (`phase5_clone_enters_as_copy.rs`), full workspace suite green.
- Key changes:
  - `ReplacementPayload::EntersAsCopy(CopyExceptions)` — same "never applied by
    `apply_enters_battlefield_modifiers`, needs a real mid-ETB pause" shape Devour/Amplify already
    established.
  - `replacement::pending_copy_choice(state, script_runtime, object, from) -> Option<(CopyExceptions,
    Vec<ObjectId>)>` — candidates are every creature on the battlefield, any controller (Clone's own
    unrestricted wording, unlike Devour's "creatures you control"/Amplify's "cards from your hand");
    no `controller` parameter at all, since there's nothing to filter by.
  - `ProcessState`/`StepStatus::AwaitingCopyChoice`, `Engine::submit_copy_choice(Option<ObjectId>)` —
    `Option`, not `Vec`, since this is "which one, if any." A real choice reuses `BecomeCopy`'s own
    machinery wholesale (`purge_granted_abilities` + fresh `run_card_script` against the chosen
    creature's snapshot with `CopyExceptions` applied) and logs the same `GameEventKind::BecameCopy`
    rather than a new event. Either way (copied or declined), the resume always runs
    `apply_enters_battlefield_modifiers` afterward, with the entering spell's own real
    `costs_paid`-derived `colors_spent` carried through the pause — this is what makes CR 707.2's own
    Skyshroud Behemoth worked example real (a Clone copying a Fading + "enters tapped" creature enters
    tapped with fade counters too).
  - Checked in `ResolveStackTop` *before* the existing Devour/Amplify checks — becoming a copy is the
    most identity-fundamental of the three mid-ETB choices. The post-choice resume deliberately doesn't
    chain back into Devour's/Amplify's own pause checks (no real Clone-family card has either keyword).
  - `Game.ReplacementPayloads.EntersAsCopy(opts?)` — same `CopyExceptions` decoder `Game.Ops.
    BecomeCopy(opts?)` already uses.
- **Bug found and fixed mid-slice** (not a pre-existing regression from this slice, but a
  latent bug this slice's own test was the first to expose): a single permanent combining
  `EntersTapped` and `EntersWithCounters` — CR 707.2's own worked example — broke two different ways:
  1. The *generic* `apply_replacements_from`/`ReplacementCheck` pipeline independently re-matched
     these battlefield-entry-modifier payloads against the same raw `ZoneChange` event
     `apply_enters_battlefield_modifiers` had already handled, and two matches sharing one source
     spuriously triggered a CR 616.1 ordering pause for two effects that were never supposed to be
     ordered against each other (`apply_enters_battlefield_modifiers`'s own doc comment already says
     so). Fixed with a new `is_battlefield_entry_modifier` filter excluding these payload shapes from
     `apply_replacements_from`'s own `matching` computation entirely.
  2. `apply_enters_battlefield_modifiers`'s own loop re-derived each matched effect's index by
     `source: ObjectId` (`.position(|r| r.source == source)`), which silently assumed one source could
     only ever have one matching effect — with two (both sharing the freshly-copied identity's source
     id), the second lookup kept re-finding the *first* effect, so the second payload was silently
     skipped. Fixed by collecting real `usize` indices up front and processing them in reverse order
     (safe against a mid-loop `OneShot`/`LimitedUses(1)` removal), via a new index-based
     `apply_replacement_at_index` that `apply_one_replacement` itself now wraps.
  Neither bug was reachable before this slice — no prior test combined two battlefield-entry-modifier
  payloads on one permanent, since only a copy-producing effect can put more than one such payload
  under a single freshly-shared source id.
- Unresolved: Search doesn't offer this choice either, same "no real producer reaches this yet"
  posture `pending_devour_choice`'s own doc comment already takes. Self-copy (the entrant choosing
  itself) isn't special-cased away, untested as its own scenario (same posture Devour's own
  self-sacrifice case takes).

## Slice: script-effect-dependency (branch open)

- Branch `phase5/script-effect-dependency`.
- Scope: CR 613.6-8's one real gap left after `layer-engine-dependency` — `EffectPayload::Script`
  effects were always treated as dependency-independent (`layers::depends_on` returned `false`
  outright whenever either side was `Script`), so a Lua-scripted effect could never participate in
  the one real CR 613.8a dependency rule this engine models (`DoublePT` vs. same-field
  `ModifyPT`/`DoublePT`).
- Key finding that shaped the plan: no card anywhere in this engine actually authors a `DoublePT`
  effect from Lua — there wasn't even a `double_pt` key in `ScriptRuntime::invoke_continuous_effect`'s
  seven-key decode table, and no `Game.Effects.DoublePT` tag factory either (Berserk, the real card
  this primitive models, isn't implemented as a card script). Needed to add the decode key just to
  construct a test scenario, not scope creep — the fix itself is otherwise a pure generalization of
  existing machinery.
- Verify: 4 new tests (`phase5_layer_engine_dependency.rs`: scripted-depends-on-primitive,
  primitive-depends-on-scripted, a non-`DoublePT` scripted shape introduces no spurious dependency;
  `phase5_continuous_effects_script.rs`: `double_pt` decode unit test), full workspace suite green
  (`cargo test --workspace`), zero regressions.
- Key changes:
  - `ScriptRuntime::invoke_continuous_effect` gained an eighth recognized key,
    `double_pt = {power=bool, toughness=bool}` -> `EffectPrimitive::DoublePT`.
  - `layers::depends_on` no longer pattern-matches `EffectPayload::Primitive` directly — it resolves
    both sides through the existing `layers::resolve_primitive` helper (already used by every
    per-layer loop in `effective_characteristics`) first, then runs the exact same
    `DoublePT`-vs-`ModifyPT`/`DoublePT` comparison against the resolved shapes. Works in both
    directions (script depends on primitive, primitive depends on script, script depends on script)
    since resolution happens before comparison either way.
  - `layers::order_by_dependency` gained `state`/`script_runtime`/`controller` parameters purely to
    thread through to `depends_on`'s new resolution step — `applicable_effects_in_layer` (the only
    caller) already had all three in scope.
- Unresolved: a script *dynamically* deciding whether it depends on another specific effect (as
  opposed to always resolving to a fixed structural shape like `DoublePT`) would need the script
  bridge to expose partial/in-progress layer state mid-computation, which `state_to_lua`'s snapshot
  deliberately doesn't do — not needed by any real card, since CR 613.8a dependency is a structural
  property of what an effect does, not something a card's own text ever asks a script to decide
  case-by-case. Cross-layer dependency stays out of scope, same as `layer-engine-dependency` already
  noted.

## Slice: copy-retain-exceptions (merged)

- Branch `phase5/copy-retain-exceptions`.
- Scope: CR 707.9c/d — "some copy effects specifically state that they don't copy certain
  characteristics and the affected objects instead retain their original values." The inverse of
  `copy-exceptions`'s own `set_power_toughness`/`add_keywords` (which *provide new* values) —
  narrowed to the two most common real "retains original X" shapes: name and color (CR 707.9d itself
  singles out color for special handling, "any color indicator of that object is also not copied").
- Verify: 6 new tests (`phase5_copy_retain_exceptions.rs`), full workspace suite green.
- Key finding that made this cheap: `GameObject` already keeps both `definition` (the object's own
  true, never-overwritten characteristics) and `copiable_override` (the frozen copy snapshot) —
  `layers.rs` always reads override-or-`definition`. So the "original value to retain" was already
  sitting right there at every copy call site; no new `GameObject` state needed.
- Key changes:
  - `CopyExceptions` gained `retain_name: bool`/`retain_color: bool` (`Default` extended to `false`
    for both — small ripple, `Default` already existed, only 3 exhaustive test literals plus the one
    production Lua-decode site needed the two new fields added).
  - `CopyExceptions::apply` grew an `original: Option<&CardDefinition>` parameter — `Some(&obj.
    definition)` at `BecomeCopy`/`submit_copy_choice` (the copying object's own true pre-copy
    definition, captured before anything mutates), `None` at `TokenSpec::CopyOf`. `retain_color` sets
    an explicit `color_indicator` on the snapshot matching the original's own resolved `colors()`,
    not just a raw field copy — this is what satisfies CR 707.9d's own color-indicator clause.
  - `Game.Ops.BecomeCopy(opts?)`/`Game.Tokens.CopyOf(objectId, opts?)`'s shared `decode_copy_exceptions`
    recognizes `retain_name`/`retain_color` boolean keys alongside the existing ones.
- Scoping notes: `TokenSpec::CopyOf` passing `None` means `retain_name`/`retain_color` are a
  deliberate, tested no-op there — a freshly created token has no prior "own identity" to retain a
  name/color from at all (a real, not just unbuilt, semantic gap: retaining implies there's something
  to retain). Only name/color are supported, not the full CR 707.9c characteristic list (P/T, types,
  subtypes, ...) — no real card combination needs more yet, same posture `set_power_toughness`/
  `add_keywords` were originally scoped with.
- Unresolved: CR 707.9e/g (unchanged — still needs a real `GameObject` schema addition, a copy
  generation/history, not a `CopyExceptions` field; confirmed again while scoping this slice, not
  just carried forward unexamined).

## Slice: copy-generation (merged)

- Branch `phase5/copy-generation`.
- Scope: the schema prerequisite CR 707.9e/g's own unresolved note (above) named three times over —
  "has *another* copy effect applied to this object since a given earlier one did?" CR 707.9e (an
  "additional effect" exception stops applying once superseded) and CR 707.9g (a linked triggered
  ability stops triggering once superseded) both ask the identical question against the identical
  counter; this slice builds the counter and the one real consumer this engine has a hook for
  (triggered-ability matching), and stops there.
- Key finding that shaped (and narrowed) the plan: neither CR 707.9e nor 707.9g has a producer in this
  engine yet — `CopyExceptions` only has characteristic-modification shapes (`set_power_toughness`/
  `add_keywords`/`retain_name`/`retain_color`), never "this copy effect also grants a whole new
  ability" (the real shape Vesuvan Doppelganger's own "except it has [ability]" clause needs). Building
  that producer too would mean inventing a second, unrelated new mechanism (synthesizing a fresh
  `RegisteredAbility` mid-copy) in the same slice — confirmed with the user first that the
  infrastructure alone (real, directly tested) is the right scope, same "real mechanism, no producer
  wired in yet" posture `HandRevealModifier`'s own doc comment already established for an analogous
  gap.
- Key changes:
  - `GameObject.copy_generation: u32` — how many times this specific object has become a copy. Starts
    at `0`; a freshly minted `TokenSpec::CopyOf` token also starts at `0` (a brand-new `object_id` has
    no prior identity to have copied *from* yet, so its first copy application isn't a second one).
    Bumped at both real copy-application sites, right alongside the `copiable_override` write itself:
    `resolve_become_copy_effect` (`misc_effects.rs`, CR 707.2) and `submit_copy_choice` (CR 707.6). ~39
    `GameObject` struct-literal construction sites needed the new field (compiler-driven, mechanical) —
    smaller than it first looked, since `GameObject::new` is an unused `todo!()` stub and the
    overwhelming majority of real construction funnels through `testutil::card_object`, one choke
    point.
  - `GameState.linked_ability_generation: HashMap<AbilityId, u32>` — a side-channel tag (not a new
    `RegisteredAbility` field; that type has 100+ construction sites and no `Default` impl, the same
    "field would ripple everywhere, mostly meaninglessly" reasoning `ManaPool.restricted`'s own side-
    channel already used) mapping a triggered ability to the `copy_generation` its source was at when
    a copy effect granted it.
  - `triggers::collect_triggers`: right after resolving an ability's live `source_object`, a tagged
    ability (`linked_ability_generation.get(&ability.id)` is `Some`) is skipped entirely unless the
    source's *current* `copy_generation` still equals the tagged value — CR 707.9g's actual text, "the
    ability doesn't trigger," falls out as a plain `continue` rather than a special "trigger but no-op"
    path. An untagged ability (the overwhelming majority) is completely unaffected, checked first.
- Verify: `become_copy_bumps_copy_generation` (`phase5_layer_engine_copy.rs`) drives two real,
  independent `BecomeCopy` activations on the same object and confirms the counter reaches `1` then
  `2`; a `copy_generation` assertion added to the existing `EntersAsCopy` choose/decline tests
  (`phase5_clone_enters_as_copy.rs`) confirms choosing bumps it and declining doesn't. Three new direct
  tests of `collect_triggers`'s own consult (`phase5_copy_generation.rs`, no real producer to drive
  them end-to-end yet, same posture `is_hand_revealed`'s own tests already take) — a tagged ability
  matches while its source is still at the granted generation, stops matching the instant the source's
  generation moves past it, and an untagged ability is unaffected by its source's generation regardless
  of value. Full workspace suite green (161 test-result blocks — a new test binary, not a regression).
- Mistake caught while writing the first end-to-end `BecomeCopy`-twice test: reusing this file's own
  `bear_definition()` (an unconditional self-registered `GainLife` trigger, used throughout this file's
  *other* tests) as the first copy target left that trigger sitting on the stack, unresolved, after the
  copy resolved — `activate_pay_and_resolve`'s single `PassPriority` call resolves the `BecomeCopy`
  activation itself, not a *second*, newly-registered triggered ability the copy just granted. Every
  other test in this file only ever does one `BecomeCopy` and never inspects `process_state` afterward,
  so the same latent gap never surfaced before. Fixed by copying a scriptless creature instead — no
  real fix needed in engine code, just a test-fixture choice; logged so a future "activate BecomeCopy
  twice in one test" attempt doesn't rediscover the same trap.
- Unresolved, deliberately: no producer exists for either CR 707.9e or CR 707.9g in this engine — the
  mechanism this slice built is real and directly tested, but nothing yet grants an ability or an
  "additional effect" as part of a copy application. Building one (a new `CopyExceptions` shape,
  synthesizing a fresh `RegisteredAbility` mid-`BecomeCopy`/`EntersAsCopy`) is its own separate,
  larger slice — the natural next step once (or if) a Vesuvan Doppelganger-style card is actually
  wanted.

## Slice: keyword-layer-fix-and-remove-keyword (merged)

- Branch `phase5/keyword-layer-fix-and-remove-keyword`.
- Scope: two things found together while scoping "can attack as though it didn't have defender"
  (Assault Formation): (1) `commands.rs`'s Haste/Defender/Vigilance checks (`has_summoning_sickness`,
  the attack-declaration Defender check, the Vigilance tap-check) read the creature's *printed*
  `CardDefinition.keywords` directly instead of layer-resolved `effective_keywords` — an undocumented
  oversight, confirmed by git history: all three were written before `effective_keywords` even
  existed, and no later slice (`first-strike`, `combat-evasion-keywords`, `landwalk`, `banding`, ...)
  ever came back to migrate them, unlike every other keyword check in the file, which already reads
  `effective_keywords`. (2) `EffectPrimitive::RemoveKeyword(KeywordSet)` — a scoped removal shape
  `RemoveAllAbilities`'s own doc comment had already named as deliberately unbuilt ("there's no
  narrower 'loses these specific keywords' shape here").
- Verify: 10 new tests (continuous-effect-granted Haste/Defender/Vigilance actually respected by
  combat; `RemoveKeyword` letting a printed-Defender creature attack; `RemoveKeyword` only stripping
  the named keyword, not others; timestamp-ordering in both directions against `AddKeyword`;
  `:Register()` pairing rejection; script-backed `remove_keyword` decode), full workspace suite green
  (148 test-result blocks, 0 failed).
- Key changes:
  - `commands::has_summoning_sickness`/the Defender check/the Vigilance tap-check all now call
    `effective_keywords(state, script_runtime, id)` instead of reading `object.definition.keywords`
    directly. The Vigilance site needed a small reorder (compute `effective_keywords` before taking
    `state.object_mut`, since the two borrows can't overlap).
  - `EffectPrimitive::RemoveKeyword(KeywordSet)` (`types/effect.rs`) — layer 6, pairs with
    `AbilityAdding` same as `AddKeyword`. `layers.rs`'s layer-6 fold: `keywords &= !removed`, folded
    in at this effect's own CR 613.7 timestamp position right next to `AddKeyword`'s `keywords |=
    added` — a later-timestamp `AddKeyword` still re-adds whatever an earlier `RemoveKeyword` took
    away (and vice versa), no special CR 613.8a dependency needed, same reasoning `RemoveAllAbilities`
    already documents for the identical interaction.
  - `Game.Effects.RemoveKeyword(name)` (script.rs) — same `parse_keyword_name` decoder `AddKeyword`
    uses; added to `validate_layer_effect_pairing`'s pairing table; `remove_keyword` added as a
    ninth recognized key on the script-backed custom-function effect-shape form
    (`invoke_continuous_effect`), mirroring `add_keyword`'s decode exactly.
  - Docs: `docs/lua/game-constants.md` gained a `RemoveKeyword` row plus the full supported-keyword-
    name table (with CR citations, verified against the actual enforcement code rather than assumed);
    `docs/lua/static-abilities.md`'s script-backed effect-shape table gained `remove_keyword`.
- Unresolved: none new — Banding remains the one real `KeywordSet` bit with no Lua grant path at all
  (unchanged by this slice, already named in `docs/lua/game-constants.md`).

## General layer notes

- Layer order in `effective_characteristics`: 2 control, 4 type, 5 color, 6 abilities, 7a CDA, 7b set P/T, 7c modify P/T interleaved with counters (CR 613.4c, `phase5/layer7d-counter-interleaving`), 7e switch.
- Dependency ordering only active in 7c due to `DoublePT`; other layers use timestamp order. Counters merge into 7c by timestamp comparison, not the dependency graph itself (they have no dependency of their own).
