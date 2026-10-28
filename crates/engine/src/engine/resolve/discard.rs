//! The discard arms: `DiscardRandom` and the targeted `Discard` (CR 701.9b), which
//! pauses for the affected player's own choice. See `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::commands;
use crate::replacement::{self, ReplacementOutcome};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::effect::{matches_effect_filter, EffectFilter};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, ReplacementResumeTarget, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};

impl Engine {
    /// CR 701.9b: draws via `pick_discard_random_ids`, then hands the result to
    /// `continue_scripted_zone_move`. The picking half is split out for `ForEachPlayer`.
    pub(super) fn resolve_discard_random_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        count: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let chosen = self.pick_discard_random_ids(top.source, top.controller, filter, count);
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Discard,
            chosen,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            events,
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// The picking half of `resolve_discard_random_effect`. Draws `count` of the
    /// controller's matching hand cards without replacement via `next_rng`, or fewer
    /// if fewer match, logging each as `RngDrawn`. An empty filter matches every card.
    pub(in crate::engine) fn pick_discard_random_ids(
        &mut self,
        source: ObjectId,
        controller: PlayerId,
        filter: &EffectFilter,
        count: u32,
    ) -> VecDeque<ObjectId> {
        let mut candidates: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Hand, Some(controller))
            .map(|zone| {
                zone.contents
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.state.object(*id).is_some_and(|object| matches_effect_filter(filter, object, source, controller))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut chosen: VecDeque<ObjectId> = VecDeque::new();
        for _ in 0..count {
            if candidates.is_empty() {
                break;
            }
            let rng_val = self.next_rng();
            let idx = (rng_val % candidates.len() as u64) as usize;
            chosen.push_back(candidates.remove(idx));
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::RngDrawn { purpose: "discard_random".to_string(), value: rng_val },
                caused_by_command: None,
                parent_event: None,
            });
        }
        chosen
    }

    /// CR 701.9: pauses on `AwaitingDiscardChoice` for the targeted player's choice.
    /// No legal target is a silent no-op.
    pub(super) fn resolve_discard_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        count: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let Some(Target::Player(player)) = legal_targets.first().copied() else {
            return ControlFlow::Continue(events);
        };
        if count == 0 {
            return ControlFlow::Continue(events);
        }
        // `remaining_count` counts picks owed after the one this pause represents.
        let next = Self::next_discard_state(&self.state, player, count - 1, top.stack_id, events);
        let status = Self::status_for(&next);
        self.process_state = next;
        ControlFlow::Break(status)
    }

    /// What follows a discard: another `AwaitingDiscardChoice` while the hand has
    /// cards, or finishing when it is empty. A pure query, shared by the initial raise
    /// and every later pick.
    fn next_discard_state(
        state: &GameState,
        player: PlayerId,
        remaining_count: u32,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        let candidates: Vec<ObjectId> =
            state.zone(ZoneKind::Hand, Some(player)).map(|z| Vec::from(z.contents.clone())).unwrap_or_default();
        if candidates.is_empty() {
            return Self::finish_discard(stack_id, events_so_far);
        }
        ProcessState::AwaitingDiscardChoice { player, candidates, remaining_count, stack_id, events_so_far }
    }

    /// `StepStatus` mirror of `next_discard_state`, needed only by the `ControlFlow` path.
    fn status_for(process_state: &ProcessState) -> StepStatus {
        match process_state {
            ProcessState::AwaitingDiscardChoice { player, candidates, remaining_count, .. } => {
                StepStatus::AwaitingDiscardChoice { player: *player, candidates: candidates.clone(), remaining_count: *remaining_count }
            }
            _ => StepStatus::Progressed,
        }
    }

    /// The shared tail: pushes `StackObjectResolved` and hands off to `ReplacementCheck`.
    fn finish_discard(stack_id: ObjectId, mut events_so_far: Vec<GameEventKind>) -> ProcessState {
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
    }

    /// Answers `submit_discard_choice`: proposes, replacement-checks and commits one
    /// hand-to-graveyard move, with the madness marker (CR 702.35a). An ordering pause
    /// (CR 616.1) resumes through `TargetedDiscard` into `finish_or_continue_discard`.
    pub(in crate::engine) fn continue_targeted_discard(
        &mut self,
        chosen: ObjectId,
        player: PlayerId,
        remaining_count: u32,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        let Some(proposed) = commands::propose_zone_move(&self.state, chosen, ZoneKind::Hand, ZoneKind::Graveyard) else {
            // Vanished since being offered; skip rather than guess.
            return self.finish_or_continue_discard(player, remaining_count, stack_id, events_so_far);
        };
        // CR 614.8
        match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
            ReplacementOutcome::Resolved(event) => {
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
                    resume: ReplacementResumeTarget::TargetedDiscard { player, remaining_count, stack_id, events_so_far },
                }
            }
        }
    }

    /// Once one pick settles, raises `AwaitingDiscardChoice` again for any picks
    /// owed, or finishes. Shared by the unpaused commit and the CR 616.1 resume.
    pub(in crate::engine) fn finish_or_continue_discard(
        &mut self,
        player: PlayerId,
        remaining_count: u32,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        if remaining_count == 0 {
            return Self::finish_discard(stack_id, events_so_far);
        }
        Self::next_discard_state(&self.state, player, remaining_count - 1, stack_id, events_so_far)
    }
}
