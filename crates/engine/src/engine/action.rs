//! `submit_action` validation, `CommandId` allocation and the
//! `ProcessState::PendingAction` pipeline stage. See `DESIGN.md`.

use crate::commands::{self, find_ability, Internal, PendingPaymentRequest, PriorityHandoff};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement;
use crate::types::ability::AbilityEffect;
use crate::types::command::{EngineCommand, PlayerAction, PlayerActionKind};
use crate::types::ids::CommandId;
use crate::types::mana::ColorSet;
use crate::types::zone::ZoneKind;

use super::{ActionError, Engine, ProcessState, StepStatus, SuspendedPayment};

impl Engine {
    pub(super) fn allocate_command_id(&mut self) -> CommandId {
        let id = CommandId(self.next_command_id);
        self.next_command_id += 1;
        id
    }

    /// Validates `action` against the current state and queues it for the next
    /// `step()`. A rejection leaves the engine untouched and the player keeps
    /// priority (CR 733.1/733.2).
    ///
    /// Declare-attackers, declare-blockers and combat damage skip the priority
    /// check and leave the actor check to `validate_command`: they are
    /// turn-based actions that don't use the stack (CR 508.1, 509.1, 510.1).
    // TODO: fire these on entering their step, before any priority window opens (CR 508.1, 510.1)
    pub fn submit_action(&mut self, action: PlayerAction) -> Result<(), ActionError> {
        // Every rejection below runs before the first mutation, including the
        // payment suspension in the nested mana-ability arm.
        let mut suspend_payment = false;
        match &self.process_state {
            ProcessState::PriorityWindow => {}
            // CR 605.3a: the payer's own mana abilities may interrupt a pending
            // payment; any other action is refused. The interrupted payment is
            // stashed on `suspended_payments` and resumed once the nested action settles.
            ProcessState::AwaitingPayment { payer, .. }
                if action.player == *payer && self.is_mana_ability_activation(&action.kind) =>
            {
                suspend_payment = true;
            }
            _ => {
                return Err(ActionError::IllegalAction(
                    "engine is mid-pipeline, not waiting for a player action".to_string(),
                ));
            }
        }
        let is_combat_turn_based_action = matches!(
            action.kind,
            PlayerActionKind::DeclareAttackers { .. }
                | PlayerActionKind::DeclareBands { .. }
                | PlayerActionKind::DeclareBlockers { .. }
                | PlayerActionKind::AssignCombatDamage { .. }
        );
        if !is_combat_turn_based_action && action.player != self.state.priority_player {
            return Err(ActionError::NotYourTurn);
        }
        if matches!(
            action.kind,
            PlayerActionKind::KeepHand
                | PlayerActionKind::TakeMulligan
                | PlayerActionKind::BottomCards { .. }
        ) {
            return Err(ActionError::IllegalAction(
                "pregame actions use submit_mulligan_decision / submit_bottom_cards".to_string(),
            ));
        }
        // Checked in full before queueing so the answer doesn't depend on the
        // caller; a rejection leaves the pending payment untouched.
        commands::validate_command(&self.state, &self.script_runtime, action.player, action.kind.clone())
            .map_err(ActionError::Rejected)?;
        if suspend_payment {
            self.suspend_current_payment();
        }
        self.process_state = ProcessState::PendingAction(action);
        Ok(())
    }

    /// CR 605.1a: true only for an `ActivateAbility` on a real mana ability.
    /// A bad `source` or `ability_index` is false too, so it is rejected up
    /// front instead of suspending the outer payment first.
    fn is_mana_ability_activation(&self, kind: &PlayerActionKind) -> bool {
        let PlayerActionKind::ActivateAbility { source, ability_index, .. } = kind else {
            return false;
        };
        let Some(ability) = find_ability(&self.state, *source, *ability_index) else {
            return false;
        };
        matches!(ability.effect, Some(AbilityEffect::AddMana(_) | AbilityEffect::AddManaChoice { .. }))
    }

