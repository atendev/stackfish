//! Turn, phase and step advancement, the untap and cleanup turn-based actions,
//! and cleanup-discard answers. See `DESIGN.md`.

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
    /// Answers the cleanup discard-to-hand-size pause (CR 514.1). `cards` must be
    /// exactly the excess, each a replaceable zone change (CR 616.1), after which
    /// `CollectTriggers` still runs so the next `step()` can apply CR 514.3.
    pub fn submit_discard(&mut self, cards: Vec<ObjectId>) -> Result<(), DiscardError> {
        let ProcessState::AwaitingDiscard { player, excess } = self.process_state else {
            return Err(DiscardError::NotAwaitingDiscard);
        };

        if cards.len() != excess {
            return Err(DiscardError::WrongCount { expected: excess, got: cards.len() });
        }

        let mut seen: Vec<ObjectId> = Vec::with_capacity(cards.len());
        for &card in &cards {
            if seen.contains(&card) {
                return Err(DiscardError::DuplicateCard);
            }
            seen.push(card);
        }

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

    /// Drains the cleanup-discard queue one card at a time. `since_ledger` is
    /// carried through any ordering pause (CR 616.1) for the trigger window.
    pub(super) fn continue_discard_moves(
        &mut self,
        mut remaining: VecDeque<PendingZoneMove>,
        since_ledger: usize,
    ) -> ProcessState {
        while let Some(pending) = remaining.pop_front() {
            let Some(proposed) = commands::propose_zone_move(&self.state, pending.id, pending.from, pending.to) else {
                continue;
            };
            // CR 614.8: a cleanup discard is never destruction.
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                ReplacementOutcome::Resolved(event) => {
                    // CR 702.35a: derived before commit purges the object's replacement effects.
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
        // The discard's zone changes need real trigger collection. `CleanupSbaCheck`
        // reaches the SBA check whether or not anything triggered.
        self.pipeline_exit = PipelineExit::CleanupSbaCheck;
        ProcessState::CollectTriggers { since: since_ledger }
    }

    /// The tail every step reaches before opening priority: replacement check,
    /// SBAs, trigger collection (CR 117.3b, 603.3). `since_ledger` is a parameter
    /// so the draw step can pass one taken before its draw committed.
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

    /// CR 502.3: untaps what the active player controls.
    // TODO: phasing (CR 502.1) and "doesn't untap" effects
    fn untap_step(&mut self) -> Vec<GameEventKind> {
        let active = self.state.active_player;
        // CR 502.3: uses the effective controller, not the raw field.
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
        // HashMap order is unstable; sort so the ledger is deterministic.
        ids.sort_by_key(|id| id.0);

        let mut events = Vec::with_capacity(ids.len());
        for id in ids {
            self.state.object_mut(id).expect("id just collected from state.objects").tapped = false;
            events.push(GameEventKind::Untapped { object: id });
        }
        events
    }

    /// CR 514: cleanup turn-based actions. Expires "until end of turn" effects
    /// (CR 514.2), pauses for a hand-size discard (CR 514.1), then queues
    /// `CleanupSbaCheck` to decide whether priority opens (CR 514.3).
    fn begin_cleanup_step(&mut self) -> StepStatus {
        // CR 514.2
        // TODO: ReplacementEffect has no `duration`; drain `replacement_effects` here once it does
        self.state
            .continuous_effects
            .retain(|e| !matches!(e.duration, EffectDuration::UntilEndOfTurn));

        let active = self.state.active_player;
        let hand_size = self
            .state
            .zone(ZoneKind::Hand, Some(active))
            .map(|z| z.contents.len())
            .unwrap_or(0);
        // CR 402.2: no maximum hand size means no discard.
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

        self.process_state = ProcessState::CleanupSbaCheck;
        StepStatus::Progressed
    }

    /// CR 500-514 phase and step transition table. Advances the turn position and
    /// returns its log events; the rules work each step triggers happens elsewhere.
    fn advance_turn_step(&mut self) -> Vec<GameEventKind> {
        // CR 500.5, 106.4: mana empties as each step or phase ends.
        for player in &mut self.state.players {
            player.mana_pool.empty();
        }

        let phase_before = self.state.phase;
        let starting_new_turn = match (self.state.phase, self.state.step) {
            (Phase::PreGame, _) => {
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                // CR 302.6: seeds the active player's turn mark, else every object stays summoning sick.
                self.state
                    .last_active_turn_number
                    .insert(self.state.active_player, self.state.turn_number);
                // CR 103.8a: queued as a step skip so it shares the path of card-driven skips.
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
              // CR 506.1
              if self.state.combat.as_ref().map(|c| c.attackers.is_empty()).unwrap_or(true) {
                  self.state.phase = Phase::Combat;
                  self.state.step = Step::EndOfCombat;
              } else {
                self.state.step = Step::DeclareBlockers;
              }
              false
            }
            (Phase::Combat, Step::DeclareBlockers) => {
                // CR 510.1a: read through effective keywords so granted keywords count.
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
                // Fresh damage step: reset the auto-fire guard and any partial banding
                // splits so they can't leak into another step's assignment.
                if let Some(combat) = self.state.combat.as_mut() {
                    combat.damage_assigned_this_step = false;
                    combat.pending_damage_splits.clear();
                }
                false
            }
            (Phase::Combat, Step::FirstStrikeDamage) => {
                self.state.step = Step::CombatDamage;
                // The regular step is assigned independently of the first-strike step.
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
                // CR 506.4
                self.state.combat = None;
                // CR 511.2: "until end of combat" ends with the phase, not the End of Combat step.
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
                // CR 514.2: marked damage clears so it can't re-trigger the lethal-damage check next turn.
                for object in self.state.objects.values_mut() {
                    if object.zone == ZoneKind::Battlefield {
                        object.damage_marked = 0;
                        object.deathtouched = false;
                    }
                }
                false
            }
            (Phase::Ending, Step::Cleanup) => {
                // CR 800.4b
                self.state.active_player =
                    commands::next_active_player(&self.state, self.state.active_player);
                // "Until your next turn" drains effects whose source's controller is about
                // to become active.
                // TODO: an effect whose source no longer resolves is left inert
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
                // CR 305.2
                self.state.lands_played_this_turn.clear();
                // CR 603.2h: the once-per-turn cap resets with the turn.
                self.state.once_per_turn_triggered.clear();
                // CR 302.6
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

        // Untap to upkeep stays within the beginning phase and must not re-log `PhaseStarted`.
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

    /// Advances one step, performs its turn-based action, then opens a priority
    /// window (none for untap, CR 502.4).
    pub(super) fn resolve_advance_turn_step_state(&mut self) -> StepStatus {
        let mut events = self.advance_turn_step();

        if self.state.step == Step::Untap {
            // CR 502.4: untap grants no priority. It still runs the full pipeline so an
            // untap trigger is collected and waits on the stack (CR 603.3); the exit
            // keeps the next step from opening a window.
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
            // CR 500.7: a skipped step has no turn-based actions and no priority window.
            let active = self.state.active_player;
            if self.state.consume_step_skip(active, Step::Draw) {
                // CR 500.8: still logs `StepStarted`, but nothing else happens.
                self.log_events(events);
                self.process_state = ProcessState::AdvanceTurnStep;
                return StepStatus::Progressed;
            }
            // CR 504.1, 616.1: the draw is a replaceable proposal that commits directly,
            // bypassing `ReplacementCheck` so it isn't re-checked. `since_ledger` is taken
            // before it so trigger collection also covers the step markers.
            let since_ledger = self.state.ledger.len();
            if let Some(proposed) = commands::propose_draw(&mut self.state, active) {
                match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                    ReplacementOutcome::Resolved(event) => {
                        let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        // CR 702.29b: a hand-only ability registers when the card reaches hand.
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
            // CR 514.3: cleanup normally grants no priority; `begin_cleanup_step` decides.
            // TODO: collect triggers when cleanup has no discard (CR 514.3a)
            self.log_events(events);
            return self.begin_cleanup_step();
        }

        // CR 510.1: a damage step needs a player choice only when
        // `execute_assign_combat_damage` says a split is required. Otherwise the one
        // legal assignment is made here before priority. Reusing that command keeps
        // this from drifting from its own validation.
        if matches!(self.state.step, Step::CombatDamage | Step::FirstStrikeDamage) {
            let active = self.state.active_player;
            if let Ok(outcome) =
                commands::execute_assign_combat_damage(&mut self.state, &self.script_runtime, active, Vec::new())
            {
                events.extend(outcome.events);
            }
        }

        // CR 117.3b, 603.3: settle replacements, SBAs and triggers before priority.
        let since_ledger = self.state.ledger.len();
        self.process_state = self.enter_priority_via_replacement_check(events, since_ledger);
        StepStatus::Progressed
    }

    /// Redisplays the hand-size pair; `excess`, fixed when the pause began, is
    /// what `submit_discard` validates against.
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
