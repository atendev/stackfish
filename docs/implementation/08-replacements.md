# 08-replacements

## Slice: replacement-prevention-effects (merged)

- Branch `phase5/replacement-prevention-effects`.
- Scope: real `ReplacementEffect` primitives (`PreventDamage`, `RedirectDamage`, `ReplaceZoneDestination`) and `apply_replacements`.
- Verify: 7 unit + 7 integration + 3 engine pipeline tests, 260 total, 0 failed (no hangs after fix).
- Key changes:
  - `EventFilter::{DamageDealt{affected}, WouldDie{affected}}`.
  - `ReplacementPayload::{PreventDamage, RedirectDamage(Target), ReplaceZoneDestination(ZoneKind)}`.
  - `ReplacementSpec::{Primitive, Script}`.
  - `ReplacementEffect::matches`/`apply` real.
  - `apply_replacements_from` CR 616 loop with `excluded` to prevent same effect reapplying to same event.
  - `apply_one_replacement` handles `ConsumptionPolicy` (OneShot, LimitedUses decrement, Repeatable).
  - `affected_player_for` derives CR 616.1 "affected player".
- Bug caught mid-slice: missing `excluded` caused infinite loop on `Repeatable`/`PreventDamage` zeroing already-zero damage.
- Unresolved:
  - `EventFilter` only two shapes.
  - No Lua builder.
  - Combat damage and SBA death did not route through replacement checking (fixed next).

## Slice: fix-replacement-bypasses (merged)

- Branch `phase5/fix-replacement-bypasses`.
- Scope: route `AssignCombatDamage` and SBA-driven deaths through replacement checking.
- Verify: 2 combat + 3 SBA tests, 265 total, 0 failed.
- Key changes (combat damage):
  - `AssignCombatDamage` no longer mutates; only proposes `DamageDealt` events.
  - `commands::apply_damage_event(state, script_runtime, source, target, amount, is_combat_damage)` mutates at `CommitEvents`.
- Key changes (SBA death):
  - `sba::apply_creature_death` split into `collect_dying_creatures` + `resolve_deaths`.
  - `resolve_deaths` proposes `ZoneChange`, runs `apply_replacements` before commit; `DeathResolution::NeedsOrderingChoice` for CR 616.1.
  - `commit_death` parameterized by final `to` zone.
  - `check_state_based_actions` return type changed to `SbaOutcome::{Progressed(Vec<GameEvent>), NeedsOrderingChoice{..}}`.
  - `ReplacementResumeTarget::SbaDeath { pending_deaths, then }`.
- Deviations:
  - 19 direct test callers of `check_state_based_actions` needed `SbaOutcome` wrapping.

## Slice: redirect-damage (merged)

- Branch `phase5/redirect-damage`.
- Scope: CR 614.9 redirection effects, bound to registering permanent's own source.
- Verify: 3 integration + 4 unit tests, 0 failed.
- Key changes:
  - `Game.ReplacementPayloads.RedirectDamageToSource` bound as a sentinel tag.
  - `ReplacementEffectBuilder::SetPayload` substitutes `this.source` into `RedirectDamage(Target::Object(source))`.
  - `ReplacementEffect::matches` rechecks redirect destination validity fresh each application (battlefield + creature/planeswalker/battle/player not eliminated).
  - Fixed player-elimination check: uses `!p.eliminated`, not just `state.player().is_some()`.
- Doc fix: `RedirectDamage` cites CR 614.9, not 614.13.
- Unresolved:
  - Arbitrary chosen redirect targets, computed-from-event targets, attach-machinery destinations.

## Slice: destroy-ordering-choice (merged)

- Branch `phase5/destroy-ordering-choice`.
- Scope: CR 616.1 pause/resume for scripted `Destroy`/`Sacrifice`/`Exile` ops.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `ScriptedZoneMoveKind { Destroy, Sacrifice, Exile }`.
  - `ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, stack_id }`.
  - `Engine::continue_scripted_zone_move` drains target queue, pauses on ordering choice.
  - `Engine::commit_scripted_zone_move` shared commit logic.
  - `ResolveStackTop` `Destroy`/`Sacrifice`/`Exile` arms hand off to helper.
- Unresolved:
  - Exile ordering-choice not end-to-end testable (no `EventFilter` shape matches `to: Exile` yet except via `WouldMoveZone`, added later).

## Slice: eventfilter-would-move-zone (merged)

- Branch `phase5/eventfilter-would-move-zone`.
- Scope: general zone-change replacement matching `WouldMoveZone { from, to, affected }`.
- Verify: 3 unit + 3 integration tests, 0 failed.
- Key changes:
  - `EventFilter::WouldMoveZone { from: Option<ZoneKind>, to: Option<ZoneKind>, affected }`.
  - `WouldDie` untouched (still ~15 sites unchanged).
  - `Game.EventFilters.WouldMoveZone(fromName, toName)` function; nil = wildcard.
  - `validate_replacement_filter_payload_pairing` allows `(WouldMoveZone, ReplaceZoneDestination)`.
