//! CR 500-514 turn/phase/step advancement, the untap/cleanup turn-based
//! actions, and CR 514.1 cleanup-discard answers — split out of
//! `engine.rs`.

use std::collections::VecDeque;

use crate::commands;
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement::{self, ReplacementOutcome};
use crate::types::card::KeywordSet;
use crate::types::effect::EffectDuration;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::player::effective_max_hand_size;
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

use super::{DiscardError, Engine, PendingZoneMove, ProcessState, ReplacementResumeTarget, StepStatus};

impl Engine {
    /// Answers a pending CR 514.1 discard-to-hand-size choice raised by
    /// `StepStatus::AwaitingDiscard` — `cards` must be exactly the right
    /// number of cards from the active player's hand. Each card is now a
    /// real replaceable zone-change proposal (CR 616.1's "if a card would be
    /// discarded, exile it instead" pattern - e.g. Leyline of the Void) -
    /// `continue_discard_moves` drives the queue, same propose/replacement-
    /// check/commit split `Engine::continue_cost_zone_moves` established for
    /// Sacrifice/Discard cost payments (`phase5/replaceable-cleanup-
    /// discard`). Once every card's committed, `ProcessState::
    /// CollectTriggers` still runs (`PipelineExit::CleanupSbaCheck`
    /// unconditionally continues on to `ProcessState::CleanupSbaCheck` once
    /// any `DiscardsCard`/`EachDiscardsCard` trigger has had a chance to be
    /// placed) so the next `step()` decides whether CR 514.3's priority
    /// exception applies - unchanged from before this split.
    pub fn submit_discard(&mut self, cards: Vec<ObjectId>) -> Result<(), DiscardError> {
        let ProcessState::AwaitingDiscard { player, excess } = self.process_state else {
            return Err(DiscardError::NotAwaitingDiscard);
        };

        if cards.len() != excess {
            return Err(DiscardError::WrongCount { expected: excess, got: cards.len() });
        }

        // Duplicate check before any mutation.
        let mut seen: Vec<ObjectId> = Vec::with_capacity(cards.len());
        for &card in &cards {
            if seen.contains(&card) {
                return Err(DiscardError::DuplicateCard);
            }
            seen.push(card);
        }

        // Validate each card is in the active player's hand under their control.
        for &card in &cards {
            let object = self.state.object(card).ok_or(DiscardError::CardNotInHand)?;
            if object.zone != ZoneKind::Hand || object.controller != player {
                return Err(DiscardError::CardNotInHand);
            }
        }

        let since_ledger = self.state.ledger.len();
        let pending: VecDeque<PendingZoneMove> = cards
            .into_iter()
            .map(|id| PendingZoneMove { id, from: ZoneKind::Hand, to: ZoneKind::Graveyard })
            .collect();
        self.process_state = self.continue_discard_moves(pending, since_ledger);
        Ok(())
    }

