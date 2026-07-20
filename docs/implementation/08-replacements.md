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
  - ETB-modifier replacements need new event payload.
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
