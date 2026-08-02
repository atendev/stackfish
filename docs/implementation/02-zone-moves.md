# 02-zone-moves

## Fix: `ZoneChange` LKI (CR 603.10)

- Added `GameEventKind::ZoneChange.last_known_info: Option<GameObject>`.
- Populated by producers as snapshot before move; used by triggers for leaves-battlefield conditions.
- `GameObject` and its field chain gained `Clone`/`Debug`; `CounterKind` gained `PartialEq`/`Eq` as side effect.

## Fix: object id minting (CR 400.7)

- Added `GameState.next_object_id: u64` + `allocate_object_id()`.
- Every zone change now mints a new `ObjectId`, sets `ObjectLineage.previous = Some(old_id)`, removes old id from `state.objects`.
- `last_known_info` carries the old object snapshot; `ZoneChange.object` carries the new id.

## Slice: play-land (merged)

- Branch `phase5/play-land`.
- Scope: `PlayerActionKind::PlayLand` (CR 305.1) as first real zone-move consumer.
- Verify: 9 new tests + 1 unit test, all green.
- Key changes:
  - `Zone::remove`/`push_bottom`, `GameState::object`/`object_mut`/`zone`/`zone_mut` filled.
  - `PlayLand` validates land type, hand control, snapshots LKI, moves hand -> battlefield, mints new id.
  - `commands::is_sorcery_speed(state, player)` predicate (priority + actor's turn + `Phase::Main` + empty stack).
  - `GameState.lands_played_this_turn: HashMap<PlayerId, u32>`; cleared at Cleanup -> next turn.
- Deviations:
  - `is_sorcery_speed`/`lands_played_this_turn` added mid-slice after CR review.
- Unresolved:
  - `is_sorcery_speed` ignores continuous-effect timing permissions.
  - Land-limit hardcoded to 1; ignores "additional land" effects.

## Slice: cast-spell (merged)

- Branch `phase5/cast-spell`.
- Scope: `CastSpell` (CR 601) with cost stubbed free; resolution moves permanent spells to battlefield, instant/sorcery to graveyard (CR 608.2m).
- Verify: 9 new tests, 30 total, 0 failed.
- Key changes:
  - `StackObject::new` real body; signature grew `chosen_modes`/`targets`/`x_value`.
  - `CastSpell` rejects lands, checks timing (instant vs sorcery speed), mints new id hand -> stack, pushes real `StackObject`.
  - `ResolveStackTop` real for `Spell`: moves stack entry to battlefield or owner's graveyard, mints new id.
- Unresolved:
  - Cost payment free.
  - Mode/target legality unvalidated.
  - Flash/timing permissions beyond instant split.
  - `operation: ScriptHandle(0)` stub.
  - No SBA/fizzle on illegal targets.

## Slice: cast-spell-payment (merged)

- Branch `phase5/cast-spell-payment`.
- Scope: real cost determination + payment (CR 601.2f-h) for `CastSpell`.
- Verify: 8 new tests, 38 total, 0 failed.
- Key changes:
  - `ProcessState::AwaitingPayment { stack_id, cost }`, `StepStatus::AwaitingPayment`, `Engine::submit_payment(plan)`.
  - `CostSpec::mana_only` and `PaymentPlan::validate(spec, pool)` real for colored/generic only.
  - `cost::spend(spec, pool)` deducts literally.
  - `GameEventKind::CostPaid { stack_id, payer }` added.
  - `GameState::player`/`player_mut` filled.
- Deviations:
  - `CommandOutcome` gained `requires_payment: Option<PendingPaymentRequest>`; `PendingPaymentRequest` gained `source: ObjectId`.
  - `Engine` gained `pending_payment` field.
- Unresolved:
  - Additional/alternative costs, hybrid/Phyrexian/snow/X, cost modifiers.
  - Mana abilities not modeled.
  - CR 601.6 (failed payment) leaves engine stuck; unwind added later.

## Slice: mana-payment-choice (merged)

- Branch `phase5/mana-payment-choice`.
- Scope: CR 118.13a — payer decides which floating mana pays colored vs generic.
- Verify: 40 tests, 0 failed (38 prior + 2 new).
- Key changes:
  - `ManaUnit { Colored(Color), Colorless }`.
  - `CostPayment::Mana(Vec<ManaUnit>)` replaces `ManaCost` echo.
  - `PaymentPlan::validate` rewritten: colored pips exact, extras sum to generic exactly.
  - `cost::spend` decrements pool by literal units.
  - Scenario `ManaPaymentSpec` redesigned to flat `units: Vec<String>`.
- Unresolved: mana abilities, hybrid/Phyrexian/snow/X, CR 601.6 unwind.

## Slice: beginning-phase (merged)

- Branch `phase5/beginning-phase`.
- Scope: CR 502-504 — Untap, Upkeep, Draw.
- Verify: 5 new tests, all prior suites green.
- Key changes:
  - `GameEventKind::Untapped { object }`.
  - `engine::untap_step` untaps active player's tapped battlefield permanents (sorted by `ObjectId` for determinism).
  - `engine::draw_step`: CR 103.8a 2-player starting player skips first draw; empty library silent no-op.
  - `AdvanceTurnStep` now emits `PhaseStarted` only when phase actually changes.
  - `Step::Untap` auto-chains through without opening priority (CR 502.4).
- Unresolved: phasing, day/night, upkeep triggers (script bridge), empty-library loss SBA, multiplayer draw-skip, "doesn't untap" effects.

## Slice: ending-phase (merged)

- Branch `phase5/ending-phase`.
- Scope: CR 511-514 — End of Combat, End Step, Cleanup.
- Verify: 9 new tests, all prior green.
- Key changes:
  - `Zone::len`/`contains` filled.
  - `ProcessState::AwaitingDiscard { player, excess }`, `StepStatus::AwaitingDiscard`, `Engine::submit_discard`.
  - `ProcessState::CleanupSbaCheck`.
  - `begin_cleanup_step`: drains `UntilEndOfTurn` effects, checks hand size, pauses or advances.
  - `ActionSpec::SubmitDiscard`, `AssertionSpec::HandSize` in scenario crate.
- Unresolved:
  - `ReplacementEffect` duration not drained.
  - Second cleanup after SBA/triggers not modeled.
  - Max hand size hardcoded 7 (later fixed by `HandSizeModifier`).

## Fix: `state.step` stale in Main phase

- Added `Step::PreCombatMain`/`Step::PostCombatMain`; removed `GameState.postcombat_main`.
- Scenario `parse_step` and `docs/scenario_format.md` updated.

## Slice: escape-alternative-cost (merged)

- Branch `phase5/escape-alternative-cost`.
- Scope: CR 702.138 Escape ("cast from your graveyard by paying [cost]," where `[cost]` typically
  includes "exile N other cards from your graveyard"). Requirements report given in chat before
  implementation. Note: `AlternativeCost::Replacement` (CR 601.2b flashback/escape-style
  alternative costs) was already fully real and generic by this point — built in an earlier,
  undocumented slice (no prior entry exists in this file; `phase5_flashback_alternative_costs.rs`
  is the pre-existing proof) — this slice only needed the one missing cost component.
- Verify: 6 new tests (`phase5_escape.rs`) — happy path (exiles exactly `count`, leaves the rest of
  the graveyard alone), 3 rejection cases (too few exile targets, duplicate id, a non-graveyard
  id), the pre-existing "wrong from_zone" rejection shape reused from Flashback, and one Lua
  self-registration/decode test. Full workspace suite (173 test binaries) green.
- Key finding that confirmed the scope stayed small: read `commands::cast::execute_cast_spell`
  directly rather than assuming — a cast spell moves to the stack (CR 601.2a) *before* its cost is
  paid, so by the time an exile-cost payment is validated, the escaping card has already left the
  graveyard. No self-exclusion logic (the kind Cycling's `DiscardSelf` needed) was needed here —
  structurally impossible for the card being cast to appear among its own exile candidates.
- Key changes:
  - `AdditionalCost::ExileFromGraveyard { filter: FilterSpec, count: u32 }` / `CostPayment::
    Exile(ObjectId)` — the same N-distinct-payments shape `TapPermanents`/`CostPayment::Tap`
    already established, checked against `ZoneKind::Graveyard` instead of `Battlefield`.
    `PaymentPlan::validate` gained one new arm, mirroring `TapPermanents`'s loop exactly.
  - `commands::zone_ops::propose_zone_move` — `to: ZoneKind` was a hardcoded `Graveyard` before
    this slice (every prior caller — Sacrifice/Discard cost payments, CR 514.1 cleanup discard —
    happened to end there); widened to a real parameter so an `Exile` payment can propose a
    `Graveyard -> Exile` move through the same replacement-checked pipeline. `collect_cost_
    zone_moves` widened from `Vec<(ObjectId, ZoneKind)>` (id, from) to `Vec<(ObjectId, ZoneKind,
    ZoneKind)>` (id, from, to) accordingly; `PendingZoneMove` gained a `to` field. Two existing call
    sites (`engine/payment.rs`, `engine/turn.rs::continue_discard_moves`) updated mechanically —
    both keep passing `Graveyard` as before, no behavior change for Sacrifice/Discard/cleanup
    discard.
  - `Game.Costs.ExileFromGraveyard(filter, count)` — Lua binding, same shape as `TapPermanents`.
  - No change needed to the cast/alternative-cost pipeline itself (`commands/cast.rs`) — already
    fully generic over any `AlternativeCost::Replacement`, proven by Flashback.
- Deviations: none from the requirements report's design.
- Unresolved, named not silently dropped:
  - CR 702.138c/d ("escapes with [counters/ability]," linked to "if this permanent escaped") — no
    provenance tracking exists anywhere in this engine for "was this permanent cast via escape";
    would need a new linked-ability mechanism, a separate and bigger piece.
  - No Lua builder for `AlternativeCost::Replacement` itself (the escape/flashback declaration) —
    same scope Flashback's own (undocumented) slice already established; only `Game.Costs.
    ExileFromGraveyard` (the cost component) has Lua exposure.
