//! The `Engine` struct, its pipeline dispatcher (`advance`), error types and
//! plumbing. Each `ProcessState` arm delegates to a sibling module or `resolve/`.
//! See `DESIGN.md`.

use crate::commands::{Internal, PendingPaymentRequest, Rejected};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::script::{ScriptError, ScriptRuntime};
use crate::types::ability::{AbilityEffect, ConditionSpec, RegisteredAbility};
use crate::types::card::ScriptReference;
use crate::types::config::GameSettings;
use crate::types::cost::CostSpec;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ScriptHandle, Target};
use crate::types::turn::Phase;
use crate::types::zone::ZoneKind;

mod action;
#[cfg(test)]
mod atomicity_tests;
mod choice_answers;
#[cfg(test)]
mod decision_tests;
mod event_pipeline;
mod library_choices;
mod payment;
mod pregame;
mod process_state;
mod replacement_resume;
mod resolve;
#[cfg(test)]
mod tests;
mod triggers_pipeline;
mod turn;

pub use process_state::*;

/// Why `submit_action` rejected a `PlayerAction`: wrong player, or the command
/// itself was illegal (`Rejected`). A broken invariant surfaces as
/// `StepStatus::Error` instead.
#[derive(Debug)]
pub enum ActionError {
    NotYourTurn,
    IllegalAction(String),
    /// The command itself is illegal (CR 733.1). Nothing changed and the player
    /// may try another action.
    Rejected(Rejected),
}

/// Why `submit_replacement_choice` rejected an answer to a CR 616.1
/// ordering choice.
#[derive(Debug)]
pub enum ReplacementChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingReplacementChoice`.
    NotAwaitingChoice,
    /// `chosen` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_legend_rule_choice` rejected an answer to a CR 704.5j
/// legend-rule choice.
#[derive(Debug)]
pub enum LegendRuleChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingLegendRuleChoice`.
    NotAwaitingChoice,
    /// `kept` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_attach_choice` rejected an answer to a CR 303.4d choice. Only
/// reachable by constructing `AwaitingAttachChoice` directly.
#[derive(Debug)]
pub enum AttachChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingAttachChoice`.
    NotAwaitingChoice,
    /// `chosen` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_discard_choice` rejected an answer to a pending CR
/// 701.9/701.9b targeted-discard choice (`AbilityEffect::Discard`).
#[derive(Debug)]
pub enum DiscardChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingDiscardChoice`.
    NotAwaitingChoice,
    /// The submitting player isn't the one this pause is actually asking -
    /// only the targeted player may answer their own discard choice.
    NotCurrentPlayer,
    /// `chosen` wasn't one of the offered `candidates` (that player's own
    /// current hand).
    InvalidCandidate,
}

/// Why `submit_devour_choice` rejected an answer to a CR 702.82a Devour
/// choice.
#[derive(Debug)]
pub enum DevourChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingDevourChoice`.
    NotAwaitingChoice,
    /// One of `sacrificed` wasn't one of the offered `candidates`.
    InvalidCandidate,
    /// The same id appeared twice in `sacrificed`.
    DuplicateCandidate,
}

/// Why `submit_amplify_choice` rejected an answer to a CR 702.38a choice. Same shape
/// as `DevourChoiceError`.
#[derive(Debug)]
pub enum AmplifyChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingAmplifyChoice`.
    NotAwaitingChoice,
    /// One of `revealed` wasn't one of the offered `candidates`.
    InvalidCandidate,
    /// The same id appeared twice in `revealed`.
    DuplicateCandidate,
}

/// Why `decline_madness_cast`/`submit_madness_cast` rejected an answer to a
/// CR 702.35a Madness choice.
#[derive(Debug)]
pub enum MadnessChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingMadnessChoice`.
    NotAwaitingChoice,
    /// `card` has no exile alternative cost. Defensive: nothing enforces that a
    /// madness card declares both halves.
    NoMadnessCost,
    /// `execute_command` rejected the synthetic `CastSpell`; the pause stays open.
    IllegalCast(Rejected),
    /// Casting broke an engine invariant. Fatal: `Engine::fatal_error` has the detail.
    Internal,
}

