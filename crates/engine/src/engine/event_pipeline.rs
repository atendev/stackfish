//! The `ReplacementCheck` and `CommitEvents` pipeline stages. See `DESIGN.md`.

use std::collections::VecDeque;

use crate::commands::{self, PendingPaymentRequest};
use crate::replacement::{self, ReplacementOutcome};
use crate::types::event::{GameEvent, GameEventKind};

use super::{Engine, ProcessState, ReplacementResumeTarget, StepStatus};

impl Engine {
    /// Checks one event per `step()` so an ordering choice (CR 616.1) can pause
    /// the run on `AwaitingReplacementChoice`; `remaining` and `checked` carry
    /// the progress across that pause.
    pub(super) fn resolve_replacement_check_state(
        &mut self,
        mut remaining: VecDeque<GameEventKind>,
        mut checked: Vec<GameEventKind>,
    ) -> StepStatus {
        let Some(event) = remaining.pop_front() else {
            self.process_state = ProcessState::CommitEvents { checked };
            return StepStatus::Progressed;
        };
        // CR 614.8: never a destroy proposal. The real destroy producers run
        // their own inline replacement check.
        match replacement::apply_replacements(&mut self.state, &self.script_runtime, event, false) {
            ReplacementOutcome::Resolved(event) => {
                checked.push(event);
                self.process_state = ProcessState::ReplacementCheck { remaining, checked };
            }
            ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                self.process_state = ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::ReplacementCheck { remaining, checked },
                };
            }
        }
        StepStatus::Progressed
    }

    /// Appends the checked events to the ledger, applying the deferred damage and
    /// counter effects, then pauses on `AwaitingPayment` or continues to `SbaLoop`.
    pub(super) fn resolve_commit_events_state(&mut self, checked: Vec<GameEventKind>) -> StepStatus {
        let (caused_by_command, parent_event) = self
            .pipeline_context
            .as_ref()
            .map(|ctx| (ctx.caused_by_command, ctx.parent_event))
            .unwrap_or((None, None));

        // CR 700.2d: a mode of a multi-mode resolution just settled. Its tail
        // ends in the stack object's `StackObjectResolved`, but the object
        // hasn't resolved until the last mode has, so that marker is kept out
        // of the ledger and the next mode runs instead of the SBA loop.
        let modal_resume = self.modal_continuation.as_ref().is_some_and(|continuation| {
            checked.iter().any(|kind| {
                matches!(kind, GameEventKind::StackObjectResolved { stack_id } if *stack_id == continuation.stack_id())
            })
        });

        for kind in checked {
            if modal_resume && matches!(kind, GameEventKind::StackObjectResolved { .. }) {
                continue;
            }
            // CR 510.2: damage is applied only now that replacements have settled
            // its final amount and target.
            if let GameEventKind::DamageDealt { source, target, amount, is_combat_damage } = &kind {
                commands::apply_damage_event(
                    &mut self.state,
                    &self.script_runtime,
                    *source,
                    *target,
                    *amount,
                    *is_combat_damage,
                );
            }
            // CR 122.1a: applied here like damage, after replacements.
            // CR 613.7: the counter timestamp is allocated apart from the ledger
            // sequence number so the ledger numbering never gaps.
            if let GameEventKind::CounterAdded { object, kind, amount } = &kind {
                let timestamp = self.state.allocate_sequence_no();
                if let Some(obj) = self.state.object_mut(*object) {
                    obj.counters.add(kind.clone(), *amount, timestamp);
                }
            }
            let event_id = self.state.allocate_event_id();
            let sequence_no = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind,
                caused_by_command,
                parent_event,
            });
        }
        if modal_resume {
            if let Some(continuation) = self.modal_continuation.take() {
                return self.resume_modal_resolution(continuation);
            }
        }
        // CR 601.2f: pause for payment after the cast's events are committed.
        self.process_state = match self.pending_payment.take() {
            Some(PendingPaymentRequest { stack_id, payer, cost, source, on_paid }) => {
                ProcessState::AwaitingPayment { stack_id, payer, cost, source, on_paid }
            }
            None => ProcessState::SbaLoop,
        };
        StepStatus::Progressed
    }
}
