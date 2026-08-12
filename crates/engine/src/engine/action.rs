//! `submit_action`'s validation entry point, `CommandId` allocation, and the
//! `ProcessState::PendingAction` pipeline stage - split out of `engine.rs`.

use crate::commands::{self, find_ability, PendingPaymentRequest, PriorityHandoff};
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

    /// Validates `action` is legal right now (right player, matches what's
    /// actually available) and queues it for the next `step()` to act on.
    /// Does not itself advance the ledger — `step()` does that.
    ///
    /// Only checks "are we even waiting for an action right now" and "is
    /// it this player's priority" (CR 117.3a). Cost/target/timing legality
    /// beyond that isn't designed yet — see `commands::execute_command`'s
    /// TODOs for where that has to land.
    ///
    /// CR 508.1/509.1/510.1 exception: `DeclareAttackers`/`DeclareBlockers`/
    /// `AssignCombatDamage` are turn-based actions that explicitly "don't
    /// use the stack" and aren't a priority decision at all - the defending
    /// player has no reason to hold priority when blockers are declared
    /// (508.2: priority stays with the active player through the declare-
    /// attackers step), and combat damage is dealt automatically as the
    /// combat damage step opens (510.1/510.2), not in response to a
    /// priority window. Skipping the priority check for these three and
    /// leaving the actor check entirely to `execute_command` (active player
    /// / defending player, per CR 506.2) is a deliberate simplification -
    /// the fully correct model would have these fire automatically on
    /// entering their step, before any priority window opens, which this
    /// engine doesn't have a pipeline stage for yet (`ProcessState`/
    /// `AdvanceTurnStep` only open a generic priority window on every step,
    /// combat steps included).
    pub fn submit_action(&mut self, action: PlayerAction) -> Result<(), ActionError> {
        match &self.process_state {
            ProcessState::PriorityWindow => {}
            // CR 601.2g/605.3a: a mana ability may be activated "even if
            // it's in the middle of casting or resolving a spell or
            // activating or resolving an ability" that itself needs a mana
            // payment - the one exception to "only in a real priority
            // window", and only for the payer's own mana abilities, never
            // any other action (a second spell, a non-mana ability, or
            // anyone but the payer trying to interject). `suspend_current_payment`
            // stashes the interrupted `AwaitingPayment` (plus the
            // `pipeline_context`/`pipeline_exit` this nested action's own
            // `handle_pending_action` call is about to overwrite) onto
            // `suspended_payments`, resumed by `resume_suspended_payment_or`
            // once this nested action's own pipeline run settles.
            ProcessState::AwaitingPayment { payer, .. }
                if action.player == *payer && self.is_mana_ability_activation(&action.kind) =>
            {
                self.suspend_current_payment();
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
        self.process_state = ProcessState::PendingAction(action);
        Ok(())
    }

    /// CR 605.1a/605.3a: `submit_action`'s own scope-check for its nested-
    /// mid-payment exception - `true` only for an `ActivateAbility` whose
    /// target ability is a real mana ability (`AddMana`/`AddManaChoice`),
    /// same effect-shape dispatch `commands::activate::execute_activate_ability`
    /// itself already branches on. `false` (not just "no such ability") for
    /// a bad `source`/`ability_index` too - either way, `submit_action`
    /// rejects up front rather than queuing something `execute_command`
    /// would just reject again downstream, since a downstream rejection
    /// would (harmlessly, but pointlessly) still suspend the outer payment
    /// first via `handle_pending_action`'s error-arm fallback.
    fn is_mana_ability_activation(&self, kind: &PlayerActionKind) -> bool {
        let PlayerActionKind::ActivateAbility { source, ability_index, .. } = kind else {
            return false;
        };
        let Some(ability) = find_ability(&self.state, *source, *ability_index) else {
            return false;
        };
        matches!(ability.effect, Some(AbilityEffect::AddMana(_) | AbilityEffect::AddManaChoice { .. }))
    }

    /// `submit_action`'s own nested-mid-payment branch: pulls the current
    /// `AwaitingPayment` (and the `pipeline_context`/`pipeline_exit` the
    /// about-to-run nested `PendingAction` would otherwise clobber - see
    /// `Engine::suspended_payments`' own doc comment) off the live fields
    /// and onto the suspension stack, leaving `process_state` set to
    /// whatever the caller assigns next (`submit_action` always follows
    /// this with `ProcessState::PendingAction(action)`, same as the normal
    /// `PriorityWindow` branch). `mem::take`/`mem::replace`, not `.clone()`
    /// - nothing reads the live fields again before the nested action's own
    /// `handle_pending_action` call overwrites them anyway, so there's
    /// nothing to preserve in place.
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
        let command_id = self.allocate_command_id();
        let since_ledger = self.state.ledger.len();
        let command = EngineCommand { command_id, actor: action.player, action: action.kind };

        let mut outcome = match commands::execute_command(&mut self.state, &self.script_runtime, command) {
            Ok(outcome) => outcome,
            Err(err) => {
                // Reset to PriorityWindow rather than leaving process_state
                // in limbo - see StepStatus::Error's doc comment for why
                // this doesn't attempt rollback. If this was a nested
                // mana-ability activation (CR 601.2g/605.3a) that errored
                // out before `pipeline_context`/`pipeline_exit` were even
                // touched, resume the outer `AwaitingPayment` it suspended
                // instead of abandoning it - see `submit_action`'s own doc
                // comment.
                self.process_state = self.resume_suspended_payment_or(ProcessState::PriorityWindow);
                return StepStatus::Error(err);
            }
        };

        // CommandOutcome::battlefield_entries - execute_command
        // takes &ScriptRuntime as of the layer-engine-cda slice (for
        // effective_power/effective_keywords's read-only CDA
        // resolution), but that's an immutable borrow - it still
        // can't run a card's self-registering script itself
        // (run_card_script needs &mut ScriptRuntime to load into the
        // registry), so it just reports which ids became new
        // battlefield permanents; this is the "one place performs
        // the side effect" half, same split priority_handoff/broke_pass_chain
        // already use. TODO: a script error here is silently
        // dropped, same gap ResolveStackTop's own call flags.
        //
        // CR 603.6d/614.1c: `battlefield_entries` is only ever
        // populated by `PlayLand` (a hand->battlefield move) as of
        // this writing - once the script above has (possibly)
        // self-registered an EntersTapped/EntersWithCounters effect
        // targeting this same object, apply it (and any externally
        // granted one already sitting in state.replacement_effects)
        // right here, same call `move_resolving_spell`'s own
        // ResolveStackTop caller makes.
        // TODO(`phase5/devour-amplify-search-entry`): no
        // `replacement::pending_devour_choice`/`pending_amplify_choice`
        // check here, unlike `Search`'s own battlefield-entry loop - and
        // deliberately so, not just unimplemented. This loop only ever runs
        // for `PlayLand` (a land entering the battlefield), and a land can
        // never carry CR 702.82a Devour or CR 702.38a Amplify text (both are
        // creature-only replacement effects) - there is no real card this
        // could ever matter for. Revisit only if this loop is ever
        // generalized to a non-land battlefield-entry producer.
        for object_id in &outcome.battlefield_entries {
            if let Some(definition) = self.state.object(*object_id).map(|o| o.definition.clone()) {
                let _ = self.script_runtime.run_card_script(&mut self.state, &definition, *object_id);
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

        self.pipeline_context = Some(PipelineContext {
            since_ledger,
            caused_by_command: Some(command_id),
            parent_event: None,
        });
        // See `pending_payment`'s doc comment - read here (this
        // command's own outcome) rather than inferred later, since
        // CommitEvents has no way to know "was this a CastSpell"
        // without it.
        self.pending_payment = outcome.requires_payment;

        // engine.rs owns state.priority_player entirely now -
        // execute_command just reports intent (CommandOutcome::
        // priority_handoff, commands.rs) instead of writing the
        // field itself.
        if let PriorityHandoff::To(player) = outcome.priority_handoff {
            self.state.priority_player = player;
        }

        // engine.rs owns state.consecutive_passes entirely now -
        // execute_command just reports whether its variant broke
        // the chain (CommandOutcome::broke_pass_chain, commands.rs)
        // instead of mutating the counter itself, so there's one
        // place deciding what "everyone just passed" means rather
        // than relying on every match arm in execute_command
        // remembering to reset it.
        if outcome.broke_pass_chain {
            self.state.consecutive_passes = 0;
            self.pipeline_exit = PipelineExit::BackToPriority;
        } else {
            self.state.consecutive_passes += 1;
            // CR 800.4b: an eliminated player is never asked for
            // priority, so "everyone passed" only needs a pass from
            // each player still in the game.
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