/// Why `decline_cascade_cast`/`submit_cascade_cast` rejected an answer to a CR 702.85a
/// choice.
#[derive(Debug)]
pub enum CascadeChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingCascadeChoice`.
    NotAwaitingChoice,
    /// There was no candidate, so only `decline_cascade_cast` is legal.
    NoCandidate,
    /// `execute_command` rejected the synthetic `CastSpell`; the pause stays open.
    IllegalCast(Rejected),
    /// Casting broke an engine invariant. Fatal: `Engine::fatal_error` has the detail.
    Internal,
}

/// Why `submit_unless_paid_payment`/`decline_unless_paid` rejected an answer
/// to a pending CR 118.12/118.12a "unless paid" pause
/// (`AbilityEffect::UnlessPaid` — Ward/Mana Leak/Echo-style).
#[derive(Debug)]
pub enum UnlessPaidError {
    /// Engine isn't sitting in `ProcessState::AwaitingUnlessPaidChoice`.
    NotAwaitingChoice,
    /// The submitting player isn't the front of `payer_queue`.
    NotCurrentPayer,
    /// The plan doesn't cover the pending `cost`; the pause stays open.
    InvalidPlan,
}

/// Why `submit_copy_choice` rejected an answer to a CR 707.9 choice. No duplicate
/// variant since the answer is a single `Option`.
#[derive(Debug)]
pub enum CopyChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingCopyChoice`.
    NotAwaitingChoice,
    /// `Some(id)` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_mode_choice` rejected an answer to a CR 700.2e "chosen by
/// opponent" modal choice.
#[derive(Debug)]
pub enum ModeChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingModeChoice`.
    NotAwaitingChoice,
    /// `modes.len()` fell outside the announced `min..=max` range.
    InvalidCount,
    /// The same mode index appeared twice (CR 700.2e).
    DuplicateMode,
    /// One of `modes` wasn't a valid index into the card's own mode list.
    NoSuchMode,
    /// 2+ modes chosen but combining them hit `mode_can_join_others`'s narrowing.
    UnsupportedModeCombination,
}

/// Why `submit_commander_return_choice` rejected an answer to a CR 903.9a
/// commander-return choice.
#[derive(Debug)]
pub enum CommanderReturnChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingCommanderReturnChoice`.
    NotAwaitingChoice,
}

/// Why `submit_script_choice` rejected an answer to a `Game.SelectTargets` pause.
#[derive(Debug)]
pub enum ScriptChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingScriptChoice`.
    NotAwaitingChoice,
    /// `chosen` fails the request's `TargetFilter` (CR 115).
    InvalidCandidate,
    /// The answer shape doesn't match the pending request (target versus may-choice).
    WrongRequestKind,
}

/// Why `submit_trigger_order` rejected an answer to a CR 603.3b
/// intra-group ordering choice.
#[derive(Debug)]
pub enum TriggerOrderError {
    /// Engine isn't sitting in `ProcessState::AwaitTriggerOrderChoice`.
    NotAwaitingChoice,
    /// `chosen_order` has the wrong length.
    WrongLength,
    /// Right length but not the same triggers; checked by `is_permutation`.
    NotAPermutation,
}

/// Why `submit_trigger_targets` rejected an answer to a CR 603.3d
/// triggered-ability target choice.
#[derive(Debug)]
pub enum TriggerTargetsError {
    /// Engine isn't sitting in `ProcessState::AwaitingTriggerTargets`.
    NotAwaitingChoice,
    /// `chosen` failed `validate_single_target_requirement`.
    InvalidTargets,
}

/// Why `submit_trigger_mode_choice` rejected an answer to a CR 603.3c mode choice.
#[derive(Debug)]
pub enum TriggerModeChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingTriggerModeChoice`.
    NotAwaitingChoice,
    /// `modes.len()` fell outside the announced `min..=max` range.
    InvalidCount,
    /// The same mode index appeared twice and the choice isn't repeatable (CR 700.2d).
    DuplicateMode,
    /// One of `modes` wasn't a valid index into the ability's own mode list.
    NoSuchMode,
    /// 2+ modes chosen but combining them hit `combine_chosen_modes`'s narrowing.
    UnsupportedModeCombination,
}

/// Why `submit_payment` rejected an answer to a CR 601.2f-h
/// `ProcessState::AwaitingPayment` pause.
#[derive(Debug)]
pub enum PaymentError {
    /// Engine isn't sitting in `ProcessState::AwaitingPayment`.
    NotAwaitingPayment,
    /// The plan doesn't validate against the pending cost. State is untouched.
    InvalidPlan,
}

/// Why `submit_discard` rejected an answer to a CR 514.1
/// `ProcessState::AwaitingDiscard` pause.
#[derive(Debug)]
pub enum DiscardError {
    /// Engine isn't sitting in `ProcessState::AwaitingDiscard`.
    NotAwaitingDiscard,
    /// Wrong number of cards — exactly `expected` must be discarded.
    WrongCount { expected: usize, got: usize },
    /// Same card id listed more than once.
    DuplicateCard,
    /// A card id isn't in the active player's hand under their control.
    CardNotInHand,
}