    /// Drains `remaining` cleanup-discard targets one at a time - same
    /// propose/replacement-check/commit loop shape `continue_cost_zone_moves`
    /// already established for Sacrifice/Discard cost payments, reused here
    /// for `Engine::submit_discard`'s own CR 514.1 targets. `since_ledger`
    /// (captured before the first card in this discard committed) is what
    /// the eventual `ProcessState::CollectTriggers { since }` transition
    /// needs once the queue empties - carried through any CR 616.1 pause via
    /// `ReplacementResumeTarget::CleanupDiscard` so it isn't lost.
    ///
    /// `pub(super)`: `replacement_resume.rs`'s `submit_replacement_choice`
    /// also calls this (resuming a paused `ReplacementResumeTarget::
    /// CleanupDiscard`).
    pub(super) fn continue_discard_moves(
        &mut self,
        mut remaining: VecDeque<PendingZoneMove>,
        since_ledger: usize,
    ) -> ProcessState {
        while let Some(pending) = remaining.pop_front() {
            let Some(proposed) = commands::propose_zone_move(&self.state, pending.id, pending.from, pending.to) else {
                continue;
            };
            // CR 614.8: a cleanup discard is never genuine destruction - a
            // regeneration shield never intercepts one.
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                ReplacementOutcome::Resolved(event) => {
                    // CR 702.35a: must be derived before commit_zone_move
                    // purges this object's own replacement_effects entry -
                    // see madness_marker_event's own doc comment.
                    let madness = replacement::madness_marker_event(&self.state, &event);
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
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
                        resume: ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger },
                    };
                }
            }
        }
        // Route through CollectTriggers/PutTriggersApnap instead of ending
        // the pipeline here - this discard's own ZoneChange events (and any
        // DiscardsCard/EachDiscardsCard trigger they raise) need real
        // trigger collection, same as every other real zone move.
        // PipelineExit::CleanupSbaCheck (unconditional, unlike
        // PipelineExit::CleanupCheck) is what still gets to the SBA check
        // afterward regardless of whether anything triggered, matching
        // begin_cleanup_step's own discard-then-SBA-check sequencing.
        self.pipeline_exit = PipelineExit::CleanupSbaCheck;
        ProcessState::CollectTriggers { since: since_ledger }
    }

    /// The shared tail every `ProcessState::AdvanceTurnStep` step reaches
    /// once its own turn-based action (if any) is done and it's ready to
    /// open a priority window (CR 603.3/117.3b: replacement check -> SBA ->
    /// trigger collection first, same "settle everything before priority"
    /// contract `PendingAction`/`ResolveStackTop` already follow). Factored
    /// out (`phase5/replaceable-draw`) so `Step::Draw`'s own handling can
    /// reach it both on the normal path and after a CR 616.1 pause resumes
    /// (`ReplacementResumeTarget::Draw`) - `since_ledger` is a parameter
    /// rather than captured fresh here specifically so `Step::Draw` can
    /// supply one taken *before* its own draw committed (see that call
    /// site's own comment for why the ordering matters).
    ///
    /// `pub(in crate::engine)`: `replacement_resume.rs`'s
    /// `submit_replacement_choice` also calls this.
    pub(in crate::engine) fn enter_priority_via_replacement_check(
        &mut self,
        events: Vec<GameEventKind>,
        since_ledger: usize,
    ) -> ProcessState {
        self.state.priority_player = self.state.active_player;
        self.pipeline_context = Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
        self.pipeline_exit = PipelineExit::BackToPriority;
        ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() }
    }

    /// CR 502.3: untaps every battlefield permanent the active player
    /// controls, as the untap step's turn-based action. No phasing (CR
    /// 502.1 - `phased_out` stays untouched, out of scope this slice) and
    /// no "doesn't untap" query (no continuous-effects query API exists
    /// yet - same gap `PlayLand`'s land-limit TODO already flags).
    fn untap_step(&mut self) -> Vec<GameEventKind> {
        let active = self.state.active_player;
        // CR 502.3: untaps what the active player *currently* controls
        // (layers::effective_controller), not the raw field.
        let mut ids: Vec<ObjectId> = self
            .state
            .objects
            .iter()
            .filter(|(_, o)| {
                o.zone == ZoneKind::Battlefield
                    && o.tapped
                    && crate::layers::effective_controller(&self.state, &self.script_runtime, o.object_id)
                        == active
            })
            .map(|(id, _)| *id)
            .collect();
        // HashMap iteration order is unstable - sort so the ledger is
        // deterministic.
        ids.sort_by_key(|id| id.0);

        let mut events = Vec::with_capacity(ids.len());
        for id in ids {
            self.state.object_mut(id).expect("id just collected from state.objects").tapped = false;
            events.push(GameEventKind::Untapped { object: id });
        }
        events
    }

    /// CR 514.1/514.2/514.3: entry point for the cleanup step's turn-based
    /// actions. Called from `AdvanceTurnStep` when `advance_turn_step()`
    /// transitions into `Step::Cleanup`.
    ///
    /// Order: (1) drain "until end of turn" continuous effects (514.2),
    /// (2) check hand size and pause for discard if needed (514.1),
    /// (3) queue `CleanupSbaCheck` so the next `step()` decides whether
    /// priority opens (CR 514.3 exception) or the turn advances silently.
    fn begin_cleanup_step(&mut self) -> StepStatus {
        // CR 514.2: drain "until end of turn" continuous effects.
        // TODO: ReplacementEffect has no `duration` field yet - once added,
        // drain state.replacement_effects here with the same retain().
        self.state
            .continuous_effects
            .retain(|e| !matches!(e.duration, EffectDuration::UntilEndOfTurn));

        // CR 514.1: active player discards to max hand size if needed.
        let active = self.state.active_player;
        let hand_size = self
            .state
            .zone(ZoneKind::Hand, Some(active))
            .map(|z| z.contents.len())
            .unwrap_or(0);
        // CR 402.2: `None` means "no maximum hand size" - skip the discard
        // check entirely, same as if hand_size could never exceed it.
        let Some(max_hand_size) = effective_max_hand_size(&self.state, &self.state.hand_size_modifiers, active, 7) else {
            self.process_state = ProcessState::CleanupSbaCheck;
            return StepStatus::Progressed;
        };
        let max_hand_size = max_hand_size as usize;

        if hand_size > max_hand_size {
            let excess = hand_size - max_hand_size;
            self.process_state = ProcessState::AwaitingDiscard { player: active, excess };
            return StepStatus::AwaitingDiscard { player: active, hand_size, max_hand_size };
        }

        // No discard needed - proceed to the SBA check.
        self.process_state = ProcessState::CleanupSbaCheck;
        StepStatus::Progressed
    }

    /// Deterministic CR 500-514 phase/step transition table for this slice:
    /// advances `state.phase`/`step`/`turn_number`/`active_player` and
    /// returns the log events for it. Does NOT carry out any of the actual
    /// rules work a step triggers (untapping permanents, drawing a card,
    /// etc.) - that's separate, larger work still gated on other `todo!()`s
    /// (layers.rs, zone manipulation); this only answers "what step comes
    /// next."
    ///
    /// CR 505.1's "Main happens twice per turn" ambiguity (`Phase` alone
    /// can't tell which - see turn.rs's `Phase` doc) is resolved by `Step`
    /// itself now (`PreCombatMain`/`PostCombatMain`), not a separate
    /// `postcombat_main: bool` this table has to remember to flip - see
    /// turn.rs's `Step` doc for why that field was folded in here instead.
    fn advance_turn_step(&mut self) -> Vec<GameEventKind> {
        // CR 500.5/106.4: any unspent mana in every player's mana pool
        // empties as the step or phase being left ends - this fn runs
        // exactly once per real step transition, so "about to leave the
        // current step/phase" is true on every call, including the
        // PreGame -> Beginning/Untap entry (pools start empty anyway).
        for player in &mut self.state.players {
            player.mana_pool.empty();
        }

        let phase_before = self.state.phase;
        let starting_new_turn = match (self.state.phase, self.state.step) {
            (Phase::PreGame, _) => {
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                // CR 302.6/602.5a: seeds the active player's first
                // "most recent turn began at" mark - without this,
                // has_summoning_sickness's None branch would treat every
                // object as sick forever, since no rotation has happened
                // yet to set it.
                self.state
                    .last_active_turn_number
                    .insert(self.state.active_player, self.state.turn_number);
                // CR 103.8a: in a two-player game, the starting player skips
                // their draw step on turn 1. Seeded here so the general
                // consume_step_skip path handles it uniformly with any
                // card effect that also says "skip your next draw step".
                if self.state.turn_order.len() == 2 {
                    let starter = self.state.active_player;
                    self.state.pending_step_skips.push((starter, Step::Draw));
                }
                true
            }
            (Phase::Beginning, Step::Untap) => {
                self.state.step = Step::Upkeep;
                false
            }
            (Phase::Beginning, Step::Upkeep) => {
                self.state.step = Step::Draw;
                false
            }
            (Phase::Beginning, Step::Draw) => {
                self.state.phase = Phase::Main;
                self.state.step = Step::PreCombatMain;
                false
            }
            (Phase::Main, Step::PreCombatMain) => {
                self.state.phase = Phase::Combat;
                self.state.step = Step::BeginningOfCombat;
                false
            }
            (Phase::Combat, Step::BeginningOfCombat) => {
                self.state.step = Step::DeclareAttackers;
                false
            }
            (Phase::Combat, Step::DeclareAttackers) => {
              // CR 506.1: if no attackers were declared, skip to end of combat
              if self.state.combat.as_ref().map(|c| c.attackers.is_empty()).unwrap_or(true) {
                  self.state.phase = Phase::Combat;
                  self.state.step = Step::EndOfCombat;
              } else {
                self.state.step = Step::DeclareBlockers;
              }
              false
            }
            (Phase::Combat, Step::DeclareBlockers) => {
                // CR 510.1a: two combat damage steps instead of one iff some
                // attacker or blocker currently has first strike or double
                // strike - read through effective_keywords (CR 613 layer 6)
                // so a continuous effect granting the keyword counts too,
                // not just the printed KeywordSet.
                let has_first_or_double_strike = self
                    .state
                    .combat
                    .as_ref()
                    .map(|combat| {
                        combat
                            .attackers
                            .iter()
                            .map(|(attacker, _)| *attacker)
                            .chain(combat.blocks.iter().map(|(blocker, _)| *blocker))
                            .any(|id| {
                                commands::effective_keywords(&self.state, &self.script_runtime, id)
                                    .intersects(KeywordSet::FIRST_STRIKE | KeywordSet::DOUBLE_STRIKE)
                            })
                    })
                    .unwrap_or(false);
                self.state.step = if has_first_or_double_strike {
                    Step::FirstStrikeDamage
                } else {
                    Step::CombatDamage
                };
                // `phase5/automatic-combat-damage`: fresh damage step,
                // nothing assigned for it yet - resets
                // resolve_advance_turn_step_state's own auto-fire guard.
                // `pending_damage_splits` (phase5/banding-two-actor-
                // damage-split) resets alongside it - any partial splits
                // from a step that never fully resolved (shouldn't happen
                // in practice, since both steps grant priority before
                // advancing) don't leak into a different step's own
                // assignment.
                if let Some(combat) = self.state.combat.as_mut() {
                    combat.damage_assigned_this_step = false;
                    combat.pending_damage_splits.clear();
                }
                false
            }
            (Phase::Combat, Step::FirstStrikeDamage) => {
                self.state.step = Step::CombatDamage;
                // `phase5/automatic-combat-damage`: the regular damage
                // step is its own fresh assignment, independent of
                // whatever the first-strike step already resolved.
                if let Some(combat) = self.state.combat.as_mut() {
                    combat.damage_assigned_this_step = false;
                    combat.pending_damage_splits.clear();
                }
                false
            }
            (Phase::Combat, Step::CombatDamage) => {
                self.state.step = Step::EndOfCombat;
                false
            }
            (Phase::Combat, Step::EndOfCombat) => {
                self.state.phase = Phase::Main;
                self.state.step = Step::PostCombatMain;
                // CR 506.4: attacking/blocking status ends with the combat
                // phase - clear it so it doesn't leak into the next combat.
                self.state.combat = None;
                // CR 511.2/500.5a: "until end of combat" effects expire at
                // the end of the combat phase itself (this transition out of
                // it), not at the beginning of the End of Combat step.
                self.state
                    .continuous_effects
                    .retain(|e| !matches!(e.duration, EffectDuration::UntilEndOfCombat));
                false
            }
            (Phase::Main, Step::PostCombatMain) => {
                self.state.phase = Phase::Ending;
                self.state.step = Step::End;
                false
            }
            (Phase::Ending, Step::End) => {
                self.state.step = Step::Cleanup;
                // CR 514.2: all damage marked on permanents is removed as a
                // turn-based action of the cleanup step. The combat-damage
                // slice is the first real damage_marked producer in this
                // engine - without this, marked damage would persist across
                // turns and falsely re-trigger the lethal-damage SBA next
                // turn.
                for object in self.state.objects.values_mut() {
                    if object.zone == ZoneKind::Battlefield {
                        object.damage_marked = 0;
                        object.deathtouched = false;
                    }
                }
                false
            }
            (Phase::Ending, Step::Cleanup) => {
                // CR 800.4b: the next turn skips any eliminated player.
                self.state.active_player =
                    commands::next_active_player(&self.state, self.state.active_player);
                // CR 723.1-style "until your next turn": the effect lasts
                // until the beginning of its controller's own next turn, not
                // just any next turn - drain exactly the effects whose
                // source's current controller is the player who is about to
                // become active. An effect whose source no longer resolves
                // (e.g. a spell that already left the stack) can never match
                // here and is left inert rather than guessed at - same
                // stance `applicable_effects_in_layer` already takes for an
                // unresolvable source (TODO: known limitation, not fixed by
                // this slice).
                let new_active_player = self.state.active_player;
                let objects = &self.state.objects;
                self.state.continuous_effects.retain(|e| {
                    if !matches!(e.duration, EffectDuration::UntilYourNextTurn) {
                        return true;
                    }
                    match objects.get(&e.source) {
                        Some(source_obj) => source_obj.controller != new_active_player,
                        None => true,
                    }
                });
                self.state.turn_number += 1;
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                // CR 305.2's per-turn land-drop count resets with the turn.
                self.state.lands_played_this_turn.clear();
                // CR 603.2h's "only once each turn" cap resets with the turn
                // too - same reasoning, a new turn is a fresh "turn" for
                // this restriction.
                self.state.once_per_turn_triggered.clear();
                // CR 302.6/602.5a: the new active player's most recent
                // turn now begins at this turn_number.
                self.state
                    .last_active_turn_number
                    .insert(self.state.active_player, self.state.turn_number);
                true
            }
            (phase, step) => unreachable!(
                "invalid Phase/Step combination {phase:?}/{step:?} - Step values are only \
                 meaningful within the Phase they belong to (CR 500-514); reaching this means \
                 state held a combination that doesn't correspond to any real turn position"
            ),
        };

        // Only re-emit PhaseStarted when the phase actually changed - e.g.
        // Untap -> Upkeep stays within Beginning and shouldn't double-log
        // it once Untap auto-chains into Upkeep without a player action
        // (CR 502.4) in between.
        let mut events = Vec::new();
        if self.state.phase != phase_before {
            events.push(GameEventKind::PhaseStarted { phase: self.state.phase });
        }
        events.push(GameEventKind::StepStarted { step: self.state.step });
        if starting_new_turn {
            events.push(GameEventKind::TurnStarted {
                player: self.state.active_player,
                turn_number: self.state.turn_number,
            });
        }
        events
    }

    /// `ProcessState::AdvanceTurnStep` — advances one CR 500-514 step via
    /// `advance_turn_step`, then handles each step's own turn-based action
    /// (untap/draw/cleanup) before opening (or, for Untap, deliberately not
    /// opening) a priority window.
    pub(super) fn resolve_advance_turn_step_state(&mut self) -> StepStatus {
        let mut events = self.advance_turn_step();

        if self.state.step == Step::Untap {
            // CR 502.4: no priority window during untap - perform
            // its turn-based action (CR 502.3) and loop straight
            // back into AdvanceTurnStep instead of opening
            // PriorityWindow. The next call into this same arm runs
            // advance_turn_step() again (Untap -> Upkeep), which
            // does open priority normally below.
            //
            // Still routed through the real pipeline (ReplacementCheck
            // -> CommitEvents -> SbaLoop -> CollectTriggers ->
            // PutTriggersApnap), same as every other step - CR
            // 603.3's last sentence means an untap-step trigger
            // (BeginningOfYourUntap/EachUntap - rare on real cards,
            // but real) just waits to go on the stack until the next
            // time a player would receive priority, rather than
            // never triggering at all. Placing it onto state.stack
            // right here (instead of literally deferring the "put on
            // stack" action until Upkeep's priority window) is
            // functionally equivalent once that window opens - the
            // trigger is already sitting there either way.
            // PipelineExit::AdvanceTurnStep (not BackToPriority) is
            // what keeps this from opening a priority window Untap
            // isn't allowed to grant.
            let untap_events = self.untap_step();
            events.extend(untap_events);
            let since_ledger = self.state.ledger.len();
            self.pipeline_context =
                Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
            self.pipeline_exit = PipelineExit::AdvanceTurnStep;
            self.process_state =
                ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
            return StepStatus::Progressed;
        }
        if self.state.step == Step::Draw {
            // CR 500.7/500.8: consume any queued skip before doing
            // anything — a skipped step has no turn-based actions and
            // no priority window.
            let active = self.state.active_player;
            if self.state.consume_step_skip(active, Step::Draw) {
                // CR 500.8: a skipped step doesn't happen at all -
                // still log its own StepStarted marker (unchanged
                // pre-existing behavior), but no turn-based action,
                // no priority window, and no trigger collection
                // either.
                self.log_events(events);
                self.process_state = ProcessState::AdvanceTurnStep;
                return StepStatus::Progressed;
            }
            // CR 504.1/616.1: drawing is a real replaceable zone-
            // change proposal now (phase5/replaceable-draw) - same
            // propose/apply_replacements/commit split every other
            // zone-move producer in this engine uses. since_ledger
            // is captured *before* the draw's own event is logged
            // (directly, via commit_zone_move + a plain
            // state.ledger.push - bypassing the generic
            // ReplacementCheck pipeline below entirely, so the
            // already-resolved event never gets re-checked a second
            // time against a fresh, empty `excluded` set) so the
            // eventual trigger-collection window still covers both
            // the draw and this call's own TurnStarted/PhaseStarted/
            // StepStarted markers (`events`, still unlogged at this
            // point) - same property `DrawsCard`/`EachDrawsCard`
            // already depend on.
            let since_ledger = self.state.ledger.len();
            if let Some(proposed) = commands::propose_draw(&mut self.state, active) {
                match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                    ReplacementOutcome::Resolved(event) => {
                        let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        // CR 702.29b: a drawn card's own script self-registers
                        // any hand-only ability (Cycling) now that it's
                        // really in the hand zone - see run_card_script_for's
                        // own doc comment.
                        if let GameEventKind::ZoneChange { object, .. } = committed.kind {
                            self.run_card_script_for(object);
                        }
                        self.state.ledger.push(committed);
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                        self.process_state = ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event: event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::Draw { tail_events: events, since_ledger },
                        };
                        return StepStatus::Progressed;
                    }
                }
            }
            self.process_state = self.enter_priority_via_replacement_check(events, since_ledger);
            return StepStatus::Progressed;
        }
        if self.state.step == Step::Cleanup {
            // CR 514.3: normally no priority during cleanup - run
            // cleanup's turn-based actions (514.1 discard, 514.2
            // effect expiry) then check SBAs before deciding
            // whether to open a priority window or advance the turn
            // silently. begin_cleanup_step handles all of this via
            // its own CleanupSbaCheck flow (which still doesn't run
            // collect_triggers either - a second, differently-shaped
            // gap, left for its own slice per docs/implementation.md's
            // backlog item 7a).
            self.log_events(events);
            return self.begin_cleanup_step();
        }

        // `phase5/automatic-combat-damage`, CR 510.1/510.2: a combat
        // damage step's turn-based action needs a real player choice only
        // when some attacker/blocker has 2+ living blockers, a single
        // trampling blocker, or is a banding-forced multi-attacker
        // blocker (commands::execute_assign_combat_damage's own
        // `requires_assignment`) - every other case (unblocked, single
        // non-trample blocker, or no attackers survive to deal damage
        // this step) has exactly one legal assignment, so there's nothing
        // for a player to actually decide. Attempt it here with an empty
        // split before priority ever opens, same "no player action
        // needed" contract untap_step above already models for its own
        // turn-based action - Err (a real split is genuinely needed)
        // just falls through unchanged to the same explicit-submission
        // path this step has always used, priority opening exactly as
        // before. Reusing execute_assign_combat_damage directly (instead
        // of re-deriving "is a split needed") means this can never drift
        // from what AssignCombatDamage's own validation considers a real
        // choice.
        if matches!(self.state.step, Step::CombatDamage | Step::FirstStrikeDamage) {
            let active = self.state.active_player;
            if let Ok(outcome) =
                commands::execute_assign_combat_damage(&mut self.state, &self.script_runtime, active, Vec::new())
            {
                events.extend(outcome.events);
            }
        }

        // CR 603.3/117.3b: route through the real pipeline
        // (replacement check -> SBA -> trigger collection) before
        // opening the priority window that follows every step/phase
        // transition except Untap/Draw/Cleanup above - same "settle
        // everything before priority" contract PendingAction/
        // ResolveStackTop already follow. This is the fix for the
        // gap ConditionPrimitive::BeginningOfYourUpkeep's own doc
        // comment (ability.rs) names: AdvanceTurnStep used to push
        // these events via log_events, which never ran
        // collect_triggers at all.
        let since_ledger = self.state.ledger.len();
        self.process_state = self.enter_priority_via_replacement_check(events, since_ledger);
        StepStatus::Progressed
    }

    /// `ProcessState::AwaitingDiscard { player, excess }` — nothing to do
    /// until `submit_discard` answers; recomputes the same hand-size/max
    /// hand-size pair `begin_cleanup_step` derived to redisplay them (this
    /// arm only re-displays them, `excess` — fixed when this pause first
    /// started — is what `submit_discard` actually validates against).
    pub(super) fn resolve_awaiting_discard_state(&mut self, player: PlayerId, excess: usize) -> StepStatus {
        let hand_size = self
            .state
            .zone(ZoneKind::Hand, Some(player))
            .map(|z| z.contents.len())
            .unwrap_or(0);
        let max_hand_size = effective_max_hand_size(&self.state, &self.state.hand_size_modifiers, player, 7)
            .unwrap_or(u32::MAX) as usize;
        let status = StepStatus::AwaitingDiscard { player, hand_size, max_hand_size };
        self.process_state = ProcessState::AwaitingDiscard { player, excess };
        status
    }
}
