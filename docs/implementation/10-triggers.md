# 10-triggers

## Slice: damage-dealt-triggers (merged)

- Branch `phase5/damage-dealt-triggers`.
- Scope: CR 603 "whenever this deals/is dealt damage" conditions.
- Verify: 6 unit + 3 integration tests, 0 failed.
- Key changes:
  - `ConditionPrimitive::{DealsDamage, IsDealtDamage}`.
  - `Game.Conditions.{DealsDamage, IsDealtDamage}`.
- Unresolved:
  - Phase/step-boundary triggers (blocked by `AdvanceTurnStep` not running trigger collection).

## Slice: upkeep-triggers (merged)

- Branch `phase5/upkeep-triggers`.
- Scope: route normal step/phase transitions through `ReplacementCheck`→`CommitEvents`→`SbaLoop`→`CollectTriggers` so phase-boundary triggers fire.
- Verify: 8 unit + 3 integration tests, 0 failed.
- Key changes:
  - `AdvanceTurnStep` normal transitions set `pipeline_context`/`pipeline_exit` and route through `ReplacementCheck`.
  - `ConditionPrimitive::{BeginningOfYourUpkeep, BeginningOfEachUpkeep}`.
  - `ConditionPrimitive::matches` gained `controller`/`active_player` params.
  - `Untap` and skipped Draw step left on direct `log_events` path.
- Unresolved:
  - Cleanup trigger collection.

## Fix: Cleanup trigger collection

- `ProcessState::CleanupSbaCheck` routes through `CollectTriggers`/`PutTriggersApnap` via `PipelineExit::CleanupCheck { since_ledger }`.
- `finish_cleanup_sba_check` stashes departed abilities and enters trigger collection.
- Fixed `CleanupCheck` priority-vs-advance to also check `!stack.is_empty()`.

## Slice: dies-triggers (merged)

- Branch `phase5/dies-triggers`.
- Scope: CR 603.10 LKI fallback for "when this dies"/"leaves the battlefield".
- Verify: 6 unit + 5 integration tests, 333 total, 0 failed.
- Key changes:
  - `retire_object` returns `Vec<RegisteredAbility>` of the departed object's triggered abilities.
  - `commit_death` returns `(GameEvent, Vec<RegisteredAbility>)`.
  - `Engine.departed_triggered_abilities` transient stash; populated by SBA loop, consumed by `collect_triggers`/`PutTriggersApnap`, cleared at pipeline end.
  - `ConditionPrimitive::{Dies, LeavesBattlefield}` match by `last_known_info.object_id`.
  - `triggers::collect_triggers` second loop over departed abilities.
- Unresolved:
  - Sacrifice/discard-as-cost departure paths discarded at that time (fixed by draw-combat-end-triggers).
  - Filter-scoped "a creature you control dies" not supported.

## Slice: draw-combat-end-triggers (merged)

- Branch `phase5/draw-combat-end-triggers`.
- Scope: extend LKI stash to Sacrifice cost payment; add draw/combat/end step conditions.
- Verify: 18 unit + 9 draw/combat/end + 3 sacrifice-dies tests, 0 failed.
- Key changes:
  - `move_to_graveyard_for_cost`/`commit_payment` return departed abilities; `submit_payment` stashes them.
  - `ConditionPrimitive::{DrawsCard, EachDrawsCard, BeginningOfYourCombat, BeginningOfEachCombat, BeginningOfYourEndStep, BeginningOfEachEndStep}`.
  - `draw_step`'s library-side retirement also stashes departed abilities (unreachable in practice).
- Unresolved:
  - Cleanup discard/draw library retirement still discard departed abilities (cleanup fixed later, draw left as inert).

## Fix: once-per-turn trigger cap (CR 603.2h)

- `ConditionSpec::PrimitiveOncePerTurn(ConditionPrimitive)`.
- `GameState.once_per_turn_triggered: HashSet<(ObjectId, u32)>`, reset each turn.
- `PendingTrigger.once_per_turn: bool`; `CollectTriggers` records matches per event before next event in same batch.
- `Game.Conditions.OncePerTurn(tag)`.
- 3 tests.

## Fix: untap-step triggers

- `AdvanceTurnStep::Untap` routes through real pipeline but with `PipelineExit::AdvanceTurnStep` so no priority window opens (CR 502.4).
- `ConditionPrimitive::{BeginningOfYourUntap, BeginningOfEachUntap}`.
- 3 tests.

## Fix: submit_discard triggers

- `submit_discard` stashes departed abilities and routes through `CollectTriggers` with `PipelineExit::CleanupSbaCheck`.
- `ConditionPrimitive::{DiscardsCard, EachDiscardsCard}`.
- 2 tests.

## Slice: intervening-if (merged)

- Branch `phase5/intervening-if`.
- Scope: CR 603.4 intervening-if clause.
- Verify: 3 integration + 4 unit tests, 476 total, 0 failed.
- Key changes:
  - `InterveningIfPrimitive::{ControllerAtOrBelowLife(i32)}`.
  - `InterveningIfSpec::Primitive` (Script deferred).
  - `RegisteredAbility.intervening_if: Option<InterveningIfSpec>`; `StackObject.intervening_if` baked at push.
  - `collect_triggers` checks at trigger time; `ResolveStackTop` rechecks at resolution and ORs into fizzle.
  - `Game.InterveningIf.AtOrBelowLife(n)` and `:SetInterveningIf(tag)`.
- Deviations:
  - ~70 `RegisteredAbility` struct literals + 3 `StackObject` sites needed new field.
- Unresolved:
  - `InterveningIfSpec::Script` and more primitive shapes (later fixed by trigger-completeness-cleanup).

## Slice: trigger-completeness-cleanup (merged)

- Branch `phase5/trigger-completeness-cleanup`.
- Scope: close remaining trigger gaps: scry/surveil, filter-scoped dies/leaves, script intervening-if.
- Verify: 13 tests, 546 total, 0 failed.
- Key changes:
  - `ConditionPrimitive::{Scries, EachScries, Surveils, EachSurveils}`.
  - `ConditionPrimitive::{DiesMatching, LeavesBattlefieldMatching(FilterSpec)}` (live-source loop, not departed loop).
  - `InterveningIfSpec::Script(ScriptHandle)`; `ScriptRuntime::invoke_intervening_if(handle, state, source, controller)`.
  - `state_to_lua` snapshot passed as 4th arg to `invoke_condition` and used by `invoke_intervening_if`.
- Deviations:
  - `ConditionPrimitive`/`ConditionSpec` lost `Copy` due to `FilterSpec` field; `ConditionTag`/`OncePerTurnTag` dropped `Copy`.

## General trigger notes

- Trigger ordering: `group_apnap` groups by controller in APNAP order; intra-group ordering pauses via `AwaitTriggerOrderChoice`.
- Departed-abilities stash is per-pipeline-run, cleared in `end_pipeline_run`.
