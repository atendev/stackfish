//! Replacement-ordering and SBA choice answers, the `SbaLoop` and
//! `CleanupSbaCheck` stages, and the scripted zone-move and draw resume helpers.
//! See `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::replacement::{self, ReplacementOutcome};
use crate::sba::{self, SbaOutcome};
use crate::script::ScriptZoneMoveKind;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::LinkedExile;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, Target};
use crate::types::mana::ColorSet;
use crate::types::zone::ZoneKind;

use super::{
    CommanderReturnChoiceError, Engine, LegendRuleChoiceError, ProcessState,
    ReplacementChoiceError, ReplacementResumeTarget, SbaResumeThen, ScriptedZoneMoveKind,
    ScriptedZoneMoveThen, StepStatus,
};

impl Engine {
    /// Answers a replacement ordering pause (CR 616.1). `chosen` must be one of
    /// the offered candidates. A separate entry point from `submit_action` because
    /// it answers a sub-decision, not a priority action.
    pub fn submit_replacement_choice(
        &mut self,
        chosen: replacement::ReplacementCandidate,
    ) -> Result<(), ReplacementChoiceError> {
        let ProcessState::AwaitingReplacementChoice { candidates, .. } = &self.process_state else {
            return Err(ReplacementChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(ReplacementChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingReplacementChoice {
            pending_event,
            mut excluded,
            resume,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // Apply the chosen effect, exclude it from this event, then re-run resolution
        // on the result (CR 616.1). `excluded` carries forward so an effect already
        // applied can't reapply after the pause.
        let rewritten = replacement::apply_one_replacement_or_protection(
            &mut self.state,
            chosen,
            pending_event,
        );
        excluded.push(chosen);
        // Only an `SbaDeath` resume can be a destroy proposal; it carries its own tag.
        let is_destroy = match &resume {
            ReplacementResumeTarget::ReplacementCheck { .. } => false,
            ReplacementResumeTarget::SbaDeath { is_destroy, .. } => *is_destroy,
            ReplacementResumeTarget::ScriptedZoneMove { kind, .. } => {
                *kind == ScriptedZoneMoveKind::Destroy
            }
            // CR 614.8: a sacrifice or discard cost is never destruction.
            ReplacementResumeTarget::CostZoneMove { .. } => false,
            // CR 614.8
            ReplacementResumeTarget::CleanupDiscard { .. } => false,
            // CR 614.8
            ReplacementResumeTarget::MadnessDecline { .. } => false,
            // CR 614.8
            ReplacementResumeTarget::Draw { .. } => false,
            // CR 614.8
            ReplacementResumeTarget::ScriptedDraw { .. } => false,
            // CR 614.8
            ReplacementResumeTarget::TargetedDiscard { .. } => false,
            // CR 614.8: a destination redirect is a zone change, never a destroy proposal.
            ReplacementResumeTarget::PermanentEntry { .. } => false,
        };
        let outcome = replacement::apply_replacements_from(
            &mut self.state,
            &self.script_runtime,
            rewritten,
            is_destroy,
            excluded,
        );

        self.process_state = match resume {
            ReplacementResumeTarget::ReplacementCheck {
                remaining,
                mut checked,
            } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    checked.push(event);
                    ProcessState::ReplacementCheck { remaining, checked }
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::ReplacementCheck { remaining, checked },
                },
            },
            ReplacementResumeTarget::SbaDeath {
                pending_deaths,
                then,
                ..
            } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let committed_pairs = sba::commit_death_or_regeneration(&mut self.state, event);
                    let (committed, departed_lists): (Vec<_>, Vec<_>) =
                        committed_pairs.into_iter().unzip();
                    let departed: Vec<_> = departed_lists.into_iter().flatten().collect();
                    self.resume_sba_death(pending_deaths, committed, departed, then)
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::SbaDeath {
                        pending_deaths,
                        is_destroy,
                        then,
                    },
                },
            },
            ReplacementResumeTarget::ScriptedZoneMove {
                kind,
                remaining,
                then,
                mut trailing,
            } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    trailing.extend(self.commit_scripted_zone_move(kind, event));
                    self.continue_scripted_zone_move(kind, remaining, then, trailing)
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::ScriptedZoneMove {
                        kind,
                        remaining,
                        then,
                        trailing,
                    },
                },
            },
            ReplacementResumeTarget::CostZoneMove {
                remaining,
                payer,
                stack_id,
                on_paid,
                cycling_source,
            } => match outcome {
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
                    self.continue_cost_zone_moves(
                        remaining,
                        payer,
                        stack_id,
                        on_paid,
                        cycling_source,
                    )
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
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
                },
            },
            ReplacementResumeTarget::CleanupDiscard {
                remaining,
                since_ledger,
            } => match outcome {
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
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::CleanupDiscard {
                        remaining,
                        since_ledger,
                    },
                },
            },
            ReplacementResumeTarget::MadnessDecline {
                card,
                stack_id,
                mut events_so_far,
            } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                    events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                    ProcessState::ReplacementCheck {
                        remaining: events_so_far.into(),
                        checked: Vec::new(),
                    }
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::MadnessDecline {
                        card,
                        stack_id,
                        events_so_far,
                    },
                },
            },
            ReplacementResumeTarget::Draw {
                tail_events,
                since_ledger,
            } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    // CR 702.29b: hand-entry self-registration, as on the unpaused draw path.
                    if let GameEventKind::ZoneChange { object, .. } = committed.kind {
                        self.run_card_script_for(object);
                    }
                    self.state.ledger.push(committed);
                    self.enter_priority_via_replacement_check(tail_events, since_ledger)
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::Draw {
                        tail_events,
                        since_ledger,
                    },
                },
            },
            ReplacementResumeTarget::ScriptedDraw {
                remaining,
                player,
                stack_id,
                events_so_far,
            } => {
                match outcome {
                    ReplacementOutcome::Resolved(event) => {
                        let (committed, departed) =
                            commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        // CR 702.29b
                        if let GameEventKind::ZoneChange { object, .. } = committed.kind {
                            self.run_card_script_for(object);
                        }
                        self.state.ledger.push(committed);
                        self.continue_draw_cards(remaining, player, stack_id, events_so_far)
                    }
                    ReplacementOutcome::NeedsOrderingChoice {
                        affected_player,
                        event,
                        candidates,
                        excluded,
                        ..
                    } => ProcessState::AwaitingReplacementChoice {
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
                    },
                }
            }
            ReplacementResumeTarget::TargetedDiscard {
                player,
                remaining_count,
                stack_id,
                events_so_far,
            } => {
                match outcome {
                    ReplacementOutcome::Resolved(event) => {
                        // CR 702.35a
                        let madness = replacement::madness_marker_event(&self.state, &event);
                        let (committed, departed) =
                            commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        if let Some(kind) = madness {
                            self.log_events(vec![kind]);
                        }
                        self.state.ledger.push(committed);
                        self.finish_or_continue_discard(
                            player,
                            remaining_count,
                            stack_id,
                            events_so_far,
                        )
                    }
                    ReplacementOutcome::NeedsOrderingChoice {
                        affected_player,
                        event,
                        candidates,
                        excluded,
                        ..
                    } => ProcessState::AwaitingReplacementChoice {
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
                    },
                }
            }
            ReplacementResumeTarget::PermanentEntry {
                top,
                legal_targets,
                events,
            } => match outcome {
                ReplacementOutcome::Resolved(GameEventKind::ZoneChange { to, .. }) => {
                    match self.continue_permanent_entry(top, &legal_targets, events, to) {
                        ControlFlow::Break(process_state) => process_state,
                        ControlFlow::Continue((top, mut events)) => {
                            events.push(GameEventKind::StackObjectResolved {
                                stack_id: top.stack_id,
                            });
                            ProcessState::ReplacementCheck {
                                remaining: events.into(),
                                checked: Vec::new(),
                            }
                        }
                    }
                }
                // Defensive: a would-move candidate only rewrites back into a zone change.
                ReplacementOutcome::Resolved(_) => ProcessState::ReplacementCheck {
                    remaining: VecDeque::from([GameEventKind::StackObjectResolved {
                        stack_id: top.stack_id,
                    }]),
                    checked: Vec::new(),
                },
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::PermanentEntry {
                        top,
                        legal_targets,
                        events,
                    },
                },
            },
        };
        Ok(())
    }

    /// Answers a legend rule pause (CR 704.5j). `kept` must be a candidate; the
    /// others die through `resume_sba_death`, so a replacement effect on a loser
    /// can still re-pause.
    pub fn submit_legend_rule_choice(
        &mut self,
        kept: ObjectId,
    ) -> Result<(), LegendRuleChoiceError> {
        let ProcessState::AwaitingLegendRuleChoice { candidates, .. } = &self.process_state else {
            return Err(LegendRuleChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&kept) {
            return Err(LegendRuleChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingLegendRuleChoice {
            candidates, then, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 704.5j: puts into the graveyard rather than destroying, so regeneration can't intercept.
        let losers: VecDeque<sba::DyingCandidate> = candidates
            .into_iter()
            .filter(|id| *id != kept)
            .map(|id| sba::DyingCandidate {
                id,
                is_destroy: false,
            })
            .collect();
        self.process_state = self.resume_sba_death(losers, Vec::new(), Vec::new(), then);
        Ok(())
    }

    /// Answers a commander return pause (CR 903.9a). `returned` moves the card to
    /// the command zone; `false` leaves it. Either way the arrival is recorded in
    /// `commander_return_offered` so it isn't asked again. This is a direct zone
    /// move with no death to replacement-check.
    pub fn submit_commander_return_choice(
        &mut self,
        returned: bool,
    ) -> Result<(), CommanderReturnChoiceError> {
        if !matches!(
            self.process_state,
            ProcessState::AwaitingCommanderReturnChoice { .. }
        ) {
            return Err(CommanderReturnChoiceError::NotAwaitingChoice);
        }
        let ProcessState::AwaitingCommanderReturnChoice {
            commander, then, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        self.state.commander_return_offered.insert(commander);
        let mut events = Vec::new();
        if returned
            && let Some(event) = sba::move_commander_to_command_zone(&mut self.state, commander)
        {
            events.push(event);
        }

        self.process_state = match then {
            SbaResumeThen::SbaLoop => self.finish_sba_loop(events, Vec::new()),
            SbaResumeThen::CleanupSbaCheck { since_ledger } => {
                self.finish_cleanup_sba_check(events, Vec::new(), since_ledger)
            }
        };
        Ok(())
    }

    /// CR 704.3: runs one SBA pass and either loops again or moves on to
    /// `CollectTriggers`, pausing when the pass needs a choice.
    pub(super) fn resolve_sba_loop_state(&mut self) -> StepStatus {
        self.process_state =
            match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
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
                    // Commit what the pause doesn't affect rather than hold it for an unrelated choice.
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
                SbaOutcome::NeedsLegendRuleChoice {
                    player,
                    candidates,
                    resolved_so_far,
                } => {
                    // Unaffected by the pause, as above.
                    self.state.ledger.extend(resolved_so_far);
                    ProcessState::AwaitingLegendRuleChoice {
                        player,
                        candidates,
                        then: SbaResumeThen::SbaLoop,
                    }
                }
                SbaOutcome::NeedsCommanderReturnChoice {
                    player,
                    commander,
                    resolved_so_far,
                } => {
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

    /// CR 514.3: the cleanup-step SBA pass. Finishes through
    /// `finish_cleanup_sba_check` and tags each pause with `since_ledger`.
    pub(super) fn resolve_cleanup_sba_check_state(&mut self) -> StepStatus {
        let since_ledger = self.state.ledger.len();
        self.process_state =
            match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
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
                SbaOutcome::NeedsLegendRuleChoice {
                    player,
                    candidates,
                    resolved_so_far,
                } => {
                    self.state.ledger.extend(resolved_so_far);
                    ProcessState::AwaitingLegendRuleChoice {
                        player,
                        candidates,
                        then: SbaResumeThen::CleanupSbaCheck { since_ledger },
                    }
                }
                SbaOutcome::NeedsCommanderReturnChoice {
                    player,
                    commander,
                    resolved_so_far,
                } => {
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

    /// Shared by a fresh pass and a resumed pause (`resume_sba_death`) so both reach
    /// the same transition.
    fn finish_sba_loop(
        &mut self,
        events: Vec<GameEvent>,
        departed: Vec<RegisteredAbility>,
    ) -> ProcessState {
        // Accumulates across passes; `end_pipeline_run` clears it after trigger collection.
        self.departed_triggered_abilities.extend(departed);
        let next = if events.is_empty() {
            // CR 610.3: linked exiles whose source left return once SBAs settle.
            if let Some(returning) = self.return_linked_exiles() {
                return self.game_over_check(returning);
            }
            let since = self
                .pipeline_context
                .as_ref()
                .map(|ctx| ctx.since_ledger)
                .unwrap_or(0);
            ProcessState::CollectTriggers { since }
        } else {
            self.state.ledger.extend(events);
            // CR 704.3: loop until no SBA fires.
            ProcessState::SbaLoop
        };
        self.game_over_check(next)
    }

    /// CR 104.2a: overrides `next` with `GameOver` when at most one player remains.
    /// `winner` is `None` if the last players were eliminated together (CR 104.4a).
    /// Skipped for fewer than two players in turn order, since one player alone
    /// has no opponent to win against.
    fn game_over_check(&self, next: ProcessState) -> ProcessState {
        if self.state.turn_order.len() < 2 {
            return next;
        }
        let remaining = commands::non_eliminated_players(&self.state);
        if remaining.len() <= 1 {
            ProcessState::GameOver {
                winner: remaining.first().copied(),
            }
        } else {
            next
        }
    }

    /// Shared by a fresh cleanup SBA pass and one resumed after a paused choice.
    /// Stashes `departed` (CR 603.10) and routes through `CollectTriggers` via
    /// `CleanupCheck`, which compares the ledger against `since_ledger` for CR 514.3.
    // TODO: another cleanup step begins after this priority window empties the stack; the turn rotates instead (CR 514.3a)
    fn finish_cleanup_sba_check(
        &mut self,
        events: Vec<GameEvent>,
        departed: Vec<RegisteredAbility>,
        since_ledger: usize,
    ) -> ProcessState {
        self.departed_triggered_abilities.extend(departed);
        self.state.ledger.extend(events);
        self.pipeline_exit = PipelineExit::CleanupCheck { since_ledger };
        self.game_over_check(ProcessState::CollectTriggers {
            since: since_ledger,
        })
    }

    /// Commits one replacement-checked scripted zone move. Destroy and sacrifice reuse
    /// the SBA death helpers. Exile is inlined because its zone has no owner. Returns
    /// the events owed to `trailing`: counters and an Aura's `Attached`, from
    /// `Reanimate` only.
    fn commit_scripted_zone_move(
        &mut self,
        kind: ScriptedZoneMoveKind,
        final_event: GameEventKind,
    ) -> Vec<GameEventKind> {
        let mut entry_events = Vec::new();
        match kind {
            ScriptedZoneMoveKind::Destroy => {
                for (event, departed) in
                    sba::commit_death_or_regeneration(&mut self.state, final_event)
                {
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
                let GameEventKind::ZoneChange {
                    object: old_id,
                    from,
                    to,
                    last_known_info,
                    ..
                } = final_event
                else {
                    unreachable!(
                        "apply_replacements only rewrites ZoneChange fields, never the kind"
                    );
                };
                let last_known_info = last_known_info.expect(
                    "continue_scripted_zone_move always proposes with last_known_info populated",
                );
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage {
                    previous: Some(old_id),
                };
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
                // Exile is a shared zone, so the owner lookup must be `None`.
                let exile_owner = match to {
                    ZoneKind::Exile => None,
                    _ => Some(owner),
                };
                if let Some(zone) = self.state.zone_mut(to, exile_owner) {
                    zone.push_bottom(new_id);
                }
                self.state.objects.insert(new_id, new_object);
                // CR 610.3: only a card that landed in exile is linked.
                if let ScriptedZoneMoveKind::ExileUntilLeaves { source } = kind
                    && to == ZoneKind::Exile
                {
                    self.state.linked_exiles.push(LinkedExile {
                        source,
                        exiled: new_id,
                    });
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
            // CR 400.3: returns to the owner's hand as a new object (CR 400.7) with
            // battlefield-only state cleared. Hand abilities register on the next
            // `sync_off_battlefield_scripts` sweep.
            ScriptedZoneMoveKind::Bounce => {
                let GameEventKind::ZoneChange {
                    object: old_id,
                    from,
                    to,
                    last_known_info,
                    ..
                } = final_event
                else {
                    unreachable!(
                        "apply_replacements only rewrites ZoneChange fields, never the kind"
                    );
                };
                let last_known_info = last_known_info.expect(
                    "continue_scripted_zone_move always proposes with last_known_info populated",
                );
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage {
                    previous: Some(old_id),
                };
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
            // CR 701.9a: a discard moves from hand, which the death helpers hardcode as
            // battlefield, so it retires the id and mints a new one itself. Discard
            // triggers key off the `ZoneChange` it pushes.
            // CR 701.17a: library to graveyard, or wherever a replacement sent it.
            // `commit_zone_move` derives the owner from the zone kind, so a redirect to
            // exile lands correctly.
            ScriptedZoneMoveKind::Mill => {
                let (event, departed) = commands::commit_zone_move(&mut self.state, final_event);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(event);
            }
            // CR 110.2a, 400.7: graveyard or exile to battlefield, or wherever a replacement sent it.
            ScriptedZoneMoveKind::Reanimate { controller, tapped } => {
                entry_events = self.commit_reanimate(final_event, controller, tapped);
            }
            ScriptedZoneMoveKind::Discard => {
                let GameEventKind::ZoneChange {
                    object: old_id,
                    to,
                    last_known_info,
                    ..
                } = final_event
                else {
                    unreachable!(
                        "apply_replacements only rewrites ZoneChange fields, never the kind"
                    );
                };
                let last_known_info = last_known_info.expect(
                    "continue_scripted_zone_move always proposes with last_known_info populated",
                );
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage {
                    previous: Some(old_id),
                };
                new_object.zone = to;
                new_object.timestamp = self.state.allocate_sequence_no();

                if let Some(zone) = self
                    .state
                    .zone_mut(ScriptedZoneMoveKind::Discard.source_zone(), Some(owner))
                {
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
                        from: ScriptedZoneMoveKind::Discard.source_zone(),
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

    /// CR 610.3: returns each `linked_exiles` card whose source left the battlefield
    /// to the battlefield under its owner's control (CR 610.3c) as one replacement-checked
    /// batch (CR 610.3d). Cards no longer in exile drop out. `None` when nothing is due.
    fn return_linked_exiles(&mut self) -> Option<ProcessState> {
        let (due, waiting): (Vec<LinkedExile>, Vec<LinkedExile>) =
            std::mem::take(&mut self.state.linked_exiles)
                .into_iter()
                .partition(|link| {
                    self.state
                        .object(link.source)
                        .is_none_or(|source| source.zone != ZoneKind::Battlefield)
                });
        self.state.linked_exiles = waiting;
        if due.is_empty() {
            return None;
        }
        let returning: VecDeque<ObjectId> = due.into_iter().map(|link| link.exiled).collect();
        Some(self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Reanimate {
                controller: None,
                tapped: false,
            },
            returning,
            ScriptedZoneMoveThen::SbaLoop,
            Vec::new(),
        ))
    }

    /// Commit for `Reanimate`: lands a returned card as a resolving permanent spell
    /// would, minus the stack-bound copy, devour and amplify pauses.
    /// `continue_scripted_zone_move` set the controller temporarily so entry filters
    /// see the entering player; this resets it to the owner first on every path.
    // TODO: enter-as-a-copy pause for a reanimated permanent (CR 707.9)
    // TODO: devour and amplify pauses for a reanimated permanent (CR 702.82a, 702.38a)
    fn commit_reanimate(
        &mut self,
        final_event: GameEventKind,
        controller: Option<PlayerId>,
        tapped: bool,
    ) -> Vec<GameEventKind> {
        let GameEventKind::ZoneChange {
            object: old_id, to, ..
        } = &final_event
        else {
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
        let GameEventKind::ZoneChange {
            from,
            last_known_info,
            ..
        } = final_event
        else {
            unreachable!("checked above");
        };
        let last_known_info = last_known_info
            .expect("continue_scripted_zone_move always proposes with last_known_info populated");
        let owner = last_known_info.owner;
        let entering_controller = controller.unwrap_or(owner);

        // CR 303.4f: an Aura with one legal host attaches, with none stays put.
        // TODO: an Aura with two or more hosts needs a controller choice; it stays put instead (CR 303.4f)
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
        new_object.lineage = ObjectLineage {
            previous: Some(old_id),
        };
        new_object.zone = ZoneKind::Battlefield;
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 400.7: a new object with no memory of the old one, then CR 110.2a's controller.
        new_object.reset_for_new_object();
        new_object.controller = entering_controller;
        new_object.tapped = tapped;
        // CR 302.6
        new_object.controlled_since_turn = self.state.turn_number;

        let from_owner = if from == ZoneKind::Exile {
            None
        } else {
            Some(owner)
        };
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

        // The script registers abilities and entry replacements for the modifier
        // family. Sunburst gets no colors since nothing was cast (CR 702.44b).
        self.run_card_script_for(new_id);
        let mut entry_events = replacement::apply_enters_battlefield_modifiers(
            &mut self.state,
            &self.script_runtime,
            new_id,
            from,
            ColorSet::empty(),
        );

        match attach_to {
            Some(Target::Object(host)) => {
                commands::attach_permanent(&mut self.state, new_id, host);
                entry_events.push(GameEventKind::Attached {
                    object: new_id,
                    attached_to: host,
                });
            }
            Some(Target::Player(player)) => {
                commands::attach_permanent_to_player(&mut self.state, new_id, player);
                entry_events.push(GameEventKind::AttachedToPlayer {
                    object: new_id,
                    player,
                });
            }
            Some(Target::Ability(_)) | None => {}
        }
        entry_events
    }

    /// Drains the scripted zone-move targets one at a time, since any can raise an
    /// ordering pause (CR 616.1); the rest of the queue rides in
    /// `ReplacementResumeTarget::ScriptedZoneMove`. `trailing` holds events owed once
    /// the queue empties, such as a devour counter (CR 702.82a).
    pub(in crate::engine) fn continue_scripted_zone_move(
        &mut self,
        kind: ScriptedZoneMoveKind,
        mut remaining: VecDeque<ObjectId>,
        then: ScriptedZoneMoveThen,
        mut trailing: Vec<GameEventKind>,
    ) -> ProcessState {
        while let Some(id) = remaining.pop_front() {
            let Some(last_known_info) = self
                .state
                .object(id)
                .filter(|obj| kind.accepts_source_zone(obj.zone))
                .cloned()
            else {
                // Target already left its zone; skip rather than guess.
                continue;
            };
            if let ScriptedZoneMoveKind::Reanimate { controller, .. } = kind {
                // CR 400.4a
                if last_known_info
                    .definition
                    .card_types
                    .intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY)
                {
                    continue;
                }
                // Entry replacement filters read the controller, so it is set temporarily;
                // `commit_reanimate` resets it.
                let entering = controller.unwrap_or(last_known_info.owner);
                if let Some(object) = self.state.object_mut(id) {
                    object.controller = entering;
                }
            }
            if kind == ScriptedZoneMoveKind::Destroy {
                // CR 702.12b: checked up front; indestructible isn't a replacement effect.
                let indestructible =
                    commands::effective_keywords(&self.state, &self.script_runtime, id)
                        .contains(KeywordSet::INDESTRUCTIBLE);
                if indestructible {
                    continue;
                }
            }
            let to = match kind {
                ScriptedZoneMoveKind::Exile | ScriptedZoneMoveKind::ExileUntilLeaves { .. } => {
                    ZoneKind::Exile
                }
                ScriptedZoneMoveKind::Bounce => ZoneKind::Hand,
                ScriptedZoneMoveKind::Reanimate { .. } => ZoneKind::Battlefield,
                _ => ZoneKind::Graveyard,
            };
            let from = last_known_info.zone;
            let proposed = GameEventKind::ZoneChange {
                object: id,
                from,
                to,
                last_known_info: Some(last_known_info),
            };
            // CR 614.8: only destroy can be regenerated.
            let is_destroy = kind == ScriptedZoneMoveKind::Destroy;
            match replacement::apply_replacements(
                &mut self.state,
                &self.script_runtime,
                proposed,
                is_destroy,
            ) {
                ReplacementOutcome::Resolved(final_event) => {
                    trailing.extend(self.commit_scripted_zone_move(kind, final_event));
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
                        resume: ReplacementResumeTarget::ScriptedZoneMove {
                            kind,
                            remaining,
                            then,
                            trailing,
                        },
                    };
                }
            }
        }
        match then {
            ScriptedZoneMoveThen::StackResolution { stack_id } => {
                let mut final_events: VecDeque<GameEventKind> = trailing.into();
                final_events.push_back(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck {
                    remaining: final_events,
                    checked: Vec::new(),
                }
            }
            // CR 614.12a: hand control back to the search's pending candidates, which
            // push their own `StackObjectResolved` when drained.
            ScriptedZoneMoveThen::SearchResume {
                controller,
                remaining,
                destination,
                stack_id,
                shuffle_after,
            } => self.continue_search_battlefield_entry(
                remaining,
                destination,
                controller,
                stack_id,
                shuffle_after,
                trailing,
            ),
            // Run the chained batch with the same `trailing`; whichever finishes last reaches `StackResolution`.
            ScriptedZoneMoveThen::ThenScriptedZoneMove {
                kind,
                remaining,
                then,
            } => self.continue_scripted_zone_move(kind, remaining, *then, trailing),
            // CR 610.3: the returned permanents' events go to the ledger, then SBAs rerun.
            ScriptedZoneMoveThen::SbaLoop => {
                self.log_events(trailing);
                ProcessState::SbaLoop
            }
        }
    }

    /// Runs the 0-2 zone-move batches of a script operation's sacrifice and discard
    /// results. Different kinds are chained through `ThenScriptedZoneMove` because
    /// `continue_scripted_zone_move` drains one kind per call. Empty batches drop out.
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
            return self.continue_scripted_zone_move(
                ScriptedZoneMoveKind::Sacrifice,
                VecDeque::new(),
                then,
                trailing,
            );
        }
        let (first_kind, first_remaining) = moves.remove(0);
        let mut chained_then = then;
        for (kind, remaining) in moves.into_iter().rev() {
            chained_then = ScriptedZoneMoveThen::ThenScriptedZoneMove {
                kind,
                remaining,
                then: Box::new(chained_then),
            };
        }
        self.continue_scripted_zone_move(first_kind, first_remaining, chained_then, trailing)
    }

    /// CR 121: drains scripted draws one at a time, each replacement-checked and
    /// committed inline; an ordering pause (CR 616.1) carries the remaining count in
    /// `ScriptedDraw`. Draw events go straight to the ledger so they aren't checked
    /// twice; `events_so_far` still goes through the normal pipeline afterwards.
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
                    // CR 702.29b
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

    /// Continues `sba::resolve_deaths` after a paused ordering choice for one dying
    /// creature, then finishes into `then` so a resumed pause and a fresh pass reach
    /// the same transition.
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
                    SbaResumeThen::SbaLoop => {
                        self.finish_sba_loop(already_resolved, already_departed)
                    }
                    SbaResumeThen::CleanupSbaCheck { since_ledger } => self
                        .finish_cleanup_sba_check(already_resolved, already_departed, since_ledger),
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
                // Unaffected by this further pause, so commit it now.
                self.state.ledger.extend(already_resolved);
                ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::SbaDeath {
                        pending_deaths,
                        is_destroy,
                        then,
                    },
                }
            }
        }
    }
}
