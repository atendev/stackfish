# 01-pipeline

## Dry Run #1: pass-priority (discarded)

- Branch `phase5/pass-priority` (not merged, branch deleted after report).
- Scope: empty-stack `PassPriority` accepts, logs, hands priority to next player, resets pass count and advances step once all pass.
- Verify: scenario `phase5_pass_priority.toml`, `ledger_len == 2`.
- Key changes:
  - `GameState` gained `priority_player: PlayerId` and `consecutive_passes: u32`.
  - `Engine` gained `pending_action: Option<PlayerAction>` (later replaced by `ProcessState`).
  - `Phase`/`Step` gained `Clone, Copy, PartialEq, Eq` (later `Debug`).
  - Step advance hardcoded only `Beginning::Untap -> Upkeep`.
- Unresolved: combat state, replacement rewrite, script bridge gaps.

## Interface update: explicit `ProcessState`

- Real committed change to `engine.rs`.
- Added `ProcessState` enum:
  - `PriorityWindow`
  - `PendingAction(PlayerAction)`
  - `ResolveStackTop`
  - `ReplacementCheck { proposed }`
  - `CommitEvents { checked }`
  - `SbaLoop`
  - `CollectTriggers { since }`
  - `PutTriggersApnap { triggers }`
  - `AdvanceTurnStep`
  - later added `AwaitingPayment`, `AwaitingReplacementChoice`, `AwaitingTriggerOrderChoice`, `AwaitingLegendRuleChoice`, `AwaitingCommanderReturnChoice`, `AwaitingScriptChoice`, `AwaitingScryChoice`, `AwaitingSurveilChoice`, `AwaitingSearchChoice`, `AwaitingDiscard`, `GameOver`, `CleanupSbaCheck`.
- `Engine.process_state` replaces ad-hoc `pending_action`.
- Folded in for real: `priority_player`, `consecutive_passes`, `CommandError: Debug`.
- Real transitions wired; delegated work (`execute_command`, `apply_replacements`, `check_state_based_actions`, `collect_triggers`) still stubbed.
- Known incompleteness: pass-with-non-empty-stack branch, `since: 0` hardcoded, priority reset after triggers not done.

## `ProcessState` correctness pass

- Added `engine_context.rs`:
  - `PipelineContext { since_ledger, caused_by_command, parent_event }`
  - `PipelineExit { BackToPriority, AdvanceTurnStep, CleanupCheck, CleanupSbaCheck }`
- `since: 0` bug fixed: context captures real ledger position.
- `StepStatus::Error(CommandError)` replaces `.expect()`; engine resets to `PriorityWindow` (no rollback, no atomicity yet).
- Pass-chain logic: `CommandOutcome { events, broke_pass_chain, priority_handoff, requires_payment, battlefield_entries }`. `engine.rs` owns `consecutive_passes` mutation; `execute_command` only reports.
- Causality: `CommitEvents` fills `caused_by_command`/`parent_event` from context.
- `apply_replacements` takes `&mut GameState` (consumption can mutate).
- `CommandId`/`EventId` became monotonic counters (`next_command_id`, `next_event_id`), not derived from `ledger.len()`.
- `StepOutcome` gained `Errored(String)`; `run_action` fixed to not swallow returned errors.

## Fix: double-writing ledger events

- `commands::execute_command` now returns `Result<Vec<GameEventKind>, CommandError>` (proposed events) and mutates only non-ledger state.
- `engine.rs` is the sole ledger writer (`CommitEvents` allocates IDs/pushes).

## Fix: pass-chain logic location

- `CommandOutcome::broke_pass_chain: bool` — only `PassPriority` is `false`; every other variant must be `true`.
- `engine.rs` increments/resets `consecutive_passes` in exactly one place.

## Fix: pipeline_context lifecycle

- Context set at pipeline start, cleared in `PutTriggersApnap` (TODO until implemented; later `end_pipeline_run` centralizes cleanup).

## Fix: ReplacementCheck await-choice (CR 616.1)

- `apply_replacements` returns `ReplacementOutcome::{Resolved(GameEventKind), NeedsOrderingChoice { affected_player, event, candidates, excluded }}`.
- `ProcessState::AwaitingReplacementChoice` carries `remaining`, `checked`, `excluded`, later generalized via `ReplacementResumeTarget`.
- `StepStatus::AwaitingReplacementChoice` and `Engine::submit_replacement_choice` added.
- Ordering choice pauses one event at a time; `excluded` tracks effects already applied to same event.

## Fix: trigger-order pause (CR 603.3b)

- Renamed `triggers::order_apnap` -> `group_apnap`, returns groups by controller in APNAP order.
- `ProcessState::AwaitTriggerOrderChoice { player, group, ... }` and `Engine::submit_trigger_order` added.
- `PendingTrigger` gained `Clone, Copy, PartialEq, Eq, Debug`.
- `commands::PriorityHandoff` (`Unchanged` / `To(PlayerId)`) — `execute_command` reports, `engine.rs` applies.
- `Engine::end_pipeline_run` centralizes pipeline-end transitions.

## Dry Run #2: pass-priority-loop (merged)

- Branch `phase5/pass-priority-loop`.
- Scope: complete empty/all-pass loop back to `PriorityWindow`; `AdvanceTurnStep` table; empty-trigger no-op.
- Verify: `cargo test --workspace` 7 new tests + existing, 0 failed; second commit added ordering/permutation/pipeline checks.
- Key changes:
  - `PassPriority` arm real in `commands::execute_command`.
  - `ResolveStackTop` pops stack, mock `StackObjectResolved` through pipeline.
  - `CollectTriggers` scans ledger slice, calls `collect_triggers`/`group_apnap`.
  - `AdvanceTurnStep` + `advance_turn_step()`: deterministic CR 500-514 table.
  - `triggers::group_apnap` real.
  - `apply_replacements` / `check_state_based_actions` no-op when empty.
- Struct deviations:
  - `GameState.postcombat_main: bool` added (later removed in favor of `Step::PreCombatMain`/`PostCombatMain`).
  - `Phase`/`Step` derives added (`Clone, Copy, PartialEq, Eq, Debug`).
  - `PlayerId`/`ObjectId`/id newtypes gained `Debug`; `PendingTrigger`, `StepStatus` too.
- Interface deviations:
  - `Engine::pipeline_context_active()` diagnostic added.
- Follow-up verification pass: `submit_trigger_order` gained real `is_permutation` check and `TriggerOrderError::NotAPermutation`; `MakeChoice` stub for non-pass pass-chain reset test; `pipeline_context` pre/post strengthened.

## Cleanup step wiring

- `ProcessState::CleanupSbaCheck` added; `Engine::begin_cleanup_step` drains `UntilEndOfTurn` effects, checks hand size, pauses on `AwaitingDiscard` if over max.
- `Engine::submit_discard` added as own entry point.
- CR 514.3: cleanup auto-advances unless SBA fires; priority opens if so. Later wired through real trigger collection.
