//! Cost-payment answers (`submit_payment`, `unwind_payment`) and the cost
//! zone-move queue they drive. See `DESIGN.md`.

use std::collections::VecDeque;

use crate::commands::{self, PaymentResolution, PendingPaymentRequest};
use crate::replacement::{self, ReplacementOutcome};
use crate::types::cost::{AdditionalCost, PaymentPlan};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::stack::StackEntryKind;
use crate::types::zone::ZoneKind;

use super::{
    Engine, PaymentError, PendingZoneMove, ProcessState, ReplacementResumeTarget, SuspendedPayment,
};

/// CR 702.29c: the `Cycled` marker for a cost zone-move of `cycling_source`,
/// `None` for any other payment. Shared with the `CostZoneMove` resume arm.
pub(super) fn cycled_marker_event(
    event: &GameEventKind,
    cycling_source: Option<ObjectId>,
) -> Option<GameEventKind> {
    let GameEventKind::ZoneChange {
        object,
        last_known_info: Some(lki),
        ..
    } = event
    else {
        return None;
    };
    if Some(*object) != cycling_source {
        return None;
    }
    Some(GameEventKind::Cycled {
        object: *object,
        last_known_info: lki.clone(),
    })
}

impl Engine {
    /// Commits `plan` only if it validates against the pending cost, otherwise
    /// rejects without touching state (CR 601.2h). A separate entry point from
    /// `submit_action` because it answers a sub-decision, not a priority action.
    pub fn submit_payment(&mut self, plan: PaymentPlan) -> Result<(), PaymentError> {
        let ProcessState::AwaitingPayment { payer, source, .. } = &self.process_state else {
            return Err(PaymentError::NotAwaitingPayment);
        };
        let payer = *payer;
        let source = *source;
        let valid = {
            let ProcessState::AwaitingPayment { cost, .. } = &self.process_state else {
                unreachable!("checked above");
            };
            plan.validate(cost, &self.state, &self.script_runtime, payer, source)
        };
        if !valid {
            return Err(PaymentError::InvalidPlan);
        }
        // CR 702.29c: identify a cycling discard here, before the Sacrifice/
        // Discard queue loses track of which cost each payment answered.
        let cycling_source = {
            let ProcessState::AwaitingPayment { cost, .. } = &self.process_state else {
                unreachable!("checked above");
            };
            cost.additional
                .iter()
                .any(|c| matches!(c, AdditionalCost::DiscardSelf))
                .then_some(source)
        };
        let ProcessState::AwaitingPayment {
            stack_id, on_paid, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 601.2h: mana, tap and life commit immediately; sacrifices and
        // discards are queued because each is its own replaceable zone change
        // (CR 616.1).
        let pending_zone_moves: VecDeque<PendingZoneMove> =
            commands::collect_cost_zone_moves(&plan.payments)
                .into_iter()
                .map(|(id, from, to)| PendingZoneMove { id, from, to })
                .collect();
        let immediate_events = commands::commit_payment(&mut self.state, payer, &plan.payments);
        if let Some(stack_object) = self.state.stack.iter_mut().find(|s| s.stack_id == stack_id) {
            stack_object.costs_paid = plan.payments;
        }
        for kind in immediate_events {
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

        // Resumes where `CommitEvents` would have gone had no payment been pending.
        self.process_state = self.continue_cost_zone_moves(
            pending_zone_moves,
            payer,
            stack_id,
            on_paid,
            cycling_source,
        );
        Ok(())
    }

    /// Drains the queued cost zone moves one at a time, since any of them can
    /// raise an ordering choice (CR 616.1). Departed abilities are captured as
    /// each move commits so `CollectTriggers` sees them (CR 603.10).
    ///
    /// Once the queue empties it applies `on_paid`, logs `CostPaid` and returns
    /// `SbaLoop`. `replacement_resume.rs` also calls it to resume a paused move.
    pub(super) fn continue_cost_zone_moves(
        &mut self,
        mut remaining: VecDeque<PendingZoneMove>,
        payer: PlayerId,
        stack_id: ObjectId,
        on_paid: PaymentResolution,
        cycling_source: Option<ObjectId>,
    ) -> ProcessState {
        while let Some(pending) = remaining.pop_front() {
            let Some(proposed) =
                commands::propose_zone_move(&self.state, pending.id, pending.from, pending.to)
            else {
                // Already gone; `PaymentPlan::validate` confirmed it existed.
                continue;
            };
            // CR 614.8: neither sacrifice nor discard is destruction.
            match replacement::apply_replacements(
                &mut self.state,
                &self.script_runtime,
                proposed,
                false,
            ) {
                ReplacementOutcome::Resolved(event) => {
                    let cycled = cycled_marker_event(&event, cycling_source);
                    // CR 702.35a: derived before commit purges the object's replacement effects.
                    let madness = replacement::madness_marker_event(&self.state, &event);
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    if let Some(kind) = cycled {
                        self.log_events(vec![kind]);
                    }
                    if let Some(kind) = madness {
                        self.log_events(vec![kind]);
                    }
                    self.state.ledger.push(committed);
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CostZoneMove {
                            remaining,
                            payer,
                            stack_id,
                            on_paid,
                            cycling_source,
                        },
                    };
                }
            }
        }
        // CR 605.3b: a mana ability never reaches the stack, so its mana is added here.
        if let PaymentResolution::ManaAbility { controller, units } = &on_paid {
            let Some(player) = self.state.player_mut(*controller) else {
                unreachable!("a mana ability's own controller always has player state");
            };
            for unit in units {
                player.mana_pool.add_unit(*unit);
            }
            let event_id = self.state.allocate_event_id();
            let sequence_no = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind: GameEventKind::ManaAdded {
                    player: *controller,
                    units: units.clone(),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
        let event_id = self.state.allocate_event_id();
        let sequence_no = self.state.allocate_sequence_no();
        self.state.ledger.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::CostPaid { stack_id, payer },
            caused_by_command: None,
            parent_event: None,
        });
        // CR 601.2i: a spell is cast once its costs are paid. `CostPaid` alone
        // can't mark it because activated abilities log one too.
        if self
            .state
            .stack
            .iter()
            .any(|s| s.stack_id == stack_id && matches!(s.kind, StackEntryKind::Spell))
        {
            self.log_events(vec![GameEventKind::SpellCast { object: stack_id }]);
        }
        // CR 603.2: the targets become targeted once the object is fully announced and paid.
        self.log_became_target(stack_id);
        ProcessState::SbaLoop
    }

    /// CR 605.3a: pops a payment suspended by a nested mana ability and restores
    /// the pipeline fields the nested run overwrote, or returns `fallback` when
    /// nothing is suspended.
    pub(super) fn resume_suspended_payment_or(&mut self, fallback: ProcessState) -> ProcessState {
        let Some(SuspendedPayment {
            request,
            pipeline_context,
            pipeline_exit,
        }) = self.suspended_payments.pop()
        else {
            return fallback;
        };
        self.pipeline_context = pipeline_context;
        self.pipeline_exit = pipeline_exit;
        let PendingPaymentRequest {
            stack_id,
            payer,
            cost,
            source,
            on_paid,
        } = request;
        ProcessState::AwaitingPayment {
            stack_id,
            payer,
            cost,
            source,
            on_paid,
        }
    }

    /// Abandons the cast or activation because the caller cannot or will not pay
    /// (CR 601.6), where `submit_payment` would keep the pause open on a bad plan.
    ///
    /// The ledger is append-only, so a spell's card returns to hand under a
    /// fresh id (CR 400.7) via `move_resolving_spell` instead of a state rewind.
    /// An ability's stack entry is just removed. `CastingUnwound` marks the
    /// ledger either way.
    pub fn unwind_payment(&mut self) -> Result<(), PaymentError> {
        let ProcessState::AwaitingPayment { stack_id, .. } = &self.process_state else {
            return Err(PaymentError::NotAwaitingPayment);
        };
        let stack_id = *stack_id;

        let Some(idx) = self.state.stack.iter().position(|s| s.stack_id == stack_id) else {
            // Defensive: nothing else removes the stack object during the pause.
            self.process_state = self.end_pipeline_run();
            return Ok(());
        };
        let stack_object = self.state.stack.remove(idx);

        let mut events = Vec::new();
        if matches!(stack_object.kind, StackEntryKind::Spell)
            && let Some(event) = self.move_resolving_spell(stack_id, ZoneKind::Hand, false, false)
        {
            events.push(event);
        }
        events.push(GameEventKind::CastingUnwound { stack_id });

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

        self.process_state = self.end_pipeline_run();
        Ok(())
    }

    /// CR 608.2b, 608.2m: moves a resolving spell's card off the stack into
    /// `destination` under a fresh id (CR 400.7). `None` only if the stack
    /// object is already gone.
    pub(in crate::engine) fn move_resolving_spell(
        &mut self,
        stack_id: ObjectId,
        destination: ZoneKind,
        run_script: bool,
        // CR 702.138b: true only for a real battlefield entry of an escaped spell.
        entered_via_escape: bool,
    ) -> Option<GameEventKind> {
        let spell_object = self.state.object(stack_id)?;
        let last_known_info = spell_object.clone();
        // Battlefield and exile are shared zones.
        let destination_owner = match destination {
            ZoneKind::Battlefield | ZoneKind::Exile => None,
            _ => Some(last_known_info.owner),
        };

        let new_id = self.state.allocate_object_id();
        let mut new_object = last_known_info.clone();
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage {
            previous: Some(stack_id),
        };
        new_object.zone = destination;
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 302.6: only a permanent entering the battlefield is summoning sick.
        if destination == ZoneKind::Battlefield {
            new_object.controlled_since_turn = self.state.turn_number;
            // CR 702.138b: re-derived because the stack object's value is meaningless.
            new_object.entered_via_escape = entered_via_escape;
        }

        self.state.objects.remove(&stack_id);
        // CR 608.2b: a fizzled spell never runs its script.
        let definition_for_script = if run_script {
            Some(new_object.definition.clone())
        } else {
            None
        };
        if let Some(zone) = self.state.zone_mut(destination, destination_owner) {
            zone.push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);
        }
        commands::remap_abilities(&mut self.state, stack_id, new_id);
        if let Some(definition) = definition_for_script {
            let result = self
                .script_runtime
                .run_card_script(&mut self.state, &definition, new_id);
            self.record_script_error(new_id, &definition.script_ref, result);
        }

        Some(GameEventKind::ZoneChange {
            object: new_id,
            from: ZoneKind::Stack,
            to: destination,
            last_known_info: Some(last_known_info),
        })
    }
}
