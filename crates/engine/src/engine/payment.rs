//! CR 601.2f-h cost-payment answers (`submit_payment`/`unwind_payment`) and
//! the `Sacrifice`/`Discard` zone-move queue they drive — split out of
//! `engine.rs`.

use std::collections::VecDeque;

use crate::commands::{self, PaymentResolution, PendingPaymentRequest};
use crate::replacement::{self, ReplacementOutcome};
use crate::types::cost::{AdditionalCost, PaymentPlan};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::stack::StackEntryKind;
use crate::types::zone::ZoneKind;

use super::{Engine, PaymentError, PendingZoneMove, ProcessState, ReplacementResumeTarget, SuspendedPayment};

/// CR 702.29c/f: builds the `GameEventKind::Cycled` marker for a just-
/// resolved cost zone-move `event`, if it's the one payment `cycling_source`
/// names (`Engine::submit_payment`'s own `AdditionalCost::DiscardSelf`
/// check) - `None` for every other zone-moving payment in the same cost
/// (e.g. a `Sacrifice` paid alongside a `DiscardSelf`), and for every
/// non-Cycling ability entirely. Shared by `continue_cost_zone_moves`'s own
/// loop and `replacement_resume.rs`'s `CostZoneMove` resume arm, which
/// independently commits the one item that raised a CR 616.1 ordering
/// choice before re-entering that loop for the rest.
pub(super) fn cycled_marker_event(event: &GameEventKind, cycling_source: Option<ObjectId>) -> Option<GameEventKind> {
    let GameEventKind::ZoneChange { object, last_known_info: Some(lki), .. } = event else {
        return None;
    };
    if Some(*object) != cycling_source {
        return None;
    }
    Some(GameEventKind::Cycled { object: *object, last_known_info: lki.clone() })
}

impl Engine {
    /// Answers a pending CR 601.2f-h cost payment raised by
    /// `StepStatus::AwaitingPayment` — commits `plan` (deducts mana,
    /// records `StackObject.costs_paid`) only if it validates against the
    /// pending `CostSpec`; otherwise rejects without touching state, same
    /// no-partial-payment guarantee `PaymentPlan`'s own doc comment
    /// describes. Separate entry point from `submit_action` for the same
    /// reason `submit_replacement_choice`/`submit_trigger_order` are: this
    /// is a sub-decision mid-pipeline, not a `PlayerAction`/priority
    /// decision.
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
        // CR 702.29c/f: this cost's own AdditionalCost::DiscardSelf, if any,
        // is always paid by discarding `source` itself (PaymentPlan::
        // validate's own check) - so a Cycling/Typecycling ability's own
        // discard cost is identifiable here, before the general Sacrifice/
        // Discard/Exile queue below loses track of which AdditionalCost each
        // payment answered. `None` for every other ability (including a
        // plain Discard{filter} cost, which never discards `source`
        // specifically unless that's what the payer separately chose - not
        // "cycling" either way, since the ability itself isn't templated as
        // Cycling).
        let cycling_source = {
            let ProcessState::AwaitingPayment { cost, .. } = &self.process_state else {
                unreachable!("checked above");
            };
            cost.additional.iter().any(|c| matches!(c, AdditionalCost::DiscardSelf)).then_some(source)
        };
        let ProcessState::AwaitingPayment { stack_id, on_paid, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 118.3a/118.13a/602.2f-h: commit exactly what the plan declared.
        // Mana/tap/life are immediate (never replaceable in this engine) -
        // see commands::commit_payment's doc comment for why this trusts
        // validate's check rather than re-deriving anything. Sacrifice/
        // Discard are collected separately, *not* committed yet - CR
        // 601.2h/616.1 makes each one its own replaceable zone-change
        // proposal (see commands::propose_zone_move's doc comment),
        // driven below by continue_cost_zone_moves.
        let pending_zone_moves: VecDeque<PendingZoneMove> = commands::collect_cost_zone_moves(&plan.payments)
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

        // Resume exactly where CommitEvents would otherwise have gone had
        // pending_payment been None (see that arm's doc comment) - the rest
        // of this pipeline run (SBA/triggers/exit) is unaffected by this
        // pause, since pipeline_context/pipeline_exit were already decided
        // before the pause happened. continue_cost_zone_moves logs CostPaid
        // itself once every zone-moving payment (if any) has committed, and
        // returns SbaLoop the same way this used to unconditionally.
        self.process_state = self.continue_cost_zone_moves(pending_zone_moves, payer, stack_id, on_paid, cycling_source);
        Ok(())
    }