    /// Moves the current `AwaitingPayment` and the pipeline fields the nested
    /// action would overwrite onto `suspended_payments`. Takes the values
    /// rather than cloning since nothing reads the live fields before they are overwritten.
    fn suspend_current_payment(&mut self) {
        let ProcessState::AwaitingPayment { stack_id, payer, cost, source, on_paid } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("caller only takes this branch when process_state is AwaitingPayment");
        };
        self.suspended_payments.push(SuspendedPayment {
            request: PendingPaymentRequest { stack_id, payer, cost, source, on_paid },
            pipeline_context: self.pipeline_context.take(),
            pipeline_exit: std::mem::replace(&mut self.pipeline_exit, PipelineExit::BackToPriority),
        });
    }

    /// `ProcessState::PendingAction(action)` — executes the queued action via
    /// `commands::execute_command` and folds its `CommandOutcome` into
    /// `state`, then hands off to `ReplacementCheck` for the events it
    /// proposed.
    pub(super) fn handle_pending_action(&mut self, action: PlayerAction) -> StepStatus {
        // The id is consumed only once the command is accepted.
        let command_id = CommandId(self.next_command_id);
        let since_ledger = self.state.ledger.len();
        let command = EngineCommand { command_id, actor: action.player, action: action.kind };

        let mut outcome = match commands::execute_command(&mut self.state, &self.script_runtime, command) {
            Ok(outcome) => outcome,
            Err(err) => {
                // `submit_action` ran the same `validate_command`, so a rejection
                // here means the state changed in between or the engine broke:
                // fatal either way. The process state is reset and a nested mana
                // ability's outer payment resumed so a host finds the engine consistent.
                let internal = match err {
                    commands::CommandError::Internal(internal) => internal,
                    commands::CommandError::Rejected(rejected) => {
                        Internal::new(format!("a queued action was rejected after it was accepted: {rejected:?}"))
                    }
                };
                self.process_state = self.resume_suspended_payment_or(ProcessState::PriorityWindow);
                self.poison(internal.clone());
                return StepStatus::Error(internal);
            }
        };

        // `execute_command` only reports new battlefield permanents; running their
        // scripts needs `&mut ScriptRuntime`, so it happens here. A script error is
        // logged as a `ScriptError` event instead of rejecting the command.
        //
        // CR 603.6d, 614.1c: apply enters-the-battlefield replacements once the
        // script has registered its own.
        // TODO: devour/amplify choice here if this loop ever handles non-land entries (CR 702.82a, 702.38a)
        for object_id in &outcome.battlefield_entries {
            if let Some(definition) = self.state.object(*object_id).map(|o| o.definition.clone()) {
                let result = self.script_runtime.run_card_script(&mut self.state, &definition, *object_id);
                self.record_script_error(*object_id, &definition.script_ref, result);
            }
            let counter_events = replacement::apply_enters_battlefield_modifiers(
                &mut self.state,
                &self.script_runtime,
                *object_id,
                ZoneKind::Hand,
                ColorSet::empty(),
            );
            outcome.events.extend(counter_events);
        }

        self.next_command_id += 1;
        self.pipeline_context = Some(PipelineContext {
            since_ledger,
            caused_by_command: Some(command_id),
            parent_event: None,
        });
        // Read from this outcome because `CommitEvents` can't tell a cast apart.
        self.set_pending_payment(outcome.requires_payment);

        // Only the engine writes `priority_player`; commands just report intent.
        if let PriorityHandoff::To(player) = outcome.priority_handoff {
            self.state.priority_player = player;
        }

        // One place decides what "everyone just passed" means, so no command arm
        // has to remember to reset the counter.
        if outcome.broke_pass_chain {
            self.state.consecutive_passes = 0;
            self.pipeline_exit = PipelineExit::BackToPriority;
        } else {
            self.state.consecutive_passes += 1;
            // CR 800.4b: only players still in the game need to pass.
            self.pipeline_exit = if self.state.consecutive_passes as usize
                >= commands::non_eliminated_players(&self.state).len()
            {
                self.state.consecutive_passes = 0;
                if !self.state.stack.is_empty() {
                    PipelineExit::ResolveStackTop
                } else {
                    PipelineExit::AdvanceTurnStep
                }
            } else {
                PipelineExit::BackToPriority
            };
        }

        self.process_state =
            ProcessState::ReplacementCheck { remaining: outcome.events.into(), checked: Vec::new() };
        StepStatus::Progressed
    }
}