- Unresolved:
  - ETB-modifier replacements need new event payload (fixed later, see `etb-replacement-modifiers`
    — though that slice ended up bypassing this event-rewrite mechanism entirely rather than
    adding a payload that fit it, see that slice's own notes for why).
  - Draw/discard/cost-payment producers still mutate directly (fixed next).

## Slice: replaceable-cost-zone-moves (merged)

- Branch `phase5/replaceable-cost-zone-moves`.
- Scope: route `Sacrifice`/`Discard` cost payments through replacement checking.
- Verify: 4 new tests, 0 failed.
- Key changes:
  - `commands::propose_zone_move`/`commit_zone_move` (renamed from cost-specific variants), zone-owner derivation generalizes to Library/Hand.
  - `PendingZoneMove` (renamed from `PendingCostZoneMove`).
  - `ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id }`.
  - `Engine::continue_cost_zone_moves` propose/replace/commit queue.
  - `submit_payment` commits immediate payments, queues zone moves, hands off to helper.
- Unresolved:
  - `draw_one_card`, `submit_discard` still mutate directly.

## Slice: replaceable-cleanup-discard (merged)

- Branch `phase5/replaceable-cleanup-discard`.
- Scope: route CR 514.1 cleanup hand-size discard through replacement checking.
- Verify: 3 new tests, 0 failed.
- Key changes:
  - `ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger }`.
  - `Engine::continue_discard_moves` propose/replace/commit queue, exits to `CleanupSbaCheck`/`CollectTriggers`.
  - `submit_discard` shrunk to validation + queuing + helper call.
  - `PipelineExit::CleanupSbaCheck` carries `since_ledger`.
- Unresolved:
  - `draw_one_card` still direct mutation.

## Slice: replaceable-draw (merged)

- Branch `phase5/replaceable-draw`.
- Scope: route CR 504.1 turn-based draw through replacement checking.
- Verify: 4 new tests, 0 failed.
- Key changes:
  - `commands::propose_draw` builds `ZoneChange` proposal; sets `drew_from_empty_library` on empty library.
  - `ReplacementResumeTarget::Draw { tail_events, since_ledger }`.
  - `Engine::enter_priority_via_replacement_check` helper factored from `AdvanceTurnStep` tail.
  - `Step::Draw` branch captures `since_ledger`, proposes, replaces, commits, then enters shared tail.
  - `Engine::draw_step` deleted.
- Unresolved:
  - `Game.Ops.DrawCards(n)` scripted multi-draw still direct mutation.

## Slice: etb-replacement-modifiers (merged)

- Branch `phase5/etb-replacement-modifiers`.
- Scope: CR 603.6d/614.1c "how a permanent enters the battlefield" replacement effects —
  `ReplacementPayload::{EntersTapped, EntersWithCounters}`, both self-intrinsic (a card's own
  "enters tapped"/"enters with N counters" text) and externally granted (Blood Moon-style).
- Verify: 9 new tests (Lua registration mechanics, `PlayLand`/`CastSpell`-resolution end-to-end,
  consumption-policy bookkeeping), 0 failed, full workspace suite (114 binaries) green.
- Key finding that reshaped the plan: the obvious approach (a new `CardDefinition` field) would
  have rippled all ~403 raw `CardDefinition { .. }` struct-literal sites in this codebase (no
  `Default` impl) — an order of magnitude past any prior slice's ripple. Dropped in favor of
  reusing the already-real `Game.CreateReplacementEffect` self-registration builder plus two new
  payload tags, which needed zero `CardDefinition`/`GameEventKind::ZoneChange` field changes.
- Key changes:
  - `ReplacementPayload::{EntersTapped, EntersWithCounters { kind: CounterKind, amount: i32 }}` —
    pair only with `EventFilter::WouldMoveZone { to: Some(Battlefield), .. }`
    (`validate_replacement_filter_payload_pairing` enforces the `to` restriction, unlike every
    other `WouldMoveZone` pairing so far).
  - `ReplacementPayload` lost `Copy` (`CounterKind::Custom(String)` isn't `Copy`) — `Clone` only
    now, same as `ReplacementEffect`'s other non-`Copy` fields already implied elsewhere.
  - `replacement::apply_enters_battlefield_modifiers(state, script_runtime, object, from) ->
    Vec<GameEventKind>` — a narrow sibling of `apply_replacements`, not routed through
    `apply_replacements_from`'s event-rewrite loop (neither payload has anything on
    `GameEventKind::ZoneChange` to rewrite). `EntersTapped` mutates the `GameObject` directly;
    `EntersWithCounters` only proposes a `CounterAdded` event for the caller to route through
    `CommitEvents` — mutating both ways double-applies (caught by the `cast_creature_resolution_
    self_registered_enters_with_counters` test initially failing 4 counters instead of 2, see
    "Mistakes caught" in `99-backlog.md`).
  - Deliberately skips CR 616.1's ordering-choice pause even for multiple matches: `EntersTapped`
    is idempotent, `EntersWithCounters` is additive, so unlike `ReplaceZoneDestination` no
    application order ever changes the result.
  - `Game.ReplacementPayloads.{EntersTapped, EntersWithCounters(kindName, amount)}` — reuses
    `parse_counter_kind_name` (PascalCase, e.g. `"PlusOnePlusOne"` — distinct from the scenario
    crate's own snake_case `parse_counter_kind`).
  - Wired into all three real battlefield-entry producers, each already running `run_card_script`
    before this point in the same call: `PlayLand` (via `advance()`'s `battlefield_entries` loop),
    `move_resolving_spell`'s `ResolveStackTop` caller, `submit_search_choice`'s
    `SearchDestination::Battlefield` arm.
- Registration-timing note (why no chicken-and-egg problem): `run_card_script` already runs after
  an object is inserted into `state.objects` but before the resulting `ZoneChange` event is
  checked/committed at all three producers — a self-registered effect is already in
  `state.replacement_effects` by the time `apply_enters_battlefield_modifiers` runs right after.
- Unresolved:
  - Dynamic/choice-based amounts (Bloodthirst, closed by `phase5/bloodthirst` below; Sunburst,
    closed by `phase5/sunburst` below; Devour, Amplify still unmodeled) — needs a `Script` payload
    variant for the ones that stay dynamic; only fixed printed `amount` (plus Bloodthirst's
    state-gated fixed `amount` and Sunburst's fully-derived kind/amount) is real.
  - CR 614.12a/b (choices required by an ETB modifier, payability ordering across simultaneous
    entries) — unmodeled.
  - Token creation (`Game.Ops.CreateToken`) — a 4th real to-Battlefield producer, not wired. Closed
    by `phase5/token-etb-modifiers` below.
  - General CR 616 propose/replace/commit for battlefield entry still doesn't exist beyond this
    narrow carve-out — `docs/lua/replacement-effects.md`'s existing reachability note (battlefield-entry absent
    from the real-`WouldMoveZone`-producer list) stays true; a `ReplaceZoneDestination`-style
    "can't enter, goes to the graveyard instead" effect is still unreachable.
  - CR 122.6a (which player places ETB counters) always defaults to the object's controller — no
    multiplayer override.

## Slice: bloodthirst (merged)

- Branch `phase5/bloodthirst`.
- Scope: CR 702.54a Bloodthirst N, narrowed out of the broader "dynamic ETB counts" backlog note
  above — the one of the four (Bloodthirst/Sunburst/Devour/Amplify) answerable from a pure state
  query rather than needing new plumbing (mana-color-spent tracking for Sunburst; a genuine mid-ETB
  player-choice pause, a new `ProcessState`, for Devour/Amplify).
- Verify: 6 new tests (`phase5_bloodthirst.rs` — unit-level `apply_enters_battlefield_modifiers`
  calls, Lua registration, and one real end-to-end cast-and-resolve after a real `DamageDealt`
  event), full workspace suite green.
- Key changes:
  - `ReplacementPayload::EntersWithCountersBloodthirst { kind: CounterKind, amount: i32 }` — same
    "propose a `CounterAdded` event, don't mutate directly" shape `EntersWithCounters` already
    established, gated on a condition instead of unconditional. Paired into
    `validate_replacement_filter_payload_pairing`'s `WouldMoveZone(_, Battlefield)` arm alongside
    its siblings, and into `apply_enters_battlefield_modifiers`'s own matching-filter and dispatch
    arms.
  - `replacement::opponent_damaged_this_turn(state, controller) -> bool` — scans `state.ledger`
    back to the most recent `GameEventKind::TurnStarted` (append-only, so its position is always
    the current turn's start; no `TurnStarted` yet scans from the ledger's beginning rather than
    treating the condition as unknowable), then checks for a `DamageDealt` whose `target` is
    `Target::Player` and isn't `controller` — damage to an opponent's *permanents* doesn't count,
    matching the keyword's own "an opponent was dealt damage" wording.
  - `Game.ReplacementPayloads.EntersWithCountersBloodthirst(kindName, amount)` — same construction
    shape as `EntersWithCounters`.
- Unresolved:
  - CR 702.54b's "Bloodthirst X" variant (amount = *total* damage dealt to opponents this turn, not
    a fixed gate) — reuses the identical ledger scan, named as a cheap follow-up, not built.
  - Sunburst — closed by `phase5/sunburst` below. Devour — closed by `phase5/devour` below. Amplify
    — still unmodeled, needs its own candidate-computation (hand cards sharing a creature type) but
    can reuse `phase5/devour`'s pause/resume shape.

## Slice: sunburst (merged)

- Branch `phase5/sunburst`.
- Scope: CR 702.44 Sunburst, the second of the four "dynamic ETB counts" items — unlike
  Bloodthirst's fixed-amount state-gate, Sunburst's amount is genuinely dynamic (colors of mana
  spent to cast it), but still fully deterministic, no player choice involved — the missing
  prerequisite was purely plumbing: `StackObject::costs_paid` (CR 118.13a's exact payment record)
  already existed, but nothing threaded it as far as the ETB step.
- Verify: 10 new tests (`phase5_sunburst.rs` — `cost::colors_spent` unit tests, unit-level
  `apply_enters_battlefield_modifiers` calls, Lua registration, and two real end-to-end
  cast-and-resolve tests with a real colored mana payment, one creature/one non-creature), full
  workspace suite green.
- Key changes:
  - `ReplacementPayload::EntersWithCountersSunburst` — no fields at all, unlike every other
    `EntersWithCounters*` variant. Both `kind` and `amount` are derived at apply time instead:
    `amount` is the popcount of a new `colors_spent: ColorSet` parameter threaded onto
    `apply_enters_battlefield_modifiers` itself; `kind` is `PlusOnePlusOne` if the entering object's
    own *printed* `CardDefinition.card_types` include Creature, `Charge` otherwise — CR 702.44a's
    "ignoring any type-changing effects that would affect it" is explicit that this is the printed
    type, not `EffectiveCharacteristics::card_types` (layer 4), the one respect this isn't shaped
    like every other characteristic-sensitive check in this engine.
  - `cost::colors_spent(payments: &[CostPayment]) -> ColorSet` — new helper, sums the distinct
    colors across every `CostPayment::Mana` entry's `units` (a `SnowColored` unit counts toward its
    color same as a plain one; non-`Mana` payments contribute nothing).
  - `apply_enters_battlefield_modifiers` gained a `colors_spent: ColorSet` parameter. Only the
    resolve-spell-from-stack call site (`ResolveStackTop`) can supply a real value — `top` (the
    captured `StackObject`) still has its own `costs_paid` at that point, read via
    `cost::colors_spent(&top.costs_paid)`. `PlayLand`/`Search`'s battlefield-entry call sites always
    pass `ColorSet::empty()`, which is exactly correct per CR 702.44b (no cast, no colors, no
    counters) rather than a placeholder needing a follow-up.
  - `Game.ReplacementPayloads.EntersWithCountersSunburst` — a bare value (like `EntersTapped`), not
    a constructor function, since there's nothing to parametrize.
  - Paired into `validate_replacement_filter_payload_pairing`'s `WouldMoveZone(_, Battlefield)` arm
    alongside its siblings.
- Unresolved: CR 702.44c's "Sunburst used to set a variable number for another ability" (e.g.
  "Modular—Sunburst") — no such cross-keyword reference plumbing exists, not attempted. Devour,
  Amplify — Devour closed by `phase5/devour` below; Amplify still needs its own slice.

## Slice: devour (merged)

- Branch `phase5/devour`.
- Scope: CR 702.82a Devour N — the last of the four "dynamic ETB counts" items, and the first that
  genuinely needs a mid-ETB player choice (*which* creatures, if any, to sacrifice), unlike
  Bloodthirst/Sunburst's fully deterministic amounts. Bigger than any other slice this session —
  touched `ResolveStackTop`'s own resolution tail and `continue_scripted_zone_move`'s shared
  sacrifice/destroy/exile machinery, not just an additive new match arm.
- Verify: 11 new tests (`phase5_devour.rs` — `pending_devour_choice` unit tests, Lua registration,
  and five real end-to-end cast-and-resolve tests: full sacrifice, sacrifice-nothing, self-devour,
  and both `DevourChoiceError` rejections), full workspace suite green.
- Key finding that reshaped the plan: `ResolveStackTop` defers *everything* about a resolution
  (including the entering permanent's own `ZoneChange`) into one `ReplacementCheck` pass at the very
  end — nothing is committed to the ledger yet at the point Devour's choice needs to happen. So this
  couldn't be a narrow addition to `apply_enters_battlefield_modifiers` (which only ever mutates
  state/proposes events synchronously and has no way to pause the engine) — the pause has to live
  one level up, in `ResolveStackTop` itself, and has to carry `events_so_far` forward through the
  pause so those events aren't lost.
- Key changes:
  - `ReplacementPayload::EntersWithCountersDevour { per_creature: i32 }` — deliberately never
    applied by `apply_enters_battlefield_modifiers` at all (unlike every sibling payload); it isn't
    even in that function's own matching filter.
  - `replacement::pending_devour_choice(state, script_runtime, object, from, controller) ->
    Option<(i32, Vec<ObjectId>)>` — checked *before* `apply_enters_battlefield_modifiers` runs, only
    at the one call site that can actually pause (`ResolveStackTop`'s resolving-spell path).
    `PlayLand`/`Search` never call this at all — a land can never have Devour, and a Search-fetched
    Devour creature not offering the choice is a named, narrow, deliberately unclosed gap (no card in
    this engine's test corpus fetches one — same "no real producer reaches this" posture
    `aura-attach-choice`'s dormant `AwaitingAttachChoice` already established). Candidates are every
    creature the entrant's own controller controls (CR 701.17a: sacrificing is always the
    controller's own choice), the entrant itself included (real Magic allows devouring the source;
    untested as its own scenario here beyond confirming it's offered and legal to choose).
  - `ProcessState`/`StepStatus::AwaitingDevourChoice` — same "player must answer before this
    resolution can finish" shape every other choice-pause here uses; carries `stack_id`/
    `events_so_far` so `submit_devour_choice` can resume the exact same resolution.
  - `Engine::submit_devour_choice(sacrificed) -> Result<(), DevourChoiceError>` — validates, then
    performs the sacrifices via `continue_scripted_zone_move` (reused as-is: CR 616.1's ordering
    choice and dies-triggers all apply for free, same as a real `AbilityEffect::Sacrifice`), folding
    `events_so_far` plus a `CounterAdded` (if `sacrificed` was nonempty) into that function's *new*
    `trailing: Vec<GameEventKind>` parameter.
  - `continue_scripted_zone_move`/`ReplacementResumeTarget::ScriptedZoneMove` both gained that
    `trailing` field — events owed once the zone-move queue empties, threaded through even a nested
    CR 616.1 pause raised mid-sacrifice, not just the non-pausing path. Every pre-existing caller
    (`Destroy`/`Exile`/the plain `AbilityEffect::Sacrifice` path) passes `Vec::new()`, unaffected.
- Unresolved: CR 702.82c's "Devour [quality] N" variant (sacrifice permanents of [quality] rather
  than just creatures) — not modeled, `per_creature` always means "per creature." The
  nested-pause interaction (a Devour sacrifice where two simultaneously-dying creatures also raise
  their own competing CR 616.1 ordering choice, exercising `trailing` surviving *that* pause
  specifically) isn't separately tested — the code path is structurally identical to the
  pre-existing `NeedsOrderingChoice` arm (just carrying one more field), and the non-paused majority
  path is covered directly. Amplify — closed by `phase5/amplify` below.

## Slice: amplify (merged)

- Branch `phase5/amplify`.
- Scope: CR 702.38a Amplify N, the last of the four "dynamic ETB counts" items — reuses Devour's
  pause/resume shape (own `ProcessState`/`StepStatus` variant, checked at the same `ResolveStackTop`
  call site before `apply_enters_battlefield_modifiers` runs) but with a different candidate source
  and no zone-move consequence.
- Verify: 10 new tests (`phase5_amplify.rs` — `pending_amplify_choice` unit tests, Lua registration,
  and five real end-to-end cast-and-resolve tests: full reveal, reveal-nothing, and both
  `AmplifyChoiceError` rejections), full workspace suite green.
- Key changes:
  - `ReplacementPayload::EntersWithCountersAmplify { per_card: i32 }` — same "never applied by
    `apply_enters_battlefield_modifiers`" shape as `EntersWithCountersDevour`.
  - `replacement::pending_amplify_choice(state, script_runtime, object, from, controller) ->
    Option<(i32, Vec<ObjectId>)>` — candidates are cards in `controller`'s hand whose
    `EffectiveCharacteristics::subtypes` overlap `object`'s own. "Shares a creature type" is
    approximated as raw subtype overlap — this engine has no separate "creature type" category
    distinct from land/artifact/etc. subtypes, same imprecision `landwalk_subtypes`/
    `bands_with_other_subtypes` already accept for their own quality checks.
  - `ProcessState`/`StepStatus::AwaitingAmplifyChoice` — same "player must answer before this
    resolution can finish" shape `AwaitingDevourChoice` uses.
  - `Engine::submit_amplify_choice(revealed) -> Result<(), AmplifyChoiceError>` — **the one real
    divergence from Devour**: revealing a hand card doesn't move it anywhere or trigger anything, so
    the resume needs no `continue_scripted_zone_move` at all. It folds `events_so_far` plus a
    `CounterAdded` (if `revealed` wasn't empty) straight into the same `StackObjectResolved`/
    `ReplacementCheck` tail every other synchronous-only resolution already ends on — simpler than
    Devour's own resume, not just a copy of it.
  - `Game.ReplacementPayloads.EntersWithCountersAmplify(perCard)` — same construction shape as
    `EntersWithCountersDevour`, paired into `validate_replacement_filter_payload_pairing`'s
    `WouldMoveZone(_, Battlefield)` arm alongside its siblings.
- Unresolved: CR 702.82c's Devour sibling gap aside, CR 702.38a's "can't reveal this card or any
  other cards entering the battlefield at the same time as this card" is unmodeled — no producer in
  this engine ever enters multiple permanents from hand simultaneously, so the exclusion is
  unreachable, not built. CR 702.38b (multiple separate Amplify instances on one creature "work
  separately") is unmodeled too — `pending_amplify_choice` matches only the first registered
  instance via `find_map`, same single-match shape `pending_devour_choice` already has.

## Slice: protection-as-replacement (merged)

- Branch `phase5/protection-as-replacement`.
- Scope: CR 702.16e/615.1/616.1 — protection's damage-prevention aspect now genuinely participates
  in the CR 616 ordering-choice decision instead of applying unconditionally ahead of the loop.
- Verify: 5 new tests (ordering-choice pause offered, both resolution orders' divergent outcomes,
  the common-case regression, the documented sentinel-collision limitation), 0 failed, full
  workspace suite (115 binaries) green — every existing `phase5_protection_from_*.rs` test passed
  unchanged, confirming the common case (protection alone) is behavior-preserving.
- Research finding before implementing: the gap was real (CR 616.1e gives the affected player a
  genuine choice here) but completely unexercised — no existing test combined protection with a
  competing registered replacement effect on the same event. Indestructible, despite `apply_
  protection`'s own old doc comment drawing the "same stance" comparison, turned out to be a
  structurally different shape (no event is ever proposed for an indestructible creature's lethal
  damage, so there's nothing for a second effect to compete over) — explicitly *not* touched by
  this slice.
- Key changes:
  - `apply_protection` (old: unconditional mutate-and-return, called first every loop iteration)
    replaced by `protection_prevents(state, script_runtime, event: &GameEventKind) -> Option<ObjectId>`
    — a pure predicate returning the protected object's own id if protection would zero the event's
    damage, folded into `apply_replacements_from`'s own `matching` candidate list alongside real
    `ReplacementEffect::source`s instead of being applied ahead of it.
  - `apply_one_replacement_or_protection(state, chosen, event)` — new dispatch (`pub(crate)`,
    exported), shared by the loop's own single-match branch and `Engine::submit_replacement_choice`'s
    resume path (previously called `apply_one_replacement` directly): if `chosen` names a real
    registered effect, applies it as before; otherwise treats it as the protection sentinel and
    zeroes the damage.
  - `matching` dedups the protection sentinel against an already-present entry with the same id
    before pushing — without this, a permanent with *both* innate protection and its own registered
    replacement effect on itself would get a spurious duplicate-id 2-candidate pause instead of
    correctly collapsing to the one real match (caught by a failing test, see below).
  - `ReplacementOutcome::NeedsOrderingChoice`'s own doc comment updated: `candidates` can now name
    either a real effect source or the protection sentinel.
- Known, accepted limitation (documented in code, not silently papered over): the protected
  object's own id doubles as the protection sentinel. If that same object also has its own
  registered replacement effect (source == itself) matching the same event, the two collide — the
  registered effect always wins the dispatch, so protection can never independently be "the one
  chosen" for that specific id in that specific case. Locked in by its own test rather than left
  unverified. A real `ReplacementCandidate` enum (registered-vs-protection) would close this for
  real; not built this slice, matching this codebase's preference for narrow real fixes over new
  general machinery (`is_destroy`'s own call-parameter precedent).
- Bug caught mid-slice (see `99-backlog.md`'s "Mistakes caught"): the first version of the dedup
  fix was missing — a permanent with both protection and a self-registered effect produced a bogus
  `matching = [id, id]` (two candidates that were actually the same object), triggering a spurious
  ordering pause instead of the intended single-match collapse. Caught by `a_permanent_with_both_
  protection_and_its_own_registered_effect_only_ever_resolves_the_registered_one` failing before the
  fix.
- Unresolved:
  - The sentinel-collision limitation above.
  - `apply_token_zone_lock` (CR 111.8) has the identical hardcoded-ahead-of-the-loop shape and the
    same theoretical CR 616.1 exposure, but stays untouched — already flagged elsewhere as
    unreachable via real gameplay, so the same fix isn't worth it there yet.
  - Targeting/blocking enforcement (`commands::target_is_legal`, `DeclareBlockers`) are correctly
    *not* touched — CR 702.16c/d are hard restrictions, not replacement effects, so CR 616.1 never
    applied to them in the first place.

## Slice: protection-replacement-collision-fix (merged)

- Branch `phase5/protection-replacement-collision-fix`.
- Scope: fixes the sentinel-collision limitation `protection-as-replacement` documented and locked
  in with its own test — exactly the fix that slice's own doc comment already named as the way to
  close it for real.
- Verify: `phase5_protection_as_replacement.rs`'s old "locks in the bug" test replaced with 2 tests
  proving the fix (a real 2-way pause now occurs, and each choice resolves cleanly), full workspace
  suite green.
- Key changes:
  - `replacement::ReplacementCandidate { Registered(ObjectId), Protection(ObjectId) }` — a new enum
    replacing the bare `ObjectId` both kinds of CR 616.1 candidate used to share. Threaded through
    `ReplacementOutcome::NeedsOrderingChoice`'s `candidates`/`excluded`, `ProcessState`/
    `StepStatus::AwaitingReplacementChoice`, and `Engine::submit_replacement_choice`'s public
    signature.
  - `apply_replacements_from`'s candidate-list builder no longer dedups a `Protection(id)` against
    an already-present `Registered(id)` — they're distinct enum values now, so both are genuinely
    offered when they both match the same event, closing the exact gap the old dedup (deliberately
    added by `protection-as-replacement`, for the wrong reason — see that slice's own "bug caught
    mid-slice" note) opened.
  - `apply_one_replacement_or_protection`'s dispatch is now a direct `match` on the
    `ReplacementCandidate` variant instead of an `ObjectId` lookup that assumed "found a registered
    effect with this id" was authoritative — the actual bug, since that lookup was always true in
    the collision case regardless of which candidate the player meant to choose.
  - Ripple was broad-but-shallow: ~20 sites across `engine.rs`/`sba.rs` that just forward
    `candidates`/`excluded` through unrelated pipeline stages needed no conceptual changes, only a
    type that Rust's own compiler forced through; 9 test files' `submit_replacement_choice(id)` calls
    needed wrapping in `ReplacementCandidate::Registered(id)`.
- Unresolved: none — this was the one limitation the prior slice flagged as open for this feature.

## Slice: token-etb-modifiers (merged)

- Branch `phase5/token-etb-modifiers`.
- Scope: wire `AbilityEffect::CreateToken`'s dispatch (`ResolveStackTop`) into
  `replacement::apply_enters_battlefield_modifiers` — the 4th of the four real to-Battlefield
  producers (`PlayLand`, the resolving-spell path, `Search`), closing a real, previously-silent gap:
  `EntersTapped`/`EntersWithCounters`/Bloodthirst/Sunburst all already worked for every other
  permanent-entry shape but were never applied to a token.
- Verify: 4 new tests (`phase5_token_etb_modifiers.rs`), full workspace suite green.
- Key finding: a plain `Named`/`Creature` token has no script of its own (`run_card_script` is only
  ever called for `TokenSpec::CopyOf`), so this gap is exercised two real ways — an externally
  registered, non-`self_only` replacement effect (e.g. "tokens you control enter tapped," filtered
  by controller rather than by which object is entering) already sitting in
  `state.replacement_effects`, or a `CopyOf` token whose copied source had a self-registered
  modifier that `run_card_script` re-registers under the new token's id.
- Key changes:
  - One new call, `apply_enters_battlefield_modifiers(&mut self.state, &self.script_runtime,
    token_id, ZoneKind::Stack, ColorSet::empty())`, per created token — placed after
    `register_token_abilities`/the conditional `run_card_script` (CopyOf) so any self-registered
    effect is already in `state.replacement_effects` by the time it's checked, same ordering the
    other three call sites already use. `from: ZoneKind::Stack` matches the `ZoneChange` event this
    call site already logs for the token; `colors_spent: ColorSet::empty()` matches the "not a cast"
    convention `PlayLand`/`Search` already use for Sunburst.
  - The returned `counter_events` are extended onto `events` alongside each token's own `ZoneChange`.
- Unresolved: Devour/Amplify stay unwired on tokens — both need a genuine mid-ETB player pause this
  function structurally can't raise (it only ever mutates state/proposes events synchronously), same
  narrow "no real producer reaches this yet" posture `pending_devour_choice`'s own doc comment
  already takes for `Search`. CR 111.7 cessation (tokens vanishing on leaving the battlefield) is
  unrelated and stays its own pre-existing TODO.

## Slice: etb-zone-redirect (merged)

- Branch `phase5/etb-zone-redirect`.
- Scope: CR 614's general "instead" framing, wired to battlefield entry for the first time —
  `ReplacementPayload::ReplaceZoneDestination` and its `WouldMoveZone(_, Battlefield)` pairing already
  existed and were already validated as legal at `:Register()` time, but nothing ever checked for one
  at a real battlefield-entry producer. `ResolveStackTop`'s resolving-spell path is a permanent
  eagerly deciding `destination = Battlefield` and placing itself there before a `ZoneChange` event is
  even constructed (purely for logging) — a registered redirect had no chance to fire before this.
- Key finding that shaped the plan: `sba::resolve_deaths` already solves the identical problem for the
  graveyard side — propose the `ZoneChange` through `replacement::apply_replacements` *before* moving
  anything, commit to whatever destination the (possibly rewritten) event says. This slice ports that
  exact shape to the one battlefield-entry producer touched, rather than inventing a new mechanism.
- Verify: 4 new tests (`phase5_etb_zone_redirect.rs`), full workspace suite green.
- Key changes:
  - `ResolveStackTop`'s permanent-destination branch: before calling `move_resolving_spell`, a
    would-be `Battlefield` destination is proposed as a `WouldMoveZone` candidate and run through
    `replacement::apply_replacements`. `Resolved`'s own (possibly rewritten) `to` becomes the real
    destination; `move_resolving_spell` is unchanged and already handles an arbitrary destination
    correctly (its own `destination_owner`/`run_script` logic was already generic).
  - A redirected permanent never actually enters the battlefield — `run_script` is computed *after*
    redirection (`destination == Battlefield`), so no self-registration, no summoning-sickness
    bookkeeping, none of the "entering" semantics downstream (Aura auto-attach, Devour/Amplify/
    `EntersAsCopy` pauses, `apply_enters_battlefield_modifiers`) ever run for it either, since all of
    that is already gated on the event's own `to: Battlefield` shape.
- Deliberately narrow, both named in code comments:
  - Only an *externally* granted redirect effect can ever fire — the entering permanent's own
    self-registered effects haven't been registered yet at this checkpoint (`run_card_script` only
    runs inside `move_resolving_spell`, once the real destination is already decided), so a card's own
    "as this would enter, redirect it instead" text isn't reachable via this path.
  - CR 616.1's ordering choice for two *simultaneous* redirects targeting the same entering permanent
    is narrowed to "no redirect happens" rather than a real player-choice pause — genuinely rare (two
    different sources each independently hating on the same entry at once), same posture this
    codebase already takes for comparably rare nested-pause interactions. A real fix would need its
    own `ReplacementResumeTarget` variant plus extracting the whole Spell-resolution tail into a
    shared continuation (nothing has been placed yet at this point for `submit_replacement_choice` to
    resume into) — not built, named as a TODO in the code.
  - `PlayLand`/`Search`'s own battlefield destinations don't get this check either, same narrowing
    every prior mid-ETB slice (Devour/Amplify/`EntersAsCopy`) already took for the same reason: highest
    real value at the resolving-spell call site, a redirected land or search-fetch is a much rarer gap
    worth naming rather than building blind.

## Slice: umbra-armor (merged)

- Branch `phase5/umbra-armor`.
- Scope: CR 702.89a (Umbra armor) — "if enchanted permanent would be destroyed, instead remove all
  damage marked on it and destroy this Aura." A replacement effect's own "enchanted permanent"
  reference — the sibling `phase5/enchanted-reference` (CR 303.4m for *continuous* effects,
  `AffectedFilter::EnchantedOnly`) explicitly named as out of scope: `ReplacementEffect` has no
  `affected` field of its own at all — `EffectFilter` is nested two levels deep
  (`ReplacementSpec::Primitive.filter → EventFilter::WouldDie.affected → EffectFilter`), a different,
  non-`AffectedFilter` type from the continuous-effect path.
- Real correction made while scoping: the recommended card-shape ("remove a shield counter") was
  wrong — that's CR 122.1c, an unrelated generic counter mechanic, not Umbra Armor at all. Real CR
  702.89a is two distinct effects on two different objects (damage-clear on the enchanted permanent,
  destroy the Aura itself), not a counter decrement — this changed the slice from "extend a filter" to
  "extend a filter + model a genuine two-object replacement outcome."
- Key changes:
  - `EventFilter::WouldDieEnchanted` — fieldless, sibling to `EventFilter::WouldDie { affected:
    EffectFilter }` rather than a new field on it (23 existing `WouldDie` construction sites, no
    `Default` impl — same ripple class `AffectedFilter::EnchantedOnly` itself already avoided).
    Resolved directly in `ReplacementEffect::matches`: `is_destroy && state.object(self.source)
    .and_then(|o| o.attached_to) == Some(*object)` — mirrors `AffectedFilter::EnchantedOnly`'s own
    `source.attached_to == Some(object.object_id)` check exactly, just via a `GameState` lookup
    (`ReplacementEffect::matches`'s calling convention has ids, not already-borrowed `&GameObject`s).
  - `ReplacementPayload::UmbraArmor` — rewrites the matched event into a new `GameEventKind::
    UmbraArmorConsumed { enchanted, aura }`, same "nothing to rewrite the *moving* object into, the
    permanent doesn't change zones" shape `Regenerate`/`Regenerated` already established.
  - `sba::commit_death_or_regeneration` widened from returning one `(GameEvent, Vec<RegisteredAbility>)`
    to `Vec<(GameEvent, Vec<RegisteredAbility>)>` — the real novel piece of scope. `UmbraArmorConsumed`
    resolves to *two* real committed events (the enchanted permanent's own log entry, plus the Aura's
    own genuine `ZoneChange`, committed via a direct nested `commit_death` call, CR 616 replacement
    recursion deliberately skipped — same narrow stance `submit_commander_return_choice`'s own doc
    comment already takes for an analogous situation). Checked the ripple before widening: only 3 call
    sites (`sba::resolve_deaths`, `Engine`'s SBA-resume branch, `commit_scripted_zone_move`'s `Destroy`
    arm), each already just looping and appending — a small, deliberate widening, not the wider ripple
    class this backlog usually flags before doing this.
  - `Game.EventFilters.WouldDieEnchanted`/`Game.ReplacementPayloads.UmbraArmor` — new `EventFilterKind`
    variant (fieldless, `ReplacementEffectBuilder::Register` ignores `:SetAffected`'s `this.affected`
    entirely for this shape, same "no combining with a controller/card_types filter" stance the Rust
    type itself takes) and `ReplacementPayloadTag`, wired through `validate_replacement_filter_payload_
    pairing` the same way every other filter/payload pairing already is.
- Verify: 4 new tests (`phase5_umbra_armor.rs`) — direct `ReplacementEffect::matches` unit checks
  (matches the enchanted creature's own destroy, not a different creature's, not a non-destroy death),
  an end-to-end `check_state_based_actions` run (creature survives undamaged, Aura genuinely destroyed
  under a new id), a no-Umbra-Armor control case, and a `Game.CreateReplacementEffect` self-registration
  test. Full workspace suite green.
- Unresolved: CR 702.89b's "multiple Umbra Armors treated separately" isn't specifically tested (each
  instance is already its own independently-matched/consumed `ReplacementEffect`, no shared-state bug
  introduced, just unexercised); no general "replacement effect referencing the enchanted permanent"
  beyond this one concrete shape.
