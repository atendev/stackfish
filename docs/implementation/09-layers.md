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

## Slice: vesuvan-linked-copy-ability (merged)

- Branch `phase5/vesuvan-linked-copy-ability`.
- Scope: `copy-generation`'s own named next step — CR 707.9a's *other* real shape, a copy effect
  granting a whole new ability (not just a keyword) as part of copying. Vesuvan Doppelganger's own
  "except it has 'at the beginning of your upkeep, you may have this creature become a copy of target
  creature'" is the one concrete real-card shape this engine now supports.
- Key finding that reshaped the plan mid-scoping: `CR 707.9e/g`'s own `linked_ability_generation`
  supersession check (built last slice) turns out to be structurally unreachable through *either* of
  this engine's two real copy producers, even with a real producer now in place —
  `commands::purge_granted_abilities` unconditionally deletes every ability sourced from the copying
  object (by `source`, not by tag) *before* either producer re-registers anything, so a re-copy always
  deletes the *previous* granted ability outright rather than merely superseding it; the newly granted
  one always gets a fresh `AbilityId` tagged with the new generation. The supersession check the
  previous slice built never actually fires in real play as a result — confirmed by direct test
  (`become_copy_with_grant_recurring_copy_trigger_regenerates_the_linked_ability_each_time`), not just
  reasoned about. Built anyway: CR 707.9a itself (the actual thing this slice closes) doesn't depend on
  the generation-tag mattering, and the tag stays real, correct metadata regardless.
- Key changes:
  - `CopyExceptions.grant_recurring_copy_trigger: bool` (`Default` extended to `false`, `Clone` already
    derived) — the flag `commands::grant_linked_copy_ability` (new, `attach.rs`, alongside
    `purge_granted_abilities`) consults.
  - `grant_linked_copy_ability(state, object, exceptions, generation)`: a no-op unless the flag is set;
    otherwise mints a fresh `AbilityId` and pushes a hardcoded `RegisteredAbility` — `Triggered`,
    `ConditionPrimitive::BeginningOfYourUpkeep`, a `TargetRequirement` for one creature, `effect: Some(
    AbilityEffect::BecomeCopy(exceptions.clone()))` — then tags it in `linked_ability_generation`. A
    hardcoded Rust shape rather than something `run_card_script`/Lua produces: no "arbitrary ability
    blueprint" type exists to author one generically, and this is the only real card that needs one.
    Carrying `exceptions` forward into the granted ability's own effect is what makes it self-
    perpetuating — resolving it re-grants the same ability again, matching Vesuvan's own "keeps
    flipping indefinitely" text.
  - Called from both real producers (`resolve_become_copy_effect`, `submit_copy_choice`), right after
    their existing `purge_granted_abilities` + `run_card_script` + `copy_generation` bump.
  - `script::decode::decode_copy_exceptions` recognizes `grant_recurring_copy_trigger` as a boolean key,
    shared by all three of `Game.Ops.BecomeCopy`/`Game.Tokens.CopyOf`/`Game.ReplacementPayloads.
    EntersAsCopy`'s own `opts` decoding.
- Deliberately narrow, named in code comments: CR 601.2c's "may" isn't modeled as a genuine opt-out
  pause — this engine has no "decline a triggered ability" mechanism at all yet, so the granted ability
  simply requires a real target the same way any other targeted trigger does, and fizzles at resolution
  with none available. More consequentially: CR 603.3c ("triggered abilities that require targets
  aren't choosable yet," a pre-existing gap named in `triggers_pipeline.rs` well before this slice)
  means the granted ability can never actually *resolve* through the real turn-based upkeep pipeline
  today — `PutTriggersApnap` always pushes it with an empty `targets` list, which fizzles at CR 608.2b's
  own recheck. The granted ability is real, correctly shaped, and appears in `state.abilities` exactly
  as CR 707.9a describes; it just can't fire yet through ordinary gameplay, same "real but currently
  unreachable, named not hidden" posture this codebase already takes for comparable gaps.
- Verify: 3 new tests. `phase5_clone_enters_as_copy.rs` (`choosing_a_vesuvan_style_copy_grants_the_
  linked_upkeep_ability`/`declining_a_vesuvan_style_copy_grants_no_linked_ability`) drive the real CR
  707.6 `EntersAsCopy` pause end to end and confirm the grant (or its absence) and the generation tag.
  `phase5_layer_engine_copy.rs`'s own new test drives two real `BecomeCopy` activations (working around
  CR 603.3c by activating the granted ability's own *effect* shape through a separately re-registered
  `Activated` ability rather than through the real Triggered one, since only the effect/regeneration
  behavior — not the natural-trigger path — is what this test targets) and confirms the linked ability
  regenerates with a fresh id each time, proving `linked_ability_generation`'s own supersession check is
  moot here as reasoned above. Full workspace suite green (161 test-result blocks).
