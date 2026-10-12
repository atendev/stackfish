//! The generic command-pipeline event stages — `ReplacementCheck` (CR
//! 614-616) and `CommitEvents` (ledger append) — split out of `engine.rs`.

use std::collections::VecDeque;

use crate::commands::{self, PendingPaymentRequest};
use crate::replacement::{self, ReplacementOutcome};
use crate::types::event::{GameEvent, GameEventKind};

use super::{Engine, ProcessState, ReplacementResumeTarget, StepStatus};

impl Engine {
    /// `ProcessState::ReplacementCheck { remaining, checked }` — processes
    /// one event per `step()` call rather than mapping the whole `Vec` in
    /// one shot — `apply_replacements` can return `NeedsOrderingChoice` (CR
    /// 616.1) for any of them, and that has to pause the whole pipeline run
    /// on a real `AwaitingReplacementChoice`, not get silently dropped or
    /// resolved without the player. `checked`/`remaining` carry this run's
    /// progress across that pause.
    pub(super) fn resolve_replacement_check_state(
        &mut self,
        mut remaining: VecDeque<GameEventKind>,
        mut checked: Vec<GameEventKind>,
    ) -> StepStatus {
        let Some(event) = remaining.pop_front() else {
            self.process_state = ProcessState::CommitEvents { checked };
            return StepStatus::Progressed;
        };
        // Never a destroy proposal - every event reaching this
        // generic command-pipeline check (damage, sacrifice/discard
        // cost payments, ...) is unrelated to CR 704.5g/614.8; the
        // real destroy producers (SBA lethal damage, the scripted
        // Destroy op) both run their own inline replacement check
        // instead of this one, same as before this slice.
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

    /// `ProcessState::CommitEvents { checked }` — appends every checked
    /// event to `state.ledger` as a real `GameEvent`, applying the
    /// still-pending `DamageDealt`/`CounterAdded` side effects along the way
    /// (see the inline comments below for why those can't happen any
    /// earlier than here), then either pauses on `AwaitingPayment` (CR
    /// 601.2f-h) or continues into `SbaLoop`.
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
            // CR 510.2/614-616: a DamageDealt event describes an
            // effect that hasn't happened to state yet - apply it
            // now that it's in its final, post-replacement form
            // (amount/target may have been prevented/redirected).
            // See commands::apply_damage_event's own doc comment for
            // why this can't happen any earlier than here.
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
            // CR 122.1a: same "describes an effect that hasn't
            // happened to state yet" shape as DamageDealt above -
            // apply it now that it's in final, post-replacement form.
            // No replacement effect actually targets CounterAdded
            // yet (no EventFilter::CounterAdded), so this never
            // changes from what ResolveStackTop proposed - still
            // applied here rather than at push time, matching
            // Damage's convention rather than GainLife's shortcut.
            // CR 613.4c/613.7: this counter batch's own timestamp -
            // allocated ahead of the ledger event's own sequence_no
            // below rather than reusing it, so a CounterAdded that
            // ends up filtered out of `checked` (shouldn't happen,
            // but keeps the two concerns independent) never leaves a
            // gap in the ledger's own numbering.
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
        // CR 601.2f-h: if this run's command was a CastSpell that
        // needs payment, pause here instead of continuing into
        // SbaLoop - the events above (card moved to stack) are
        // already committed regardless (CR 601.2a happens before
        // cost is paid), but the cast process itself isn't finished
        // until Engine::submit_payment answers. ResolveStackTop runs
        // never set pending_payment, so they fall straight through
        // to SbaLoop exactly as before this slice.
        self.process_state = match self.pending_payment.take() {
            Some(PendingPaymentRequest { stack_id, payer, cost, source, on_paid }) => {
                ProcessState::AwaitingPayment { stack_id, payer, cost, source, on_paid }
            }
            None => ProcessState::SbaLoop,
        };
        StepStatus::Progressed
    }
}