/// Why `submit_scry_choice` rejected an answer to a CR 701.22
/// `ProcessState::AwaitingScryChoice` pause.
#[derive(Debug)]
pub enum ScryChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingScryChoice`.
    NotAwaitingChoice,
    /// The two piles don't hold exactly the pending cards, once each.
    WrongCards,
}

/// Why `submit_surveil_choice` rejected an answer to a CR 701.25 pause. Its own type,
/// one per pause shape.
#[derive(Debug)]
pub enum SurveilChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingSurveilChoice`.
    NotAwaitingChoice,
    /// The two piles don't hold exactly the pending cards, once each.
    WrongCards,
}

/// Why `submit_search_choice` rejected an answer to a CR 701.23
/// `ProcessState::AwaitingSearchChoice` pause.
#[derive(Debug)]
pub enum SearchChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingSearchChoice`.
    NotAwaitingChoice,
    /// More than `count` chosen (CR 701.23b allows fewer).
    TooMany,
    /// A chosen id isn't a candidate, or is repeated.
    NotACandidate,
}

/// Why `submit_mulligan_decision` rejected a `KeepHand` or `TakeMulligan`.
#[derive(Debug)]
pub enum MulliganError {
    /// Engine isn't sitting in `ProcessState::AwaitingMulliganDecision`.
    NotAwaitingDecision,
    /// `action.player` isn't the player whose turn it is to decide this round.
    WrongPlayer,
    /// Player submitted `TakeMulligan` but has already reached
    /// `GameSettings::max_mulligans`. They must keep.
    MaxMulligansReached,
    /// Only `KeepHand` and `TakeMulligan` are valid here.
    InvalidAction,
}

/// Why `submit_bottom_cards` rejected a `BottomCards` submission.
#[derive(Debug)]
pub enum BottomCardsError {
    /// Engine isn't sitting in `ProcessState::AwaitingBottomCardChoice`.
    NotAwaitingChoice,
    /// `action.player` isn't the player who needs to bottom cards right now.
    WrongPlayer,
    /// `cards.len()` isn't exactly `count`.
    WrongCount { expected: usize, got: usize },
    /// A card id in `cards` isn't currently in the player's hand.
    CardNotInHand,
    /// Same card id appears more than once in `cards`.
    DuplicateCard,
}

/// One frame of `Engine::suspended_payments`: what resumes an outer
/// `AwaitingPayment` after a nested mana ability settles (CR 605.3a). The pipeline
/// fields are saved because the nested `PendingAction` overwrites the live ones.
#[derive(Debug)]
struct SuspendedPayment {
    request: PendingPaymentRequest,
    pipeline_context: Option<PipelineContext>,
    pipeline_exit: PipelineExit,
}

/// The step-able rules processor. Wraps `GameState` with the turn, priority and
/// stack logic. A replay re-feeds the seed and actions into a fresh `Engine` and
/// calls `step()` one at a time.
pub struct Engine {
    state: GameState,
    process_state: ProcessState,
    /// Set when a pipeline run begins, read by `CommitEvents` and `CollectTriggers`.
    /// `PutTriggersApnap` must reset it, or the next run reads stale values.
    pipeline_context: Option<PipelineContext>,
    /// Chosen when a run begins; picks the next state once trigger placement finishes.
    pipeline_exit: PipelineExit,
    /// CR 603.10: triggered abilities captured off objects that died this run, after
    /// `retire_object` purged them. Trigger lookup falls back to it. `end_pipeline_run`
    /// must clear it, or a later run sees stale entries.
    departed_triggered_abilities: Vec<RegisteredAbility>,
    /// First ledger index of a `BecameTarget` logged while triggers were put on the
    /// stack (CR 603.3d), still to be scanned (Ward). Taken when the queue empties.
    recollect_triggers_since: Option<usize>,
    /// Monotonic `CommandId` counter; commands aren't ledger entries.
    next_command_id: u64,
    /// The first broken invariant. Once set, `step()` only reports it so a host can't
    /// keep playing a half-changed game. The text is for logs, never a player.
    fatal: Option<Internal>,
    /// Set by `PendingAction` from `requires_payment`, taken by `CommitEvents` to
    /// decide between `SbaLoop` and `AwaitingPayment`. Separate from `pipeline_context`
    /// because it belongs to one command's outcome.
    pending_payment: Option<PendingPaymentRequest>,
    /// CR 700.2d: the modes of a multi-mode resolution still to run after the
    /// one currently in the pipeline. Set by `resolve::modal`, taken by
    /// `resolve_commit_events_state`.
    modal_continuation: Option<resolve::ModalContinuation>,
    /// CR 605.3a: outer payments suspended by a nested mana-ability activation, LIFO.
    /// A cost-bearing mana ability can nest another level. Popped by
    /// `resume_suspended_payment_or`.
    suspended_payments: Vec<SuspendedPayment>,
    /// Pregame and format settings. Held here rather than in `GameState` because they
    /// configure the session and no rule changes them.
    pub settings: GameSettings,
    /// True once pregame setup is done or skipped. `new_game` starts false so the
    /// first `step()` runs setup; `Engine::new` starts true for tests that build
    /// `GameState` directly in `PreGame`.
    pregame_setup_done: bool,
    /// One Lua VM per `Engine`; nothing shares a runtime across engines.
    script_runtime: ScriptRuntime,
    /// Decision id counter for the `decision` facade.
    // TODO: move into the Session
    pub(crate) decisions: crate::decision::DecisionTracker,
}

/// What a rejected input must leave unchanged. `state` is compared structurally and
/// the engine fields without `PartialEq` through their `Debug` rendering.
#[derive(Clone, PartialEq, Debug)]
pub struct EngineSnapshot {
    pub state: GameState,
    pub next_command_id: u64,
    pub script_handles: usize,
    pub transient: String,
}

impl Engine {
    /// Captures the state a rejection must not change.
    pub fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            state: self.state.clone(),
            next_command_id: self.next_command_id,
            script_handles: self.script_runtime.handle_count(),
            transient: format!(
                "{:?}",
                (
                    &self.process_state,
                    &self.pipeline_context,
                    &self.pipeline_exit,
                    &self.departed_triggered_abilities,
                    &self.recollect_triggers_since,
                    &self.pending_payment,
                    &self.modal_continuation,
                    &self.suspended_payments,
                    &self.pregame_setup_done,
                    &self.decisions,
                )
            ),
        }
    }
}

impl Engine {
    pub fn new(state: GameState) -> Self {
        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            departed_triggered_abilities: Vec::new(),
            recollect_triggers_since: None,
            next_command_id: 0,
            fatal: None,
            pending_payment: None,
            modal_continuation: None,
            suspended_payments: Vec::new(),
            settings: GameSettings::default(),
            pregame_setup_done: true,
            script_runtime: ScriptRuntime::new(),
            decisions: crate::decision::DecisionTracker::new(),
        }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// The invariant that made the engine fatal, for a host's log. It can name hidden
    /// cards, so it must not reach a player.
    pub fn fatal_error(&self) -> Option<&Internal> {
        self.fatal.as_ref()
    }

    /// Records a broken invariant. The first one wins.
    pub fn poison(&mut self, internal: Internal) {
        self.fatal.get_or_insert(internal);
    }

    /// Read access for the `decision` facade.
    pub(crate) fn process_state(&self) -> &ProcessState {
        &self.process_state
    }

    /// Read access for the `decision` facade.
    pub(crate) fn script_runtime(&self) -> &ScriptRuntime {
        &self.script_runtime
    }

    /// True until the first `step()` runs pregame setup; no Decision is open before then.
    pub(crate) fn awaiting_pregame_setup(&self) -> bool {
        self.state.phase == Phase::PreGame && !self.pregame_setup_done
    }

    /// CR 601.2f: the cost awaiting `submit_payment` and its source object (the
    /// `TapSelf` target, not necessarily `stack_id`). `None` outside `AwaitingPayment`.
    pub fn pending_payment_cost(&self) -> Option<(&CostSpec, ObjectId)> {
        match &self.process_state {
            ProcessState::AwaitingPayment { cost, source, .. } => Some((cost, *source)),
            _ => None,
        }
    }

    /// Mutable access to the state. Scripts register abilities as objects enter
    /// zones, so a host building a game from definitions needs this to seed
    /// `RegisteredAbility` entries directly.
    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    /// CR 702.29b: runs the script of a card that just entered the hand so a hand-only
    /// ability (cycling) registers. Covers every library-to-hand producer. Leaving
    /// hand purges the abilities as for any zone change.
    // TODO: abilities that continue to exist in other zones have no consumer yet (CR 702.29b)
    pub(in crate::engine) fn run_card_script_for(&mut self, object_id: ObjectId) {
        if let Some(definition) = self.state.object(object_id).map(|o| o.definition.clone()) {
            // Marks the run done so `sync_off_battlefield_scripts` won't register it twice.
            self.state.off_battlefield_scripts_run.insert(object_id);
            let result =
                self.script_runtime
                    .run_card_script(&mut self.state, &definition, object_id);
            self.record_script_error(object_id, &definition.script_ref, result);
        }
    }

    /// CR 113.6, 604.3: runs the script of every object in a hand, library,
    /// graveyard or exile that hasn't had it run, so `SetFunctionsFrom` abilities
    /// register. A sweep at trigger collection covers every zone-move path.
    pub(in crate::engine) fn sync_off_battlefield_scripts(&mut self) {
        // Sorted because `objects` order isn't deterministic and each run allocates ability ids.
        let mut pending: Vec<ObjectId> = self
            .state
            .objects
            .values()
            .filter(|o| {
                matches!(
                    o.zone,
                    crate::ZoneKind::Hand
                        | crate::ZoneKind::Library
                        | crate::ZoneKind::Graveyard
                        | crate::ZoneKind::Exile
                )
            })
            .map(|o| o.object_id)
            .filter(|id| !self.state.off_battlefield_scripts_run.contains(id))
            .collect();
        pending.sort();
        for id in pending {
            self.run_card_script_for(id);
        }
    }

    /// Stores `request` as `pending_payment`, first running a spell's script for the
    /// stack zone. CR 601.2a: a spell's abilities (Cascade's cast trigger) exist from
    /// the moment it is on the stack. The one funnel for all `CastSpell` producers;
    /// the stack entry's kind gates it since abilities share `PaymentResolution::StackObject`.
    pub(in crate::engine) fn set_pending_payment(
        &mut self,
        request: Option<PendingPaymentRequest>,
    ) {
        if let Some(request) = &request {
            let is_spell = matches!(
                request.on_paid,
                crate::commands::PaymentResolution::StackObject
            ) && self.state.stack.iter().any(|s| {
                s.stack_id == request.stack_id && matches!(s.kind, crate::StackEntryKind::Spell)
            });
            if is_spell {
                self.run_card_script_for(request.stack_id);
            }
        }
        self.pending_payment = request;
    }

    /// Logs a `run_card_script` failure as a `ScriptError` event so it is observable;
    /// resolution carries on, as a script error isn't a rejected input. Skips a
    /// `ByKey` or empty `Inline` source, the ordinary shape of a vanilla fixture
    /// card, which would otherwise flood the ledger.
    pub(in crate::engine) fn record_script_error<T>(
        &mut self,
        source: ObjectId,
        script_ref: &ScriptReference,
        result: Result<T, ScriptError>,
    ) {
        let has_real_script = matches!(script_ref, ScriptReference::Inline(s) if !s.is_empty());
        if has_real_script && let Err(e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("card script error on source {source:?}: {e:?}"),
            }]);
        }
    }

    /// Loads a standalone Lua callback and returns a handle for hand-built
    /// `RegisteredAbility` fields.
    pub fn load_script(&mut self, source: &str) -> Result<ScriptHandle, ScriptError> {
        self.script_runtime.load_function(source)
    }

    /// Loads a condition source (a `Game.Conditions.*` one-liner or custom function).
    pub fn load_condition(&mut self, source: &str) -> Result<ConditionSpec, ScriptError> {
        self.script_runtime.load_condition(source)
    }

    /// Loads an operation source (a `Game.Ops.*` one-liner or custom function).
    pub fn load_operation(
        &mut self,
        source: &str,
    ) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
        self.script_runtime.load_operation(source)
    }

    /// A card's base target requirement from its script's `target_requirement()`.
    /// Takes `&self` since registering a scripted predicate uses interior mutability.
    /// A returned `ScriptHandle` is only valid against this engine's runtime.
    pub fn build_target_requirement(
        &self,
        source: &str,
    ) -> Result<Option<crate::types::target::TargetSpec>, ScriptError> {
        self.script_runtime.build_target_requirement(source)
    }

    /// A plain spell's resolution effect from its script's `operation()`.
    pub fn build_operation(
        &self,
        source: &str,
    ) -> Result<Option<(Option<AbilityEffect>, ScriptHandle)>, ScriptError> {
        self.script_runtime.build_operation(source)
    }

    /// A card's alternative costs (CR 601.2b) from its script's `alternative_costs()`.
    pub fn build_alternative_costs(
        &self,
        source: &str,
    ) -> Result<Vec<crate::types::cost::AlternativeCost>, ScriptError> {
        self.script_runtime.build_alternative_costs(source)
    }

    /// A card's buyback cost (CR 702.27a) from its script's `buyback_cost()`.
    pub fn build_buyback_cost(
        &self,
        source: &str,
    ) -> Result<Option<crate::types::mana::ManaCost>, ScriptError> {
        self.script_runtime.build_buyback_cost(source)
    }

    /// Diagnostic: whether `pipeline_context` is set. Should be false at
    /// `PriorityWindow`, since only `end_pipeline_run` clears it.
    pub fn pipeline_context_active(&self) -> bool {
        self.pipeline_context.is_some()
    }

    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    pub fn step(&mut self) -> StepResult {
        if let Some(internal) = &self.fatal {
            return StepResult {
                status: StepStatus::Error(internal.clone()),
            };
        }
        let status = self.advance();
        StepResult { status }
    }

    /// Wraps each kind in a `GameEvent`, allocating its ids, and pushes it to the ledger.
    pub(in crate::engine) fn log_events(&mut self, events: Vec<GameEventKind>) {
        for kind in events {
            let event_id = self.state.allocate_event_id();
            let sequence_no = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind,
                caused_by_command: None,
                parent_event: None,
            });
        }
    }

    /// CR 603.2: logs one `BecameTarget` per distinct battlefield object `stack_id`
    /// targets (CR 603.2c). Called once the object is on the stack and paid for.
    pub(in crate::engine) fn log_became_target(&mut self, stack_id: ObjectId) {
        let Some(stack_object) = self.state.stack.iter().find(|s| s.stack_id == stack_id) else {
            return;
        };
        let by_controller = stack_object.controller;
        let mut seen: Vec<ObjectId> = Vec::new();
        for target in &stack_object.targets {
            let Target::Object(id) = target else { continue };
            let on_battlefield = self
                .state
                .object(*id)
                .is_some_and(|o| o.zone == ZoneKind::Battlefield);
            if on_battlefield && !seen.contains(id) {
                seen.push(*id);
            }
        }
        let events = seen
            .into_iter()
            .map(|object| GameEventKind::BecameTarget {
                object,
                by: stack_id,
                by_controller,
            })
            .collect();
        self.log_events(events);
    }

    fn advance(&mut self) -> StepStatus {
        // `PriorityWindow` is a payload-free placeholder; every other variant is replaced below.
        match std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow) {
            ProcessState::PriorityWindow => {
                if self.state.phase == Phase::PreGame && !self.pregame_setup_done {
                    self.pregame_setup_done = true;
                    self.run_pregame_setup();
                    return StepStatus::Progressed;
                }
                StepStatus::AwaitingAction {
                    player: self.state.priority_player,
                }
            }

            ProcessState::PendingAction(action) => self.handle_pending_action(action),

            ProcessState::ResolveStackTop => self.resolve_stack_top(),

            ProcessState::ReplacementCheck { remaining, checked } => {
                self.resolve_replacement_check_state(remaining, checked)
            }

            ProcessState::AwaitingReplacementChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                resume,
            } => {
                // Re-presents the pause, leaving the state in place until the answer arrives.
                let status = StepStatus::AwaitingReplacementChoice {
                    player: affected_player,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume,
                };
                status
            }

            ProcessState::AwaitingLegendRuleChoice {
                player,
                candidates,
                then,
            } => {
                let status = StepStatus::AwaitingLegendRuleChoice {
                    player,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingLegendRuleChoice {
                    player,
                    candidates,
                    then,
                };
                status
            }

            ProcessState::AwaitingCommanderReturnChoice {
                player,
                commander,
                then,
            } => {
                let status = StepStatus::AwaitingCommanderReturnChoice { player, commander };
                self.process_state = ProcessState::AwaitingCommanderReturnChoice {
                    player,
                    commander,
                    then,
                };
                status
            }

            ProcessState::AwaitingAttachChoice {
                aura,
                controller,
                candidates,
            } => {
                let status = StepStatus::AwaitingAttachChoice {
                    player: controller,
                    aura,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingAttachChoice {
                    aura,
                    controller,
                    candidates,
                };
                status
            }

            ProcessState::AwaitingEntryAttachChoice {
                attaching,
                controller,
                candidates,
                stack_id,
                events_so_far,
                remaining,
                destination,
                shuffle_after,
            } => {
                let status = StepStatus::AwaitingEntryAttachChoice {
                    player: controller,
                    attaching,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingEntryAttachChoice {
                    attaching,
                    controller,
                    candidates,
                    stack_id,
                    events_so_far,
                    remaining,
                    destination,
                    shuffle_after,
                };
                status
            }

            ProcessState::AwaitingTokenAttachChoice {
                attaching,
                controller,
                candidates,
                stack_id,
                events_so_far,
                remaining_count,
                spec,
                copy_snapshot,
            } => {
                let status = StepStatus::AwaitingTokenAttachChoice {
                    player: controller,
                    attaching,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingTokenAttachChoice {
                    attaching,
                    controller,
                    candidates,
                    stack_id,
                    events_so_far,
                    remaining_count,
                    spec,
                    copy_snapshot,
                };
                status
            }

            ProcessState::AwaitingDevourChoice {
                object,
                controller,
                per_creature,
                candidates,
                stack_id,
                events_so_far,
            } => {
                let status = StepStatus::AwaitingDevourChoice {
                    player: controller,
                    object,
                    per_creature,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingDevourChoice {
                    object,
                    controller,
                    per_creature,
                    candidates,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingAmplifyChoice {
                object,
                controller,
                per_card,
                candidates,
                stack_id,
                events_so_far,
            } => {
                let status = StepStatus::AwaitingAmplifyChoice {
                    player: controller,
                    object,
                    per_card,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingAmplifyChoice {
                    object,
                    controller,
                    per_card,
                    candidates,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingMadnessChoice {
                player,
                card,
                stack_id,
                events_so_far,
            } => {
                let status = StepStatus::AwaitingMadnessChoice { player, card };
                self.process_state = ProcessState::AwaitingMadnessChoice {
                    player,
                    card,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingCascadeChoice {
                player,
                candidate,
                exiled,
                stack_id,
                events_so_far,
            } => {
                let status = StepStatus::AwaitingCascadeChoice { player, candidate };
                self.process_state = ProcessState::AwaitingCascadeChoice {
                    player,
                    candidate,
                    exiled,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingUnlessPaidChoice {
                payer_queue,
                cost,
                otherwise,
                target_id,
                source,
                stack_id,
                events_so_far,
            } => {
                // `payer_queue` is never empty while paused; `decline_unless_paid` leaves the state before it would be.
                let payer = *payer_queue
                    .front()
                    .expect("payer_queue is never empty while paused");
                let status = StepStatus::AwaitingUnlessPaidChoice {
                    payer,
                    cost: cost.clone(),
                    otherwise: otherwise.clone(),
                };
                self.process_state = ProcessState::AwaitingUnlessPaidChoice {
                    payer_queue,
                    cost,
                    otherwise,
                    target_id,
                    source,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingDiscardChoice {
                player,
                candidates,
                remaining_count,
                stack_id,
                events_so_far,
            } => {
                let status = StepStatus::AwaitingDiscardChoice {
                    player,
                    candidates: candidates.clone(),
                    remaining_count,
                };
                self.process_state = ProcessState::AwaitingDiscardChoice {
                    player,
                    candidates,
                    remaining_count,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingSearchDevourChoice {
                object,
                controller,
                per_creature,
                candidates,
                stack_id,
                events_so_far,
                remaining,
                destination,
                shuffle_after,
            } => {
                let status = StepStatus::AwaitingSearchDevourChoice {
                    player: controller,
                    object,
                    per_creature,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingSearchDevourChoice {
                    object,
                    controller,
                    per_creature,
                    candidates,
                    stack_id,
                    events_so_far,
                    remaining,
                    destination,
                    shuffle_after,
                };
                status
            }

            ProcessState::AwaitingSearchAmplifyChoice {
                object,
                controller,
                per_card,
                candidates,
                stack_id,
                events_so_far,
                remaining,
                destination,
                shuffle_after,
            } => {
                let status = StepStatus::AwaitingSearchAmplifyChoice {
                    player: controller,
                    object,
                    per_card,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingSearchAmplifyChoice {
                    object,
                    controller,
                    per_card,
                    candidates,
                    stack_id,
                    events_so_far,
                    remaining,
                    destination,
                    shuffle_after,
                };
                status
            }

            ProcessState::AwaitingCopyChoice {
                object,
                controller,
                exceptions,
                candidates,
                stack_id,
                events_so_far,
                colors_spent,
                carry_forward_if_declined,
                x_value,
            } => {
                let status = StepStatus::AwaitingCopyChoice {
                    player: controller,
                    object,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingCopyChoice {
                    object,
                    controller,
                    exceptions,
                    candidates,
                    stack_id,
                    events_so_far,
                    colors_spent,
                    carry_forward_if_declined,
                    x_value,
                };
                status
            }

            ProcessState::AwaitingScriptChoice {
                thread,
                request,
                stack_id,
                source,
                controller,
                events_so_far,
                each_player,
            } => {
                let status = StepStatus::AwaitingScriptChoice {
                    player: controller,
                    request: request.clone(),
                };
                self.process_state = ProcessState::AwaitingScriptChoice {
                    thread,
                    request,
                    stack_id,
                    source,
                    controller,
                    events_so_far,
                    each_player,
                };
                status
            }

            ProcessState::AwaitingScryChoice {
                stack_id,
                controller,
                events_so_far,
                cards,
            } => {
                let status = StepStatus::AwaitingScryChoice {
                    player: controller,
                    cards: cards.clone(),
                };
                self.process_state = ProcessState::AwaitingScryChoice {
                    stack_id,
                    controller,
                    events_so_far,
                    cards,
                };
                status
            }

            ProcessState::AwaitingSurveilChoice {
                stack_id,
                controller,
                events_so_far,
                cards,
            } => {
                let status = StepStatus::AwaitingSurveilChoice {
                    player: controller,
                    cards: cards.clone(),
                };
                self.process_state = ProcessState::AwaitingSurveilChoice {
                    stack_id,
                    controller,
                    events_so_far,
                    cards,
                };
                status
            }

            ProcessState::AwaitingSearchChoice {
                stack_id,
                controller,
                events_so_far,
                candidates,
                count,
                destination,
                shuffle_after,
            } => {
                let status = StepStatus::AwaitingSearchChoice {
                    player: controller,
                    candidates: candidates.clone(),
                    count,
                };
                self.process_state = ProcessState::AwaitingSearchChoice {
                    stack_id,
                    controller,
                    events_so_far,
                    candidates,
                    count,
                    destination,
                    shuffle_after,
                };
                status
            }

            ProcessState::CommitEvents { checked } => self.resolve_commit_events_state(checked),

            ProcessState::AwaitingPayment {
                stack_id,
                payer,
                cost,
                source,
                on_paid,
            } => {
                let status = StepStatus::AwaitingPayment {
                    player: payer,
                    stack_id,
                };
                self.process_state = ProcessState::AwaitingPayment {
                    stack_id,
                    payer,
                    cost,
                    source,
                    on_paid,
                };
                status
            }

            ProcessState::AwaitingModeChoice {
                stack_id,
                chooser,
                min,
                max,
            } => {
                let status = StepStatus::AwaitingModeChoice {
                    player: chooser,
                    stack_id,
                    min,
                    max,
                };
                self.process_state = ProcessState::AwaitingModeChoice {
                    stack_id,
                    chooser,
                    min,
                    max,
                };
                status
            }

            ProcessState::SbaLoop => self.resolve_sba_loop_state(),

            ProcessState::CollectTriggers { since } => self.resolve_collect_triggers_state(since),

            ProcessState::PutTriggersApnap {
                ordered,
                pending_groups,
            } => self.resolve_put_triggers_apnap_state(ordered, pending_groups),

            ProcessState::AwaitTriggerOrderChoice {
                player,
                group,
                ordered,
                pending_groups,
            } => {
                let status = StepStatus::AwaitingTriggerOrderChoice {
                    player,
                    group: group.clone(),
                };
                self.process_state = ProcessState::AwaitTriggerOrderChoice {
                    player,
                    group,
                    ordered,
                    pending_groups,
                };
                status
            }

            ProcessState::AwaitingTriggerTargets {
                trigger,
                spec,
                candidates,
                remaining,
                modal_effect,
            } => {
                let status = StepStatus::AwaitingTriggerTargets {
                    player: trigger.controller,
                    spec: spec.clone(),
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingTriggerTargets {
                    trigger,
                    spec,
                    candidates,
                    remaining,
                    modal_effect,
                };
                status
            }

            ProcessState::AwaitingTriggerModeChoice {
                trigger,
                modes,
                mode_choice,
                remaining,
                chooser,
            } => {
                // `chooser`, not the controller, is prompted; CR 700.2e was resolved before the pause.
                let (min, max, _repeatable) = mode_choice.bounds();
                let status = StepStatus::AwaitingTriggerModeChoice {
                    player: chooser,
                    modes: modes.clone(),
                    mode_choice,
                    min,
                    max,
                };
                self.process_state = ProcessState::AwaitingTriggerModeChoice {
                    trigger,
                    modes,
                    mode_choice,
                    remaining,
                    chooser,
                };
                status
            }

            ProcessState::AdvanceTurnStep => self.resolve_advance_turn_step_state(),

            ProcessState::AwaitingDiscard { player, excess } => {
                self.resolve_awaiting_discard_state(player, excess)
            }

            ProcessState::CleanupSbaCheck => self.resolve_cleanup_sba_check_state(),

            ProcessState::AwaitingMulliganDecision { player } => {
                self.process_state = ProcessState::AwaitingMulliganDecision { player };
                StepStatus::AwaitingMulliganDecision { player }
            }
            ProcessState::AwaitingBottomCardChoice {
                player,
                count,
                remaining,
            } => {
                self.process_state = ProcessState::AwaitingBottomCardChoice {
                    player,
                    count,
                    remaining,
                };
                StepStatus::AwaitingBottomCardChoice { player, count }
            }
            ProcessState::GameOver { winner } => {
                self.process_state = ProcessState::GameOver { winner };
                StepStatus::GameOver { winner }
            }
        }
    }
}