- Unresolved: CR 603.3c (triggered-ability targeting) blocks this ability from ever firing through real
  gameplay — closing that is a separate, pre-existing, much larger gap (`triggers_pipeline.rs`'s own
  named TODO), not scoped here. CR 707.9e's "additional effect" exception (distinct from 9a's "gain an
  ability") still has no producer of any kind.

## Slice: copy-additional-effect-exception (branch open)

- Branch `phase5/copy-additional-effect-exception`.
- Scope: `vesuvan-linked-copy-ability`'s own named next step — CR 707.9e's "additional effect"
  exception, a different shape from every existing `CopyExceptions` field: `set_power_toughness`/
  `add_keywords`/`retain_name`/`retain_color` all modify the copy snapshot's `CardDefinition`
  fields (`CopyExceptions::apply`), while 707.9e's own worked examples (Altered Ego's "enters with
  X additional +1/+1 counters," Spark Double's "if it's a creature, it enters with an additional
  +1/+1 counter") are a real `GameObject`-level mutation instead - a counter, not a characteristic.
- Key changes:
  - `CopyExceptions.enters_with_additional_counters: Option<(CounterKind, u32)>` (`Default`
    extended to `None`) - deliberately *not* folded into `CopyExceptions::apply`, since that
    function only ever touches the `CardDefinition` snapshot.
  - `commands::apply_copy_additional_counters(object, exceptions)` (`attach.rs`, alongside
    `grant_linked_copy_ability`): a no-op unless the field is set, otherwise returns a
    `GameEventKind::CounterAdded` for the caller to push - reuses `CommitEvents`' existing generic
    counter-apply path rather than mutating `GameObject.counters` directly (the exact "pick exactly
    one" mistake this backlog's own mistakes-caught section already names for `EntersWithCounters`).
  - Called from both real copy producers (`resolve_become_copy_effect`, `submit_copy_choice`),
    right alongside their existing `grant_linked_copy_ability` call.
  - `script::decode::decode_copy_exceptions` recognizes `counter_kind`/`counter_amount` (Lua table
    keys, required together like `power`/`toughness` already are), reusing the existing
    `parse_counter_kind_name` helper.
- Deliberately out of scope, named in code comments (`CopyExceptions`'s own doc comment,
  `apply_copy_additional_counters`'s own doc comment): CR 707.9e's own second sentence - the
  exception's effect doesn't happen if *another* copy effect applies to the object first. This only
  matters when a copy effect's own granted ability re-fires a second copy effect *during the same
  entering event* (the rule's own Altered Ego example: copying Clone grants Clone's own "choose a
  creature to copy" replacement, which re-applies before Altered Ego's own queued counters do) -
  this engine has no nested/chained copy-effect-during-one-ETB producer at all, a separate, bigger
  gap than this slice's scope. Only a fixed counter amount is modeled (Spark Double's shape); no
  X-costed variable amount (Altered Ego's own shape) - that would need X-cost-at-copy-time
  plumbing this engine doesn't have for `BecomeCopy`/`EntersAsCopy` today.
- Verify: 4 new tests. `phase5_layer_engine_copy.rs`
  (`become_copy_with_additional_counter_exception_adds_a_real_counter`/
  `become_copy_without_the_exception_grants_no_counter`) drive `AbilityEffect::BecomeCopy` directly.
  `phase5_clone_enters_as_copy.rs`
  (`choosing_to_copy_applies_the_707_9e_additional_counter_exception`/
  `declining_to_copy_does_not_apply_the_additional_counter_exception`) drive the real CR 707.6
  `EntersAsCopy` pause end to end, through a real Lua script exercising the new
  `counter_kind`/`counter_amount` decode keys. Full workspace suite green.
- Unresolved: CR 707.9e's own invalidation clause (named above). CR 707.9e's X-costed variable
  amount. CR 707.9f (exceptions conditional on the copy's own resulting characteristics) untouched.

## Slice: chained-copy-effects (merged)

- Branch `phase5/chained-copy-effects`.
- Scope: CR 707.9e's own second sentence, named unmodeled by every prior slice in this cluster —
  "If another copy effect is applied to that object after applying the copy effect with that
  exception, the exception's effect doesn't happen." The rule's own worked example: Altered Ego
  copies Clone; Clone's own granted "enter as a copy" ability then lets you choose a second
  creature to copy, and because a second copy effect is applied, Altered Ego's own extra-counters
  exception never happens. Scoping turned up a much smaller fix than expected — no new
  `ProcessState`/`StepStatus` variant, no recursive call stack: `submit_copy_choice` already
  re-runs the copied definition's own script (`run_card_script`), which self-registers a fresh
  `EntersAsCopy` if the copied creature is itself a Clone/Altered-Ego-family creature. This slice
  just re-checks `replacement::pending_copy_choice` right after that (same synthetic-event shape
  the original raise site uses — it matches on the candidate event's `from`/`to` fields, not
  `object`'s live zone, so it works even though `object` already sits on the battlefield), and if a
  chain is found, re-enters `AwaitingCopyChoice` instead of finishing. `Engine::mod.rs`'s own
  step-status derivation already reads whatever's in `ProcessState::AwaitingCopyChoice` generically,
  so the second (and any further) pause surfaces to callers exactly like the first one did.
- Key changes:
  - `ProcessState::AwaitingCopyChoice` gained `carry_forward_if_declined: Option<CopyExceptions>` —
    `None` for a first pause (nothing's copied anything yet). When `submit_copy_choice` detects a
    chain, it defers this round's own additional-effect exceptions here instead of applying them
    immediately, since CR 707.9e's suppression condition ("another copy effect is *applied*... after")
    isn't actually met yet — only offered. If the next pause picks a real target, a second copy
    effect really was applied, so the deferred exceptions are conclusively superseded and never read
    again. If instead it's declined, no second copy effect was ever applied, so `submit_copy_choice`
    applies the deferred exceptions after all (found this asymmetry the hard way — the first
    implementation attempt discarded the deferred exceptions unconditionally the moment a chain was
    offered, which silently ate Spark Double's own counter in the "chain offered then declined" case;
    caught by `declining_the_chained_choice_still_applies_the_first_rounds_exception`, which failed
    with 0 counters instead of 1 on the first run).
  - `apply_copy_additional_effect` (`choice_answers.rs`, new private helper) bundles
    `grant_linked_copy_ability` + `apply_copy_additional_counters` into one call, shared by the
    "no chain" path and the "chained pause was declined" path — both mean the same thing structurally
    (this round's application was never superseded).
  - `grant_linked_copy_ability` (CR 707.9a, Vesuvan's granted-ability exception) gets the identical
    chain-suppression treatment as the counter exception, not a narrower one — CR 707.9e's own
    phrasing ("an exception that's an additional effect... rather than a modification of the
    affected object's characteristics") covers both alike, not just counters. No real card combines
    both in one chain, but there was no rules reason to treat them differently once the mechanism
    existed, so both are suppressed/deferred uniformly rather than picking one arbitrarily.
  - A chain can go arbitrarily deep — no depth cap. Each further link requires its own external
    `submit_copy_choice` call (declining at any step ends it), so there's no runaway-loop risk in the
    engine itself; `copy_generation` bumps once per real link, verified to increment correctly across
    a 2-deep chain.
- Verify: 4 new tests (`phase5_chained_copy_effects.rs`) — a chain re-pauses instead of finishing
  (and the outer round's counters haven't landed yet at that point); a completed 2-deep chain
  suppresses the first round's counters entirely, ends as a copy of the *final* link, and logs two
  real `BecameCopy` events; declining the chained pause still applies the first round's counters
  (no second copy effect was ever actually applied); a non-chaining target (no `EntersAsCopy` of its
  own) applies the exception immediately with no extra pause, same as before this slice. Full
  workspace suite green (17/17 across both `phase5_chained_copy_effects.rs` and the pre-existing
  `phase5_clone_enters_as_copy.rs`, unchanged).
- Unresolved, confirmed out of scope rather than narrowed by mistake: Altered Ego's own X-costed
  counter amount (as opposed to Spark Double's fixed one, exercised here) — `x_value` today lives
  only on `StackObject`, consumed once by mana-cost payment, and is never threaded into
  `run_card_script`'s Lua call or either copy-choice pause struct; making X available at
  copy-resolution time is separate, unrelated new plumbing, deferred to a future slice.
  `resolve_become_copy_effect` (`AbilityEffect::BecomeCopy`, CR 707.4) isn't given the same chaining
  treatment — it doesn't move zones and has no existing chain-detection hook to extend, a narrower
  sibling gap named here rather than silently extended into.

## Slice: altered-ego-x-counters (branch open)

- Branch `phase5/altered-ego-x-counters`.
- Scope: `chained-copy-effects`'s own named next step — Altered Ego's X-costed counter amount, as
  opposed to Spark Double's fixed one. CR 707.9e's Altered Ego worked example itself: "You may have
  this creature enter the battlefield as a copy of any creature on the battlefield, except it enters
  with X additional +1/+1 counters on it."
- Key changes:
  - `CopyExceptions.enters_with_additional_counters`'s amount widened from a bare `u32` to a new
    `CounterAmount { Fixed(u32), X }` (one field, two-shaped amount — not a second parallel `Option`,
    so "fixed and X both set at once" isn't representable). Only ~5 construction sites (mostly
    tests), so widening the existing field beat adding a sibling.
  - `commands::apply_copy_additional_counters`/`choice_answers::apply_copy_additional_effect` gained
    an `x_value: Option<u32>` parameter to resolve `CounterAmount::X` (CR 107.3b: undefined X treated
    as 0, same as no `{X}` in the cost at all).
  - `ProcessState::AwaitingCopyChoice` gained `x_value: Option<u32>`, captured once from
    `top.x_value` at `continue_permanent_entry`'s raise site (the entering object's own paid X) and
    carried unchanged through `submit_copy_choice`'s chained re-pause — the X belongs to the entering
    permanent itself (paid once, at cast time), not to whichever candidate ends up copied at any one
    link in a chain, so it's invariant through the chain the same way `colors_spent`/`stack_id`/
    `controller` already are.
  - `resolve_become_copy_effect` (`AbilityEffect::BecomeCopy`, Vesuvan's own producer) gets the
    identical `top.x_value` plumbing for symmetry with the `EntersAsCopy` path, though no real
    `BecomeCopy` card needs `CounterAmount::X` — unexercised by a real card, proven only by the type
    system and the shared helper, same "real but currently unreachable, named not hidden" stance
    `grant_recurring_copy_trigger` already takes elsewhere in this cluster.
  - `script::decode::decode_copy_exceptions` accepts `counter_amount = "X"` (a Lua string) alongside
    the existing numeric case — read as a raw `mlua::Value` first (a bare `.get::<u32>()` silently
    fails to match a string rather than erroring), matched by hand into `CounterAmount::Fixed`/`X`,
    erroring on anything that's neither a non-negative number nor `"X"`.
- Deliberately out of scope: a chain where *two* independent links each carry their own X-costed
  exception — no real card stacks two X-cost copy effects in one chain, so only "one X survives a
  chain" needed proving, not "two independent X's coexist."
- Verify: 5 new tests (`phase5_altered_ego_x_counters.rs`) — `counter_amount = "X"` decodes to
  `CounterAmount::X`; a non-number/non-`"X"` value is a script error at registration time; casting
  with an announced X=3 and choosing to copy lands exactly 3 counters; X=0 lands 0 (not a fixed
  fallback); declining a chained copy (Altered Ego copies a plain Clone link, decline the Clone
  link's own re-offered choice) still applies Altered Ego's own deferred exception using the
  *original* X=5, not a re-derived or reset value. Full workspace suite green.
- Unresolved: CR 707.9f (exceptions conditional on the copy's own resulting characteristics)
  untouched, same standing gap named since `copy-additional-effect-exception`.

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

## Slice: volraths-shapeshifter (merged)

- Branch `phase5/volraths-shapeshifter`.
- Scope: CR 612.6 (Volrath's Shapeshifter) — "full text of another object" changes not just the
  text box/type line (CR 612.5's own narrower `exchange-of-words` scope) but also the text that
  represents name, mana cost, color indicator, power, and toughness. Named unmodeled by
  `layer-engine-text`'s own closing note ever since. CR 612.7 (Spy Kit, "all names of nonlegendary
  creature cards") was scoped out entirely, confirmed with the user before starting — it has *zero*
  supporting substrate in this engine (no dynamic name-predicate matching anywhere; the only name
  comparison, the legend rule's own grouping, is a fixed-string equality check; no card-catalog
  capability exists to even enumerate "every nonlegendary creature card"), a much bigger, separate
  gap than 612.6's.
- Key design decision, confirmed with the user before implementing: modeled as a live
  `ContinuousEffect` (Layer 3, reusing the existing `EffectDuration`/timestamp/condition machinery
  for free — Volrath's Shapeshifter's own real duration, "until end of turn," needs genuine
  live/reversible behavior, not a one-time mutation) rather than reusing `AbilityEffect::BecomeCopy`/
  `ExchangeTextBoxes`'s existing "mutate `copiable_override`/re-run script once, never revert" shape.
  That decision has a real, structural consequence: `layers::effective_characteristics` is a pure,
  read-only query with no `&mut GameState` — it can apply new plain-data field overrides (which is
  all this slice needs) but can never re-run a script or register a real ability the way the
  one-time-mutation precedents do. So CR 612.6 is deliberately narrowed here to the *characteristic*
  half only (name/mana cost/color/type line/keywords/power/toughness) — a Shapeshifter using this
  effect looks and fights like the copied creature, but doesn't gain its activated/triggered
  abilities. Named, not hidden - a structural fact of this design choice, not an oversight.
- Key changes:
  - `FullTextOverride` (`types/effect.rs`) — a new struct carrying just the fields
    `EffectiveCharacteristics` actually derives from a `CardDefinition` (not the whole struct, which
    also carries ability/cost/target-requirement fields with no "text" meaning here at all).
    `From<&CardDefinition>` for easy construction from a real donor. Doesn't derive `PartialEq`/`Eq`
    automatically (`ManaCost`/`PTValue` don't derive them either) — a minimal by-name `PartialEq` is
    hand-written instead, since `EffectPrimitive`'s own derive just needs *something* to call, not a
    deep structural comparison no real caller will ever perform.
  - `EffectPrimitive::SetFullText(Box<FullTextOverride>)` — Layer 3, boxed to avoid bloating every
    other `EffectPrimitive` variant with this one's much larger payload.
  - `layers::effective_characteristics` restructured: `mana_cost`/`supertypes`/`subtypes` became
    mutable locals seeded from `def` up front (previously read straight off `def` at the very end) so
    `SetFullText`'s own match arm — folded into the *same* ordered Layer 3 pass `SetName` already
    runs in, rather than a second loop — can override every characteristic this slice touches,
    including re-invoking `resolve_base_pt` against the *donor's* own `power`/`toughness` (so even a
    donor with a `PTValue::CharacteristicDefining` CDA base transfers correctly, for free — CDA
    resolution is itself a pure, side-effect-free script call, unlike ability registration).
    Timestamp ordering between `SetFullText` and a separate plain `SetName` in the same layer/pass
    falls out correctly without any special-casing: a later `SetFullText` overwrites `name` too
    (among everything else), but a later plain `SetName` only ever touches `name` on top of it.
- Verify: 6 new tests (`phase5_volraths_shapeshifter.rs`) — `FullTextOverride::colors()`'s own
  derivation, a real `effective_characteristics` consumer swapping every touched field, a `self_only`
  check against an unrelated object, both timestamp-ordering directions against a separate `SetName`,
  and a real end-to-end `EffectDuration::UntilEndOfTurn` expiry test (advances through a real Cleanup
  step, confirming the effect is drained and the object reverts to its own printed identity) — this
  last one needed *zero* new expiry-handling code, since `engine/turn.rs`'s existing
  `begin_cleanup_step` already `retain`s non-`UntilEndOfTurn` effects generically, with no awareness
  of which specific payload it's draining. Full workspace suite green.
- Unresolved, confirmed narrowed rather than a gap in disguise: no producer wires a real activated
  ability that captures "target creature card in a graveyard" into a fresh `SetFullText` effect yet
  (proven via direct `ContinuousEffect` construction only, same posture several other first-proof
  slices in this cluster already took) — a real Volrath's Shapeshifter card script would need a new
  `AbilityEffect` variant to build that target-capture-into-a-new-continuous-effect shape, a separate
  follow-up. No Lua exposure either — `CardDefinition` is a large struct; a full Lua table decoder
  for it is disproportionate for one card. CR 612.7 (Spy Kit) remains entirely unaddressed, per the
  scoping note above.

## Slice: bands-with-other-keyword-loss (merged)

- CR 702.22b: "if an effect causes a permanent to lose banding, the
  permanent loses all 'bands with other' abilities as well." Closes the
  last named gap in CR 702.22 (banding).
- Backlog's own prior note on this ("no effect in this engine removes a
  keyword yet, untestable") was stale — `EffectPrimitive::RemoveKeyword`
  already existed, real and Lua-exposed (`keyword-layer-fix-and-remove-
  keyword` above), never logged back to that line. Caught by grepping the
  actual code before scoping, not trusting the prose.
- The real gap: `effective_characteristics`'s layer 6 fold already
  accumulates `bands_with_other_subtypes`/`bands_with_other_supertypes`
  fully independently of `keywords` — `RemoveKeyword(BANDING)` cleared
  Banding itself but left a creature's separate bands-with-other grant
  untouched. Fixed with one check at the end of the fold: if `def.keywords`
  (this object's own printed-or-copied base) contains Banding but the final
  accumulated `keywords` no longer does, both `bands_with_other_*`
  accumulators are cleared — the same cross-clear `RemoveAllAbilities`
  already does as a side effect of wiping everything.
- Deliberately gated on *printed* Banding (`def.keywords`), not "effective
  set merely lacks it" — `phase5/bands-with-other`'s own
  `quality_band_legal_with_no_banding_member_at_all` test already proved a
  creature can legally grant bands-with-other while never printing Banding
  at all (CR 702.22c's second clause allows this, no real card does it but
  it isn't illegal); that shape must not be affected by an actual *loss*
  rule. `def` (the copy-aware `copiable_override.as_ref().unwrap_or(&obj.
  definition)`) was already in scope at the point this check needed it, no
  new plumbing required.
- `commands::combat::declare::DeclareBands`/`damage.rs`'s
  `has_bands_with_other_pair` needed zero changes — both already consume
  `effective_characteristics` generically.
- Verify: 6 new tests (`phase5_bands_with_other_keyword_loss.rs`) - unit
  level (subtype and supertype forms both clear on loss; an unrelated
  keyword's removal leaves bands-with-other untouched; a creature that
  never had Banding at all is unaffected) plus end-to-end `DeclareBands`
  (a quality band that would be legal is rejected once Banding is stripped,
  and a control test proving it's legal without the strip). Full existing
  `phase5_bands_with_other.rs`/`phase5_banding.rs`/`phase5_bands_with_
  other_effect_blocked.rs` suites re-run clean, including the "never had
  Banding" test this slice's own gating was built specifically not to
  break. Full workspace suite green.
- Unresolved: none — CR 702.22 has no further known gap.

## General layer notes

- Layer order in `effective_characteristics`: 3 text (CR 612.6/612.8, `volraths-shapeshifter`/
  `layer-engine-text` — runs first, since `SetFullText` can override what every later layer treats
  as its own base), 2 control, 4 type, 5 color, 6 abilities, 7a CDA, 7b set P/T, 7c modify P/T
  interleaved with counters (CR 613.4c, `phase5/layer7d-counter-interleaving`), 7e switch.
- Dependency ordering only active in 7c due to `DoublePT`; other layers use timestamp order. Counters merge into 7c by timestamp comparison, not the dependency graph itself (they have no dependency of their own).
