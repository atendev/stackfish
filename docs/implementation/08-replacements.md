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
  - Dynamic/choice-based amounts (Bloodthirst, Sunburst, Devour, Amplify) — needs a `Script`
    payload variant, only fixed printed `amount` is real.
  - CR 614.12a/b (choices required by an ETB modifier, payability ordering across simultaneous
    entries) — unmodeled.
  - Token creation (`Game.Ops.CreateToken`) — a 4th real to-Battlefield producer, not wired.
  - General CR 616 propose/replace/commit for battlefield entry still doesn't exist beyond this
    narrow carve-out — `docs/lua_scripts.md`'s existing reachability note (battlefield-entry absent
    from the real-`WouldMoveZone`-producer list) stays true; a `ReplaceZoneDestination`-style
    "can't enter, goes to the graveyard instead" effect is still unreachable.
  - CR 122.6a (which player places ETB counters) always defaults to the object's controller — no
    multiplayer override.