    /// Drains `remaining` `Sacrifice`/`Discard` cost-payment targets one at a
    /// time - CR 616.1's ordering-choice pause can be raised by any of them,
    /// not just the first, since each independently proposes+replacement-
    /// checks+commits its own `ZoneChange` (`commands::propose_zone_move`/
    /// `commit_zone_move`). Same shape `continue_scripted_zone_move`
    /// already established for the scripted Destroy/Sacrifice/Exile ops -
    /// `departed_triggered_abilities`/`state.ledger` are updated inline as
    /// each target commits, rather than accumulated and flushed at the end
    /// (CR 603.10: a departed "when this dies" ability must be capturable
    /// before `CollectTriggers` ever runs, same reasoning that function's own
    /// doc comment already gives).
    ///
    /// Once the queue empties (including the trivial case of no zone-moving
    /// payments at all), applies `on_paid` (see `PaymentResolution`'s own doc
    /// comment - a cost-bearing mana ability's units are added to its
    /// controller's pool here, since no `StackObject` exists for it to
    /// resolve later), logs the final `CostPaid` event, and returns
    /// `ProcessState::SbaLoop` - exactly what `submit_payment` used to do
    /// unconditionally before this split existed.
    ///
    /// `pub(super)`: `replacement_resume.rs`'s `submit_replacement_choice`
    /// also calls this (resuming a paused `ReplacementResumeTarget::
    /// CostZoneMove`).
    pub(super) fn continue_cost_zone_moves(
        &mut self,
        mut remaining: VecDeque<PendingZoneMove>,
        payer: PlayerId,
        stack_id: ObjectId,
        on_paid: PaymentResolution,
        cycling_source: Option<ObjectId>,
    ) -> ProcessState {
        while let Some(pending) = remaining.pop_front() {
            let Some(proposed) = commands::propose_zone_move(&self.state, pending.id, pending.from, pending.to) else {
                // Already gone somehow (shouldn't happen - PaymentPlan::
                // validate confirmed it) - skip rather than guess, same
                // stance continue_scripted_zone_move's own analogous check
                // takes.
                continue;
            };
            // CR 614.8: neither Sacrifice nor Discard is genuine destruction
            // - a regeneration shield never intercepts either.
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                ReplacementOutcome::Resolved(event) => {
                    let cycled = cycled_marker_event(&event, cycling_source);
                    // CR 702.35a: must be derived before commit_zone_move
                    // purges this object's own replacement_effects entry -
                    // see madness_marker_event's own doc comment.
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
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id, on_paid, cycling_source },
                    };
                }
            }
        }
        // CR 605.1a/605.3b: a cost-bearing mana ability never reaches the
        // stack - once its (possibly sacrifice/discard-involving) cost has
        // fully committed above, its mana is added directly here instead of
        // waiting for a StackObject that was never pushed.
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
                kind: GameEventKind::ManaAdded { player: *controller, units: units.clone() },
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
        // CR 601.2i: `stack_id` has just become cast (every step of 601.2a-h
        // - moved to the stack, costs paid - is now complete) if it's a real
        // spell, not an activated ability paying its own cost (both share
        // `PaymentResolution::StackObject`) - see `GameEventKind::SpellCast`'s
        // own doc comment for why this is a dedicated marker rather than
        // reusing `CostPaid` itself (an ability's own CostPaid isn't a cast).
        if self.state.stack.iter().any(|s| s.stack_id == stack_id && matches!(s.kind, StackEntryKind::Spell)) {
            self.log_events(vec![GameEventKind::SpellCast { object: stack_id }]);
        }
        ProcessState::SbaLoop
    }

    /// CR 601.2g/605.3a: the shared check both `Engine::end_pipeline_run`'s
    /// `PipelineExit::BackToPriority` arm and `handle_pending_action`'s
    /// error arm make before falling back to their own old unconditional
    /// behavior (`fallback`, `PriorityWindow` at both current call sites).
    /// A nested mana-ability activation accepted by `submit_action` while
    /// mid-payment (see that method's own doc comment) pushed the outer
    /// `AwaitingPayment` it interrupted onto `suspended_payments` before
    /// queuing itself as a fresh `PendingAction` - once that nested action's
    /// own pipeline run fully settles (or errors out before ever really
    /// starting), popping here resumes the outer payment exactly where it
    /// left off, restoring the `pipeline_context`/`pipeline_exit` the
    /// nested run's own `handle_pending_action` call overwrote (see
    /// `Engine::pending_payment`'s sibling `suspended_payments` doc comment
    /// for why those two specifically have to travel with the pause).
    /// Nothing suspended (the overwhelmingly common case - no nesting in
    /// play) just returns `fallback` unchanged.
    pub(super) fn resume_suspended_payment_or(&mut self, fallback: ProcessState) -> ProcessState {
        let Some(SuspendedPayment { request, pipeline_context, pipeline_exit }) =
            self.suspended_payments.pop()
        else {
            return fallback;
        };
        self.pipeline_context = pipeline_context;
        self.pipeline_exit = pipeline_exit;
        let PendingPaymentRequest { stack_id, payer, cost, source, on_paid } = request;
        ProcessState::AwaitingPayment { stack_id, payer, cost, source, on_paid }
    }

    /// Answers a pending CR 601.2f-h payment pause by abandoning the cast/
    /// activation entirely (CR 601.6: "the game returns to the moment before
    /// the casting of that spell was proposed") — the caller's explicit
    /// declaration that they cannot or will not pay, distinct from
    /// resubmitting a different `PaymentPlan` via `submit_payment` (which
    /// stays valid and keeps the pause open on `PaymentError::InvalidPlan`).
    /// Without this, a cost with no valid plan at all leaves the engine
    /// stuck in `AwaitingPayment` forever — nothing else returns the card to
    /// hand or un-pushes the stack object.
    ///
    /// Not a true CR 733 state rewind (this engine's ledger is append-only,
    /// no undo primitive exists anywhere) — reuses `move_resolving_spell`
    /// (the same Stack-departure helper CR 608.2b's fizzle path already
    /// uses) to return a `Spell`'s card to hand under a fresh CR 400.7 id;
    /// an `ActivatedAbility`'s stack entry is just removed, since no card
    /// left any zone to activate it — mirrors `Fizzled`'s own "a fizzled
    /// ability just stops existing, no `ZoneChange`" stance exactly.
    /// `TriggeredAbility`/`Copy` never reach `AwaitingPayment` (the former
    /// never carries a cost, the latter isn't modeled) — not handled
    /// specially, same as `ActivatedAbility`. Either way,
    /// `GameEventKind::CastingUnwound` marks the ledger so a replay viewer
    /// can see this happened even when no `ZoneChange` accompanies it.
    pub fn unwind_payment(&mut self) -> Result<(), PaymentError> {
        let ProcessState::AwaitingPayment { stack_id, .. } = &self.process_state else {
            return Err(PaymentError::NotAwaitingPayment);
        };
        let stack_id = *stack_id;

        let Some(idx) = self.state.stack.iter().position(|s| s.stack_id == stack_id) else {
            // Defensive: shouldn't happen - nothing else removes a
            // StackObject while AwaitingPayment is pending for it.
            self.process_state = self.end_pipeline_run();
            return Ok(());
        };
        let stack_object = self.state.stack.remove(idx);

        let mut events = Vec::new();
        if matches!(stack_object.kind, StackEntryKind::Spell) {
            if let Some(event) = self.move_resolving_spell(stack_id, ZoneKind::Hand, false, false) {
                events.push(event);
            }
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

    /// CR 608.2m (normal resolution) and CR 608.2b (fizzle): moves a
    /// resolving spell's card off the stack into `destination`, minting a
    /// fresh `ObjectId` (CR 400.7) the same way every other zone change
    /// does. Pulled out so `ResolveStackTop`'s normal-resolution branch and
    /// its CR 608.2b fizzle branch (below) share one zone-move
    /// implementation instead of two copies that could drift apart -
    /// `destination`/`run_script` are the only two things that differ
    /// between them (battlefield-vs-graveyard, and whether a permanent's
    /// card script runs). Returns `None` only in the defensive case where
    /// `stack_id`'s `GameObject` is already gone (shouldn't happen - nothing
    /// else removes it while still on the stack).
    ///
    /// `pub(in crate::engine)`: `mod.rs`'s still-inlined `ResolveStackTop`
    /// arm also calls this directly for its own normal-resolution and
    /// fizzle branches.
    pub(in crate::engine) fn move_resolving_spell(
        &mut self,
        stack_id: ObjectId,
        destination: ZoneKind,
        run_script: bool,
        // CR 702.138b: `true` only from `resolve_permanent_entry`'s real
        // entry call, carrying the resolving `StackObject::escaped` flag -
        // `false` from every other call site (returning to hand on CR
        // 601.6 unwind, or a fizzle/instant/sorcery landing in the
        // graveyard/exile - none of those are "entering the battlefield").
        entered_via_escape: bool,
    ) -> Option<GameEventKind> {
        let spell_object = self.state.object(stack_id)?;
        let last_known_info = spell_object.clone();
        // Battlefield and Exile are both shared zones (owner: None) - only
        // Graveyard (the other destination this function is ever called
        // with) is per-player.
        let destination_owner = match destination {
            ZoneKind::Battlefield | ZoneKind::Exile => None,
            _ => Some(last_known_info.owner),
        };

        let new_id = self.state.allocate_object_id();
        let mut new_object = last_known_info.clone();
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage { previous: Some(stack_id) };
        new_object.zone = destination;
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 302.6/602.5a: only a permanent actually entering the
        // battlefield is summoning sick - a fizzled/graveyard-bound card
        // was never in play.
        if destination == ZoneKind::Battlefield {
            new_object.controlled_since_turn = self.state.turn_number;
            // CR 702.138b: this incarnation's own provenance - always
            // re-derived fresh here, never carried forward from
            // `last_known_info` (a stack object's own prior value is
            // meaningless; only the caller's real `escaped` flag matters).
            new_object.entered_via_escape = entered_via_escape;
        }

        self.state.objects.remove(&stack_id);
        // Captured before `new_object` moves into `objects.insert` below -
        // `run_script` is `false` for a fizzle (CR 608.2b: it never
        // resolves, so no ETB self-registration either) and for any
        // non-battlefield destination.
        let definition_for_script = if run_script { Some(new_object.definition.clone()) } else { None };
        if let Some(zone) = self.state.zone_mut(destination, destination_owner) {
            zone.push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);
        }
        commands::remap_abilities(&mut self.state, stack_id, new_id);
        if let Some(definition) = definition_for_script {
            // TODO: a script error here is silently dropped, same gap
            // ResolveStackTop's operation-invocation path already flags (no
            // error surface from ResolveStackTop back to a StepStatus yet).
            let _ = self.script_runtime.run_card_script(&mut self.state, &definition, new_id);
        }

        Some(GameEventKind::ZoneChange {
            object: new_id,
            from: ZoneKind::Stack,
            to: destination,
            last_known_info: Some(last_known_info),
        })
    }
}
