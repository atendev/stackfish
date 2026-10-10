//! CR 616.1 replacement-ordering-choice answers, CR 704.5j/903.9a SBA-driven
//! choice answers, the `SbaLoop`/`CleanupSbaCheck` pipeline stages, and the
//! scripted zone-move/draw resume helpers they all share — split out of
//! `engine.rs`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::replacement::{self, ReplacementOutcome};
use crate::sba::{self, SbaOutcome};
use crate::script::ScriptZoneMoveKind;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::mana::ColorSet;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::LinkedExile;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, Target};
use crate::types::zone::ZoneKind;

use super::{
    CommanderReturnChoiceError, Engine, LegendRuleChoiceError, ProcessState, ReplacementChoiceError,
    ReplacementResumeTarget, SbaResumeThen, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus,
};

impl Engine {
    /// Answers a pending CR 616.1 ordering choice raised by
    /// `StepStatus::AwaitingReplacementChoice` — `chosen` must be one of
    /// that status's `candidates` (a `replacement::ReplacementCandidate` —
    /// a real registered effect or CR 702.16e protection). Deliberately a
    /// separate entry point from `submit_action`: this isn't a
    /// `PlayerAction`/priority decision, it's a sub-decision mid-pipeline,
    /// so it can't go through `submit_action`'s `PriorityWindow`-only gate.
    pub fn submit_replacement_choice(
        &mut self,
        chosen: replacement::ReplacementCandidate,
    ) -> Result<(), ReplacementChoiceError> {
        let ProcessState::AwaitingReplacementChoice { candidates, .. } = &self.process_state
        else {
            return Err(ReplacementChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(ReplacementChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingReplacementChoice { pending_event, mut excluded, resume, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // Apply the chosen effect, exclude it from matching this same event
        // again (CR 616.1), then re-run resolution on the result (CR 616's
        // stabilization requirement - the rewritten form may match yet
        // another effect, including needing a further ordering choice of
        // its own) - carrying `excluded` forward rather than starting a
        // fresh exclusion set, so an effect already applied before this
        // pause can't reapply to the same event after it. `chosen` names
        // either a real registered effect or CR 702.16e protection -
        // apply_one_replacement_or_protection dispatches on the
        // `ReplacementCandidate` variant directly, same as
        // apply_replacements_from's own single-match branch.
        let rewritten = replacement::apply_one_replacement_or_protection(&mut self.state, chosen, pending_event);
        excluded.push(chosen);
        // is_destroy is per-resume-target: a ReplacementCheck-originated
        // event is never a destroy proposal (see replacement.rs's own doc
        // comment on why); an SbaDeath resume carries its own paused-event
        // tag forward (sba::DyingCandidate's own doc comment).
        let is_destroy = match &resume {
            ReplacementResumeTarget::ReplacementCheck { .. } => false,
            ReplacementResumeTarget::SbaDeath { is_destroy, .. } => *is_destroy,
            ReplacementResumeTarget::ScriptedZoneMove { kind, .. } => *kind == ScriptedZoneMoveKind::Destroy,
            // CR 614.8: a Sacrifice/Discard cost payment is never genuine
            // destruction (Sacrifice/Discard are each their own zone-change
            // verb) - a regeneration shield never intercepts one.
            ReplacementResumeTarget::CostZoneMove { .. } => false,
            // CR 614.8: cleanup discard isn't destruction either.
            ReplacementResumeTarget::CleanupDiscard { .. } => false,
            // CR 614.8: "put it into your graveyard" (CR 702.35a's decline
            // branch) is its own zone-change verb, not destruction.
            ReplacementResumeTarget::MadnessDecline { .. } => false,
            // CR 614.8: drawing a card isn't destruction either.
            ReplacementResumeTarget::Draw { .. } => false,
            // CR 614.8: a scripted DrawCards(n) ability is also just a draw,
            // not destruction.
            ReplacementResumeTarget::ScriptedDraw { .. } => false,
            // CR 614.8: a targeted CR 701.9 discard isn't destruction either.
            ReplacementResumeTarget::TargetedDiscard { .. } => false,
            // CR 614.8: a would-be Battlefield destination redirect is a
            // ZoneChange proposal, never a destroy proposal - same reasoning
            // as ReplacementCheck above (this is that exact same kind of
            // proposal, just paused one level earlier, before
            // move_resolving_spell ever runs).
            ReplacementResumeTarget::PermanentEntry { .. } => false,
        };
        let outcome =
            replacement::apply_replacements_from(&mut self.state, &self.script_runtime, rewritten, is_destroy, excluded);

        self.process_state = match resume {
            ReplacementResumeTarget::ReplacementCheck { remaining, mut checked } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    checked.push(event);
                    ProcessState::ReplacementCheck { remaining, checked }
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ReplacementCheck { remaining, checked },
                    }
                }
            },
            ReplacementResumeTarget::SbaDeath { pending_deaths, then, .. } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let committed_pairs = sba::commit_death_or_regeneration(&mut self.state, event);
                    let (committed, departed_lists): (Vec<_>, Vec<_>) = committed_pairs.into_iter().unzip();
                    let departed: Vec<_> = departed_lists.into_iter().flatten().collect();
                    self.resume_sba_death(pending_deaths, committed, departed, then)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::SbaDeath { pending_deaths, is_destroy, then },
                    }
                }
            },
            ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, then, mut trailing } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    trailing.extend(self.commit_scripted_zone_move(kind, event));
                    self.continue_scripted_zone_move(kind, remaining, then, trailing)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, then, trailing },
                    }
                }
            },
            ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id, on_paid, cycling_source } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let cycled = super::payment::cycled_marker_event(&event, cycling_source);
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
                    self.continue_cost_zone_moves(remaining, payer, stack_id, on_paid, cycling_source)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id, on_paid, cycling_source },
                    }
                }
            },
            ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let madness = replacement::madness_marker_event(&self.state, &event);
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    if let Some(kind) = madness {
                        self.log_events(vec![kind]);
                    }
                    self.state.ledger.push(committed);
                    self.continue_discard_moves(remaining, since_ledger)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger },
                    }
                }
            },
            ReplacementResumeTarget::MadnessDecline { card, stack_id, mut events_so_far } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                    events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                    ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::MadnessDecline { card, stack_id, events_so_far },
                    }
                }
            },
            ReplacementResumeTarget::Draw { tail_events, since_ledger } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    // CR 702.29b: same hand-entry self-registration as the
                    // unpaused draw path (turn.rs) - this is its CR 616.1
                    // ordering-choice-delayed sibling.
                    if let GameEventKind::ZoneChange { object, .. } = committed.kind {
                        self.run_card_script_for(object);
                    }
                    self.state.ledger.push(committed);
                    self.enter_priority_via_replacement_check(tail_events, since_ledger)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::Draw { tail_events, since_ledger },
                    }
                }
            },
            ReplacementResumeTarget::ScriptedDraw { remaining, player, stack_id, events_so_far } => {
                match outcome {
                    ReplacementOutcome::Resolved(event) => {
                        let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        // CR 702.29b: same hand-entry self-registration as
                        // continue_draw_cards' own unpaused commit below.
                        if let GameEventKind::ZoneChange { object, .. } = committed.kind {
                            self.run_card_script_for(object);
                        }
                        self.state.ledger.push(committed);
                        self.continue_draw_cards(remaining, player, stack_id, events_so_far)
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                        ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event: event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::ScriptedDraw {
                                remaining,
                                player,
                                stack_id,
                                events_so_far,
                            },
                        }
                    }
                }
            }
            ReplacementResumeTarget::TargetedDiscard { player, remaining_count, stack_id, events_so_far } => {
                match outcome {
                    ReplacementOutcome::Resolved(event) => {
                        // CR 702.35a: same Madness-compatibility as every
                        // other real discard producer - see
                        // discard::continue_targeted_discard's own doc
                        // comment.
                        let madness = replacement::madness_marker_event(&self.state, &event);
                        let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        if let Some(kind) = madness {
                            self.log_events(vec![kind]);
                        }
                        self.state.ledger.push(committed);
                        self.finish_or_continue_discard(player, remaining_count, stack_id, events_so_far)
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                        ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event: event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::TargetedDiscard {
                                player,
                                remaining_count,
                                stack_id,
                                events_so_far,
                            },
                        }
                    }
                }
            }
            ReplacementResumeTarget::PermanentEntry { top, legal_targets, events } => match outcome {
                ReplacementOutcome::Resolved(GameEventKind::ZoneChange { to, .. }) => {
                    match self.continue_permanent_entry(top, &legal_targets, events, to) {
                        ControlFlow::Break(process_state) => process_state,
                        ControlFlow::Continue((top, mut events)) => {
                            events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
                            ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() }
                        }
                    }
                }
                // Defensive - same "a WouldMoveZone candidate's own matching
                // payloads only ever rewrite it back into a ZoneChange"
                // reasoning resolve_permanent_entry's own fresh-path check
                // already documents; unreachable in practice.
                ReplacementOutcome::Resolved(_) => ProcessState::ReplacementCheck {
                    remaining: VecDeque::from([GameEventKind::StackObjectResolved { stack_id: top.stack_id }]),
                    checked: Vec::new(),
                },
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::PermanentEntry { top, legal_targets, events },
                    }
                }
            },
        };
        Ok(())
    }

    /// Answers a pending CR 704.5j legend-rule choice raised by
    /// `StepStatus::AwaitingLegendRuleChoice` — `kept` must be one of that
    /// status's `candidates` (the legendary permanent this player keeps).
    /// Deliberately a separate entry point from `submit_action`, same reason
    /// `submit_replacement_choice` is.
    ///
    /// Every other candidate is put into `pending_deaths` and resolved via
    /// the same `resume_sba_death` helper `submit_replacement_choice` already
    /// uses for `ReplacementResumeTarget::SbaDeath` — the losers' actual
    /// destruction is ordinary SBA-driven death (real replacement checking,
    /// CR 400.7 new object id, `retire_object`), not bespoke logic. A
    /// replacement effect on one of the losers (e.g. "if this would die,
    /// exile it instead") correctly re-pauses on `AwaitingReplacementChoice`
    /// via that same helper, rather than being silently skipped.
    pub fn submit_legend_rule_choice(&mut self, kept: ObjectId) -> Result<(), LegendRuleChoiceError> {
        let ProcessState::AwaitingLegendRuleChoice { candidates, .. } = &self.process_state else {
            return Err(LegendRuleChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&kept) {
            return Err(LegendRuleChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingLegendRuleChoice { candidates, then, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 704.5j is "put into its owner's graveyard," not "destroy" -
        // never interceptable by a regeneration shield, same as 704.5f/k
        // (see sba::DyingCandidate's own doc comment).
        let losers: VecDeque<sba::DyingCandidate> = candidates
            .into_iter()
            .filter(|id| *id != kept)
            .map(|id| sba::DyingCandidate { id, is_destroy: false })
            .collect();
        self.process_state = self.resume_sba_death(losers, Vec::new(), Vec::new(), then);
        Ok(())
    }

    /// Answers a pending CR 903.9a commander-return choice raised by
    /// `StepStatus::AwaitingCommanderReturnChoice`. `returned = true`
    /// performs the zone move (`sba::move_commander_to_command_zone`);
    /// `false` declines, leaving the card exactly where it is. Either way,
    /// the commander is marked in `GameState::commander_return_offered` so
    /// this exact arrival isn't re-asked about on a later pass.
    ///
    /// Deliberately a separate entry point from `submit_action`, same
    /// reason `submit_legend_rule_choice` is. Doesn't route through
    /// `resume_sba_death`/`resolve_deaths` like that method does — there's
    /// no "death" here to replacement-check, just a direct zone move (see
    /// `move_commander_to_command_zone`'s own doc comment for why that move
    /// itself skips CR 614-616 replacement checking).
    pub fn submit_commander_return_choice(
        &mut self,
        returned: bool,
    ) -> Result<(), CommanderReturnChoiceError> {
        if !matches!(self.process_state, ProcessState::AwaitingCommanderReturnChoice { .. }) {
            return Err(CommanderReturnChoiceError::NotAwaitingChoice);
        }
        let ProcessState::AwaitingCommanderReturnChoice { commander, then, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        self.state.commander_return_offered.insert(commander);
        let mut events = Vec::new();
        if returned {
            if let Some(event) = sba::move_commander_to_command_zone(&mut self.state, commander) {
                events.push(event);
            }
        }

        self.process_state = match then {
            SbaResumeThen::SbaLoop => self.finish_sba_loop(events, Vec::new()),
            SbaResumeThen::CleanupSbaCheck { since_ledger } => {
                self.finish_cleanup_sba_check(events, Vec::new(), since_ledger)
            }
        };
        Ok(())
    }

    /// `ProcessState::SbaLoop` — CR 704.3's fixed-point loop: runs one SBA
    /// pass and either re-enters `SbaLoop` (something happened) or moves on
    /// to `CollectTriggers` (nothing did), via `finish_sba_loop`. Pauses on
    /// `AwaitingReplacementChoice`/`AwaitingLegendRuleChoice`/
    /// `AwaitingCommanderReturnChoice` if this pass needs one.
    pub(super) fn resolve_sba_loop_state(&mut self) -> StepStatus {
        self.process_state = match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
            SbaOutcome::Progressed(events, departed) => self.finish_sba_loop(events, departed),
            SbaOutcome::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                is_destroy,
                pending_deaths,
                resolved_so_far,
            } => {
                // Unaffected by the pause below (player-loss events,
                // plus any deaths already resolved earlier in this
                // same pass) - commit them now rather than holding
                // them hostage to a choice about a different death.
                self.state.ledger.extend(resolved_so_far);
                ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::SbaDeath {
                        pending_deaths,
                        is_destroy,
                        then: SbaResumeThen::SbaLoop,
                    },
                }
            }
            SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far } => {
                // Unaffected by the pause below, same reasoning as
                // NeedsOrderingChoice's own resolved_so_far above.
                self.state.ledger.extend(resolved_so_far);
                ProcessState::AwaitingLegendRuleChoice { player, candidates, then: SbaResumeThen::SbaLoop }
            }
            SbaOutcome::NeedsCommanderReturnChoice { player, commander, resolved_so_far } => {
                self.state.ledger.extend(resolved_so_far);
                ProcessState::AwaitingCommanderReturnChoice {
                    player,
                    commander,
                    then: SbaResumeThen::SbaLoop,
                }
            }
        };
        StepStatus::Progressed
    }

    /// `ProcessState::CleanupSbaCheck` — CR 514.3's cleanup-step SBA pass,
    /// same shape as `resolve_sba_loop_state` but finishing through
    /// `finish_cleanup_sba_check` (routes to `PipelineExit::CleanupCheck`
    /// instead of re-entering the loop) and tagging every pause's `then`
    /// with this pass's own `since_ledger`.
    pub(super) fn resolve_cleanup_sba_check_state(&mut self) -> StepStatus {
        let since_ledger = self.state.ledger.len();
        self.process_state = match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
            SbaOutcome::Progressed(events, departed) => {
                self.finish_cleanup_sba_check(events, departed, since_ledger)
            }
            SbaOutcome::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                is_destroy,
                pending_deaths,
                resolved_so_far,
            } => {
                self.state.ledger.extend(resolved_so_far);
                ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::SbaDeath {
                        pending_deaths,
                        is_destroy,
                        then: SbaResumeThen::CleanupSbaCheck { since_ledger },
                    },
                }
            }
            SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far } => {
                self.state.ledger.extend(resolved_so_far);
                ProcessState::AwaitingLegendRuleChoice {
                    player,
                    candidates,
                    then: SbaResumeThen::CleanupSbaCheck { since_ledger },
                }
            }
            SbaOutcome::NeedsCommanderReturnChoice { player, commander, resolved_so_far } => {
                self.state.ledger.extend(resolved_so_far);
                ProcessState::AwaitingCommanderReturnChoice {
                    player,
                    commander,
                    then: SbaResumeThen::CleanupSbaCheck { since_ledger },
                }
            }
        };
        StepStatus::Progressed
    }

    /// `SbaLoop`'s own "what happens once this pass's events are known"
    /// transition — pulled out so a resumed CR 616.1 pause
    /// (`resume_sba_death`) and a fresh pass (`SbaLoop`'s own arm) agree on
    /// it instead of duplicating the branch.
    fn finish_sba_loop(&mut self, events: Vec<GameEvent>, departed: Vec<RegisteredAbility>) -> ProcessState {
        // Accumulate across every SbaLoop pass in this pipeline run (CR
        // 704.3's fixed point may take several passes, each killing more
        // creatures) - cleared by end_pipeline_run once CollectTriggers/
        // PutTriggersApnap have both had a chance to consult it.
        self.departed_triggered_abilities.extend(departed);
        let next = if events.is_empty() {
            // CR 610.3: state-based actions have settled, so any "until
            // [source] leaves the battlefield" exile whose source is gone
            // returns now, then the loop runs once more.
            if let Some(returning) = self.return_linked_exiles() {
                return self.game_over_check(returning);
            }
            let since = self.pipeline_context.as_ref().map(|ctx| ctx.since_ledger).unwrap_or(0);
            ProcessState::CollectTriggers { since }
        } else {
            self.state.ledger.extend(events);
            // Re-enter SbaLoop next step() - CR 704.3's fixed point, not a
            // single pass.
            ProcessState::SbaLoop
        };
        self.game_over_check(next)
    }

    /// CR 104.2a/104.4a: overrides `next` with `GameOver` if at most one
    /// player remains in a game that started with more than one — checked
    /// after every SBA pass commits its events, since `sba::
    /// apply_player_loss` (the only place `PlayerState.eliminated` flips)
    /// always runs inside one of those passes. `winner` is the sole
    /// remainder, or `None` if the last players standing were eliminated
    /// simultaneously (a draw).
    ///
    /// The `turn_order.len() < 2` guard matters: a single-player test
    /// fixture (many exist, unrelated to this feature) has exactly one
    /// never-eliminated player, which would otherwise read as "game over"
    /// after the very first SBA pass — CR 104.2a presupposes opponents to
    /// win against, so there's nothing to end here.
    fn game_over_check(&self, next: ProcessState) -> ProcessState {
        if self.state.turn_order.len() < 2 {
            return next;
        }
        let remaining = commands::non_eliminated_players(&self.state);
        if remaining.len() <= 1 {
            ProcessState::GameOver { winner: remaining.first().copied() }
        } else {
            next
        }
    }

    /// `CleanupSbaCheck`'s own "what happens once this pass's events (and
    /// any dying-creature choice) are known" transition — pulled out for the
    /// same reason `finish_sba_loop` is, and shared the same way between a
    /// fresh pass (the `CleanupSbaCheck` arm) and one resumed after a paused
    /// CR 616.1/704.5j choice (`resume_sba_death`).
    ///
    /// Stashes `departed` (CR 603.10 dies-trigger abilities) into
    /// `departed_triggered_abilities` and routes through the real
    /// `CollectTriggers`/`PutTriggersApnap` stages via
    /// `PipelineExit::CleanupCheck` instead of deciding priority-vs-advance
    /// right here — this cleanup path used to bypass `CollectTriggers`
    /// entirely (backlog item 7a's cleanup half), silently dropping any
    /// trigger a cleanup-step SBA raised. `since_ledger` (captured before
    /// this pass began) is what `PipelineExit::CleanupCheck` later compares
    /// the ledger against to decide CR 514.3's priority-vs-advance question.
    ///
    /// TODO: CR 514.3 says another cleanup step begins after a priority
    /// window opens here and its stack empties - currently
    /// advance_turn_step's (Ending, Cleanup) arm rotates the turn instead,
    /// skipping re-running discard/effect-expiry. Correct once a case
    /// actually reaches this branch.
    fn finish_cleanup_sba_check(
        &mut self,
        events: Vec<GameEvent>,
        departed: Vec<RegisteredAbility>,
        since_ledger: usize,
    ) -> ProcessState {
        self.departed_triggered_abilities.extend(departed);
        self.state.ledger.extend(events);
        self.pipeline_exit = PipelineExit::CleanupCheck { since_ledger };
        self.game_over_check(ProcessState::CollectTriggers { since: since_ledger })
    }

    /// Continues `sba::resolve_deaths` after a paused CR 616.1 ordering
    /// choice was answered for one dying creature — `already_resolved` is
    /// that creature's own just-committed death event; `pending_deaths` is
    /// the rest of this SBA pass's dying-creature list. Finishes into
    /// whichever state originally started this pass (`then`), via
    /// `finish_sba_loop`/`finish_cleanup_sba_check`, so a resumed pause and
    /// a fresh pass reach the exact same "nothing more to do" transition.
    /// Commits one scripted zone-move target's already-replacement-checked
    /// `final_event` — `Destroy`/`Sacrifice` reuse the same
    /// `commit_death_or_regeneration`/`commit_death` helpers every SBA-driven
    /// death already goes through; `Exile` is inlined (unchanged from the
    /// pre-`ScriptedZoneMove` Exile arm's own body) since its destination is
    /// the shared Exile zone (`owner: None`), not a per-player zone
    /// `commit_death`'s `zone_mut(to, Some(owner))` call assumes.
    ///
    /// Returns events owed to the caller's `trailing` list. Only `Reanimate`
    /// produces any (`CounterAdded` from enters-with-counters effects and the
    /// `Attached` event of an entering Aura); every other kind returns none.
    fn commit_scripted_zone_move(&mut self, kind: ScriptedZoneMoveKind, final_event: GameEventKind) -> Vec<GameEventKind> {
        let mut entry_events = Vec::new();
        match kind {
            ScriptedZoneMoveKind::Destroy => {
                for (event, departed) in sba::commit_death_or_regeneration(&mut self.state, final_event) {
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(event);
                }
            }
            ScriptedZoneMoveKind::Sacrifice => {
                let (event, departed) = sba::commit_death(&mut self.state, final_event);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(event);
            }
            ScriptedZoneMoveKind::Exile | ScriptedZoneMoveKind::ExileUntilLeaves { .. } => {
                let GameEventKind::ZoneChange { object: old_id, from, to, last_known_info, .. } = final_event else {
                    unreachable!("apply_replacements only rewrites ZoneChange fields, never the kind");
                };
                let last_known_info = last_known_info.expect(
                    "continue_scripted_zone_move always proposes with last_known_info populated",
                );
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage { previous: Some(old_id) };
                new_object.zone = to;
                new_object.timestamp = self.state.allocate_sequence_no();
                new_object.reset_for_new_object();

                let from_owner = match from {
                    ZoneKind::Battlefield | ZoneKind::Exile => None,
                    _ => Some(owner),
                };
                if let Some(zone) = self.state.zone_mut(from, from_owner) {
                    zone.remove(old_id);
                }
                let departed = commands::retire_object(&mut self.state, old_id);
                self.departed_triggered_abilities.extend(departed);
                // Exile is a shared zone (owner = None); per-player
                // graveyard/hand need Some(owner) - this lookup must use
                // None or the object silently disappears.
                let exile_owner = match to {
                    ZoneKind::Exile => None,
                    _ => Some(owner),
                };
                if let Some(zone) = self.state.zone_mut(to, exile_owner) {
                    zone.push_bottom(new_id);
                }
                self.state.objects.insert(new_id, new_object);
                // CR 610.3: only a card that really landed in exile is linked
                // (a replacement may have sent it elsewhere).
                if let ScriptedZoneMoveKind::ExileUntilLeaves { source } = kind {
                    if to == ZoneKind::Exile {
                        self.state.linked_exiles.push(LinkedExile { source, exiled: new_id });
                    }
                }

                let event_id = self.state.allocate_event_id();
                let sequence_no = self.state.allocate_sequence_no();
                let committed = GameEvent {
                    event_id,
                    sequence_no,
                    kind: GameEventKind::ZoneChange {
                        object: new_id,
                        from,
                        to,
                        last_known_info: Some(last_known_info),
                    },
                    caused_by_command: None,
                    parent_event: None,
                };
                self.state.ledger.push(committed);
            }
            // CR 400.3: a permanent returned to hand goes to its owner's
            // hand. Retires the old id (purging its abilities, collecting its
            // departed triggers like Exile) and clears battlefield-only state
            // on the fresh object - it is a new object (CR 400.7), not the
            // permanent with its counters/damage/attachments. The card's
            // hand abilities register on the next sweep
            // (`sync_off_battlefield_scripts`).
            ScriptedZoneMoveKind::Bounce => {
                let GameEventKind::ZoneChange { object: old_id, from, to, last_known_info, .. } = final_event else {
                    unreachable!("apply_replacements only rewrites ZoneChange fields, never the kind");
                };
                let last_known_info = last_known_info
                    .expect("continue_scripted_zone_move always proposes with last_known_info populated");
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage { previous: Some(old_id) };
                new_object.zone = to;
                new_object.timestamp = self.state.allocate_sequence_no();
                new_object.reset_for_new_object();

                let from_owner = match from {
                    ZoneKind::Battlefield | ZoneKind::Exile => None,
                    _ => Some(owner),
                };
                if let Some(zone) = self.state.zone_mut(from, from_owner) {
                    zone.remove(old_id);
                }
                let departed = commands::retire_object(&mut self.state, old_id);
                self.departed_triggered_abilities.extend(departed);
                let to_owner = match to {
                    ZoneKind::Exile | ZoneKind::Battlefield => None,
                    _ => Some(owner),
                };
                if let Some(zone) = self.state.zone_mut(to, to_owner) {
                    zone.push_bottom(new_id);
                }
                self.state.objects.insert(new_id, new_object);

                let event_id = self.state.allocate_event_id();
                let sequence_no = self.state.allocate_sequence_no();
                self.state.ledger.push(GameEvent {
                    event_id,
                    sequence_no,
                    kind: GameEventKind::ZoneChange {
                        object: new_id,
                        from,
                        to,
                        last_known_info: Some(last_known_info),
                    },
                    caused_by_command: None,
                    parent_event: None,
                });
            }
            // CR 701.9a (`phase5/mass-choice-effects`): moves a card off a
            // player's Hand rather than a permanent off the Battlefield —
            // can't reuse `sba::commit_death`/`commit_death_or_regeneration`
            // (both hardcode removing from `ZoneKind::Battlefield`) or
            // `Exile`'s own arm just above (same hardcode in its final
            // committed event's `from`), so this mirrors their generic
            // "retire the old id, mint a fresh one in the destination zone"
            // shape with `kind.from_zone()` (Hand) in both spots instead. No
            // dies-trigger bookkeeping to fold in (a discarded card was never
            // a permanent) — CR 701.9's own "whenever you discard a card"
            // triggers key off the `ZoneChange` event this pushes, the same
            // generic trigger-collection path every other zone move already
            // feeds, not anything special-cased here.
            // CR 701.17a: Library -> Graveyard (or wherever a replacement
            // sent it). `commit_zone_move` derives each zone's owner lookup
            // from the zone kind, so a redirect to the shared Exile zone
            // lands correctly, unlike the Discard arm below which hardcodes
            // `Some(owner)` for the destination.
            ScriptedZoneMoveKind::Mill => {
                let (event, departed) = commands::commit_zone_move(&mut self.state, final_event);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(event);
            }
            // CR 110.2a/400.7/614.12: Graveyard or Exile -> Battlefield, or
            // wherever a replacement sent it.
            ScriptedZoneMoveKind::Reanimate { controller, tapped } => {
                entry_events = self.commit_reanimate(final_event, controller, tapped);
            }
            ScriptedZoneMoveKind::Discard => {
                let GameEventKind::ZoneChange { object: old_id, to, last_known_info, .. } = final_event else {
                    unreachable!("apply_replacements only rewrites ZoneChange fields, never the kind");
                };
                let last_known_info = last_known_info
                    .expect("continue_scripted_zone_move always proposes with last_known_info populated");
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage { previous: Some(old_id) };
                new_object.zone = to;
                new_object.timestamp = self.state.allocate_sequence_no();

                if let Some(zone) = self.state.zone_mut(ScriptedZoneMoveKind::Discard.from_zone(), Some(owner)) {
                    zone.remove(old_id);
                }
                let departed = commands::retire_object(&mut self.state, old_id);
                self.departed_triggered_abilities.extend(departed);
                if let Some(zone) = self.state.zone_mut(to, Some(owner)) {
                    zone.push_bottom(new_id);
                }
                self.state.objects.insert(new_id, new_object);

                let event_id = self.state.allocate_event_id();
                let sequence_no = self.state.allocate_sequence_no();
                let committed = GameEvent {
                    event_id,
                    sequence_no,
                    kind: GameEventKind::ZoneChange {
                        object: new_id,
                        from: ScriptedZoneMoveKind::Discard.from_zone(),
                        to,
                        last_known_info: Some(last_known_info),
                    },
                    caused_by_command: None,
                    parent_event: None,
                };
                self.state.ledger.push(committed);
            }
        }
        entry_events
    }

    /// CR 610.3: the second one-shot effect of every "exile until [source]
    /// leaves the battlefield". Takes each `GameState::linked_exiles` entry
    /// whose source is no longer on the battlefield and returns its card to
    /// the battlefield under its owner's control (CR 610.3c) as one batch
    /// (CR 610.3d), replacement-checked like any reanimation. A card that is
    /// no longer in exile (`continue_scripted_zone_move` skips it) or never
    /// was (a token) just drops out. Returns `None` when nothing is due.
    fn return_linked_exiles(&mut self) -> Option<ProcessState> {
        let (due, waiting): (Vec<LinkedExile>, Vec<LinkedExile>) =
            std::mem::take(&mut self.state.linked_exiles).into_iter().partition(|link| {
                self.state.object(link.source).map_or(true, |source| source.zone != ZoneKind::Battlefield)
            });
        self.state.linked_exiles = waiting;
        if due.is_empty() {
            return None;
        }
        let returning: VecDeque<ObjectId> = due.into_iter().map(|link| link.exiled).collect();
        Some(self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Reanimate { controller: None, tapped: false },
            returning,
            ScriptedZoneMoveThen::SbaLoop,
            Vec::new(),
        ))
    }

    /// `ScriptedZoneMoveKind::Reanimate`'s commit: puts a returned card onto
    /// the battlefield the way a resolving permanent spell lands there
    /// (`Engine::continue_permanent_entry`), minus the stack-bound Copy,
    /// Devour and Amplify pauses (TODO on `AbilityEffect::ReturnToBattlefield`).
    ///
    /// `final_event` is the replacement-checked `ZoneChange`. If a
    /// replacement rewrote `to`, the card goes there instead and nothing
    /// battlefield-specific runs. The graveyard object's `controller` was
    /// set temporarily by `continue_scripted_zone_move` so entry replacement
    /// filters see the entering player; this function resets it to the owner
    /// first, on every path, including an Aura that stays put.
    fn commit_reanimate(
        &mut self,
        final_event: GameEventKind,
        controller: Option<PlayerId>,
        tapped: bool,
    ) -> Vec<GameEventKind> {
        let GameEventKind::ZoneChange { object: old_id, to, .. } = &final_event else {
            unreachable!("apply_replacements only rewrites ZoneChange fields, never the kind");
        };
        let (old_id, to) = (*old_id, *to);
        // CR 108.4a: undo the temporary controller write.
        if let Some(object) = self.state.object_mut(old_id) {
            object.controller = object.owner;
        }
        if to != ZoneKind::Battlefield {
            let (event, departed) = commands::commit_zone_move(&mut self.state, final_event);
            self.departed_triggered_abilities.extend(departed);
            self.state.ledger.push(event);
            return Vec::new();
        }
        let GameEventKind::ZoneChange { from, last_known_info, .. } = final_event else {
            unreachable!("checked above");
        };
        let last_known_info =
            last_known_info.expect("continue_scripted_zone_move always proposes with last_known_info populated");
        let owner = last_known_info.owner;
        let entering_controller = controller.unwrap_or(owner);

        // CR 303.4f/g: an Aura picks its host before it enters. One legal
        // host attaches; none leaves it where it is. TODO(CR 303.4f): two or
        // more hosts need a controller choice and also leave it where it is
        // (the "Aura with several hosts" TODO on `AbilityEffect::
        // ReturnToBattlefield`).
        let mut attach_to = None;
        if commands::is_aura(&last_known_info.definition) {
            let hosts = commands::legal_aura_entry_candidates(
                &self.state,
                &self.script_runtime,
                &last_known_info.definition,
                entering_controller,
                old_id,
            );
            match hosts.as_slice() {
                [only] => attach_to = Some(*only),
                _ => return Vec::new(),
            }
        }

        let new_id = self.state.allocate_object_id();
        let mut new_object = last_known_info.clone();
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage { previous: Some(old_id) };
        new_object.zone = ZoneKind::Battlefield;
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 400.7: no memory of the old object (also resets controller to
        // the owner and `entered_via_escape`), then CR 110.2a's controller.
        new_object.reset_for_new_object();
        new_object.controller = entering_controller;
        new_object.tapped = tapped;
        // CR 302.6: summoning sick.
        new_object.controlled_since_turn = self.state.turn_number;

        let from_owner = if from == ZoneKind::Exile { None } else { Some(owner) };
        if let Some(zone) = self.state.zone_mut(from, from_owner) {
            zone.remove(old_id);
        }
        let departed = commands::retire_object(&mut self.state, old_id);
        self.departed_triggered_abilities.extend(departed);
        if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
            zone.push_bottom(new_id);
        }
        self.state.objects.insert(new_id, new_object);

        let event_id = self.state.allocate_event_id();
        let sequence_no = self.state.allocate_sequence_no();
        self.state.ledger.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::ZoneChange {
                object: new_id,
                from,
                to: ZoneKind::Battlefield,
                last_known_info: Some(last_known_info),
            },
            caused_by_command: None,
            parent_event: None,
        });

        // The card's script registers its abilities and its own entry
        // replacement effects (CR 113.6h, 614.12), then the modifier family
        // reads them. Sunburst gets no colors: nothing was cast (CR 702.44b).
        self.run_card_script_for(new_id);
        let mut entry_events =
            replacement::apply_enters_battlefield_modifiers(&mut self.state, &self.script_runtime, new_id, from, ColorSet::empty());

        match attach_to {
            Some(Target::Object(host)) => {
                commands::attach_permanent(&mut self.state, new_id, host);
                entry_events.push(GameEventKind::Attached { object: new_id, attached_to: host });
            }
            Some(Target::Player(player)) => {
                commands::attach_permanent_to_player(&mut self.state, new_id, player);
                entry_events.push(GameEventKind::AttachedToPlayer { object: new_id, player });
            }
            Some(Target::Ability(_)) | None => {}
        }
        entry_events
    }

    /// Drains `remaining` scripted zone-move targets one at a time — CR
    /// 616.1's ordering-choice pause can be raised by any of them, not just
    /// the first, since each target independently proposes+replacement-
    /// checks+commits its own `ZoneChange` (`commit_scripted_zone_move`).
    /// Returns the same `ReplacementCheck { remaining: [..trailing, StackObjectResolved] }`
    /// transition the pre-`ScriptedZoneMove` non-pausing path already used
    /// once the queue empties, or pauses immediately on
    /// `AwaitingReplacementChoice` the instant one target needs an ordering
    /// choice, carrying the rest of the queue in `ReplacementResumeTarget::
    /// ScriptedZoneMove` for `submit_replacement_choice` to continue later.
    /// `trailing` (CR 702.82a, Devour): extra not-yet-committed events owed
    /// once the queue empties — empty for every caller except
    /// `submit_devour_choice`, which still needs `events_so_far` (this
    /// resolution's own pre-choice progress) and a `CounterAdded` to reach
    /// `ReplacementCheck` too, not just the sacrifices this function itself
    /// drives.
    ///
    /// `pub(in crate::engine)`: `choice_answers.rs`'s `submit_devour_choice`/
    /// `submit_search_devour_choice` also call this, and a later pass will
    /// call it from `engine/resolve/destruction.rs` once `ResolveStackTop`
    /// itself moves out of `mod.rs` — making it accessible now avoids
    /// needing to touch this file again for that.
    pub(in crate::engine) fn continue_scripted_zone_move(
        &mut self,
        kind: ScriptedZoneMoveKind,
        mut remaining: VecDeque<ObjectId>,
        then: ScriptedZoneMoveThen,
        mut trailing: Vec<GameEventKind>,
    ) -> ProcessState {
        while let Some(id) = remaining.pop_front() {
            let Some(last_known_info) =
                self.state.object(id).filter(|obj| kind.accepts_source_zone(obj.zone)).cloned()
            else {
                // Target already left its expected zone (or never resolved)
                // - nothing to move, same "skip rather than guess" stance
                // every other arm here takes.
                continue;
            };
            if let ScriptedZoneMoveKind::Reanimate { controller, .. } = kind {
                // CR 400.4a: an instant or sorcery that would enter the
                // battlefield stays in its previous zone.
                if last_known_info.definition.card_types.intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY) {
                    continue;
                }
                // Temporary: entry replacement filters read the object's
                // controller, so the entering player is written onto the
                // card now. `commit_reanimate` resets it to the owner.
                let entering = controller.unwrap_or(last_known_info.owner);
                if let Some(object) = self.state.object_mut(id) {
                    object.controller = entering;
                }
            }
            if kind == ScriptedZoneMoveKind::Destroy {
                // CR 702.12b: an indestructible permanent can't be destroyed
                // at all - not a replacement effect, checked up front.
                let indestructible =
                    commands::effective_keywords(&self.state, &self.script_runtime, id)
                        .contains(KeywordSet::INDESTRUCTIBLE);
                if indestructible {
                    continue;
                }
            }
            let to = match kind {
                ScriptedZoneMoveKind::Exile | ScriptedZoneMoveKind::ExileUntilLeaves { .. } => ZoneKind::Exile,
                ScriptedZoneMoveKind::Bounce => ZoneKind::Hand,
                ScriptedZoneMoveKind::Reanimate { .. } => ZoneKind::Battlefield,
                _ => ZoneKind::Graveyard,
            };
            let from = last_known_info.zone;
            let proposed = GameEventKind::ZoneChange { object: id, from, to, last_known_info: Some(last_known_info) };
            // CR 614.8: only Destroy is genuine destruction - a regeneration
            // shield can intercept it and only it (Sacrifice/Exile are each
            // their own real zone-change verb, neither is destruction).
            let is_destroy = kind == ScriptedZoneMoveKind::Destroy;
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, is_destroy) {
                ReplacementOutcome::Resolved(final_event) => {
                    trailing.extend(self.commit_scripted_zone_move(kind, final_event));
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, then, trailing },
                    };
                }
            }
        }
        match then {
            ScriptedZoneMoveThen::StackResolution { stack_id } => {
                let mut final_events: VecDeque<GameEventKind> = trailing.into();
                final_events.push_back(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck { remaining: final_events, checked: Vec::new() }
            }
            // CR 614.12a wired into Search (`phase5/devour-amplify-search-
            // entry`): the chosen sacrifices are done - hand control back to
            // the search's own still-pending candidates instead of
            // finalizing here; `continue_search_battlefield_entry` pushes
            // its own `StackObjectResolved` once *its* queue drains, same as
            // every other real exit from that function.
            ScriptedZoneMoveThen::SearchResume { controller, remaining, destination, stack_id, shuffle_after } => self
                .continue_search_battlefield_entry(remaining, destination, controller, stack_id, shuffle_after, trailing),
            // `phase5/mass-choice-effects`: this batch is done - run the
            // chained second batch (`chain_pending_zone_moves`'s own tail),
            // handing it the same `trailing` events since neither batch on
            // its own is the "real" one; whichever finishes last is the one
            // that actually reaches ScriptedZoneMoveThen::StackResolution.
            ScriptedZoneMoveThen::ThenScriptedZoneMove { kind, remaining, then } => {
                self.continue_scripted_zone_move(kind, remaining, *then, trailing)
            }
            // CR 610.3: the returned permanents' own events (`trailing`) go
            // straight into the ledger, then the SBA loop runs again.
            ScriptedZoneMoveThen::SbaLoop => {
                self.log_events(trailing);
                ProcessState::SbaLoop
            }
        }
    }

    /// `phase5/mass-choice-effects`: performs 0-2 zone-move batches from a
    /// script operation's own `sacrifice_chosen`/`discard_chosen` result
    /// (`OperationOutcome::PendingZoneMoves`) — `Sacrifice`/`Discard` are
    /// different `ScriptedZoneMoveKind`s (different `from_zone()`), and
    /// `continue_scripted_zone_move` only ever drains one `kind` per call, so
    /// two non-empty batches are chained via `ScriptedZoneMoveThen::
    /// ThenScriptedZoneMove` rather than attempted in one call. Empty
    /// batches are dropped rather than chained (nothing to move); an empty
    /// `moves` altogether falls through to `continue_scripted_zone_move`'s
    /// own "queue already empty" tail, which just applies `then` immediately
    /// — same as if this function had never been called. Shared by
    /// `misc_effects::resolve_misc_or_script_effect` (a standalone script
    /// operation's own result, moved immediately - no batching context) and
    /// `each_player::resolve_for_each_player_effect` (the *final* combined
    /// batch once every player in a `ForEachPlayer` loop has chosen).
    pub(in crate::engine) fn chain_pending_zone_moves(
        &mut self,
        moves: Vec<(ScriptZoneMoveKind, Vec<ObjectId>)>,
        then: ScriptedZoneMoveThen,
        trailing: Vec<GameEventKind>,
    ) -> ProcessState {
        let mut moves: Vec<(ScriptedZoneMoveKind, VecDeque<ObjectId>)> = moves
            .into_iter()
            .filter(|(_, ids)| !ids.is_empty())
            .map(|(kind, ids)| {
                let kind = match kind {
                    ScriptZoneMoveKind::Sacrifice => ScriptedZoneMoveKind::Sacrifice,
                    ScriptZoneMoveKind::Discard => ScriptedZoneMoveKind::Discard,
                };
                (kind, ids.into())
            })
            .collect();
        if moves.is_empty() {
            return self.continue_scripted_zone_move(ScriptedZoneMoveKind::Sacrifice, VecDeque::new(), then, trailing);
        }
        let (first_kind, first_remaining) = moves.remove(0);
        let mut chained_then = then;
        for (kind, remaining) in moves.into_iter().rev() {
            chained_then = ScriptedZoneMoveThen::ThenScriptedZoneMove { kind, remaining, then: Box::new(chained_then) };
        }
        self.continue_scripted_zone_move(first_kind, first_remaining, chained_then, trailing)
    }

    /// CR 120.3/121/616.1: drains `remaining` scripted draws one at a time,
    /// each proposed via `commands::propose_draw`, replacement-checked, and
    /// committed inline. A CR 616.1 ordering choice pauses on any individual
    /// draw, carrying the rest of the count in `ReplacementResumeTarget::
    /// ScriptedDraw` so `submit_replacement_choice` can resume. The draw
    /// events themselves are pushed directly to the ledger (not queued in
    /// `events_so_far`) so they are never re-checked a second time against a
    /// fresh, empty `excluded` set; `events_so_far` carries only the rest of
    /// this resolution's already-produced events (e.g. an instant/sorcery
    /// spell's Stack->Graveyard move) so they still reach the normal
    /// `ReplacementCheck`->`CommitEvents` pipeline once the draws finish.
    ///
    /// `pub(in crate::engine)`: `mod.rs`'s still-inlined `ResolveStackTop`
    /// arm (`AbilityEffect::DrawCards`) also calls this directly.
    pub(in crate::engine) fn continue_draw_cards(
        &mut self,
        mut remaining: u32,
        player: PlayerId,
        stack_id: ObjectId,
        mut events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        while remaining > 0 {
            remaining -= 1;
            let Some(proposed) = commands::propose_draw(&mut self.state, player) else {
                break;
            };
            match replacement::apply_replacements(
                &mut self.state,
                &self.script_runtime,
                proposed,
                false,
            ) {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    // CR 702.29b: a Game.Ops.DrawCards(n)-drawn card's own
                    // script self-registers any hand-only ability now, same
                    // as every other Library->Hand producer.
                    if let GameEventKind::ZoneChange { object, .. } = committed.kind {
                        self.run_card_script_for(object);
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
                        resume: ReplacementResumeTarget::ScriptedDraw {
                            remaining,
                            player,
                            stack_id,
                            events_so_far,
                        },
                    };
                }
            }
        }
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        ProcessState::ReplacementCheck {
            remaining: events_so_far.into(),
            checked: Vec::new(),
        }
    }

    fn resume_sba_death(
        &mut self,
        pending_deaths: VecDeque<sba::DyingCandidate>,
        mut already_resolved: Vec<GameEvent>,
        mut already_departed: Vec<RegisteredAbility>,
        then: SbaResumeThen,
    ) -> ProcessState {
        match sba::resolve_deaths(&mut self.state, &self.script_runtime, pending_deaths) {
            sba::DeathResolution::Done(mut events, mut departed) => {
                already_resolved.append(&mut events);
                already_departed.append(&mut departed);
                match then {
                    SbaResumeThen::SbaLoop => self.finish_sba_loop(already_resolved, already_departed),
                    SbaResumeThen::CleanupSbaCheck { since_ledger } => {
                        self.finish_cleanup_sba_check(already_resolved, already_departed, since_ledger)
                    }
                }
            }
            sba::DeathResolution::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                is_destroy,
                pending_deaths,
            } => {
                // already_resolved (this pass's progress so far) commits now
                // - it's unaffected by this further pause.
                self.state.ledger.extend(already_resolved);
                ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::SbaDeath { pending_deaths, is_destroy, then },
                }
            }
        }
    }
}
