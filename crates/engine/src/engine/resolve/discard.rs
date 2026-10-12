//! `resolve_ability_effect`'s discard arms — `DiscardRandom` (`phase5/
//! mass-choice-effects`) and `Discard` (CR 701.9/701.9b's default,
//! chosen-by-the-affected-player shape). CR 701.9's discard verb had no
//! resolution-time producer at all before `phase5/mass-choice-effects`
//! (only the cost-payment shape, `AdditionalCost::Discard`, existed). Three
//! distinct shapes now live in this one file's orbit:
//! - `DiscardRandom` — no choice at all, a true random procedure.
//! - `Game.Ops.DiscardChosen` — a script-function result-table key, chosen
//!   by *this ability's own controller* via `Game.ChooseN`; handled by
//!   `misc_effects::resolve_misc_or_script_effect`/`each_player::
//!   resolve_for_each_player_effect` via `OperationOutcome::
//!   PendingZoneMoves`, not a standalone `AbilityEffect` variant.
//! - `Discard` (this file, `phase5/discard-target-player`) — CR 701.9b's
//!   *default* case: a targeted player discards, choosing their own card(s)
//!   via a real mid-resolution pause (`ProcessState::AwaitingDiscardChoice`)
//!   distinct from both of the above.

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
    /// CR 701.9b's "require a random discard" shape — `Game.Ops.
    /// DiscardRandom(filter, count)`. Draws via `pick_discard_random_ids`
    /// below, then hands the result straight to `continue_scripted_zone_move`
    /// for the single-player case — see `destruction::
    /// resolve_sacrifice_random_effect`'s own doc comment for why the
    /// picking half is split out (a bare-tag `Game.Ops.ForEachPlayer(Game.
    /// Ops.DiscardRandom(...))` needs it too).
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

    /// The picking half of `resolve_discard_random_effect` above, split out
    /// (`phase5/mass-choice-effects`) the same way and for the same reason
    /// `destruction::pick_sacrifice_random_ids` is — see that method's own
    /// doc comment. `count` of `controller`'s own hand cards matching
    /// `filter` (relative to `source`/`controller`, same `matches_effect_
    /// filter` convention every other filtered picker in this engine uses;
    /// an empty filter table matches every card, same "no restriction"
    /// default) — or fewer, if fewer match (CR 701.17b's mill precedent
    /// again) — are drawn without replacement via `next_rng`, each draw
    /// logged as `RngDrawn`.
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

    /// CR 701.9/701.9b: `Game.Ops.Discard(count)`'s own arm - pauses on
    /// `ProcessState::AwaitingDiscardChoice` for the *targeted* player's own
    /// choice, rather than resolving synchronously. No legal target (already
    /// fizzled via `target_is_legal` upstream) is a silent no-op, same
    /// stance every other single-target effect in this engine already takes.
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
        // remaining_count counts what's owed *after* the pick this pause
        // represents - count=1 means this is the only pick, so 0 more after.
        let next = Self::next_discard_state(&self.state, player, count - 1, top.stack_id, events);
        let status = Self::status_for(&next);
        self.process_state = next;
        ControlFlow::Break(status)
    }

    /// Computes what happens next for `player`'s discard: another
    /// `AwaitingDiscardChoice` pause if their hand still has cards left, or
    /// the effect finishing outright if it's empty (CR 701.9's own "nothing
    /// to discard" silent case, same posture `Counter`/`Sacrifice` already
    /// take for a target that's no longer there). A pure query, mirroring
    /// `continue_draw_cards`'s own "return a `ProcessState`, let the caller
    /// assign it" convention - shared by the initial raise
    /// (`resolve_discard_effect` above) and every subsequent pick
    /// (`finish_or_continue_discard` below).
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

    /// `next_discard_state`'s own `StepStatus` mirror - only
    /// `resolve_discard_effect` (reached through the `ControlFlow` protocol,
    /// which needs a `StepStatus` to `Break` with) needs this; every other
    /// caller of `next_discard_state`-derived process states just assigns
    /// the `ProcessState` directly and lets the next `step()` derive its own
    /// status the ordinary way.
    fn status_for(process_state: &ProcessState) -> StepStatus {
        match process_state {
            ProcessState::AwaitingDiscardChoice { player, candidates, remaining_count, .. } => {
                StepStatus::AwaitingDiscardChoice { player: *player, candidates: candidates.clone(), remaining_count: *remaining_count }
            }
            _ => StepStatus::Progressed,
        }
    }

    /// The shared "this effect is done" tail - pushes `StackObjectResolved`
    /// and hands off to `ReplacementCheck`, same shape every other
    /// synchronous-once-finished effect in this engine already ends with.
    fn finish_discard(stack_id: ObjectId, mut events_so_far: Vec<GameEventKind>) -> ProcessState {
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
    }

    /// Answers `Engine::submit_discard_choice` — proposes+replacement-
    /// checks+commits `chosen`'s own Hand->Graveyard `ZoneChange` (CR
    /// 702.35a Madness-compatible, same `replacement::madness_marker_event`
    /// check every other real discard producer already makes - a targeted
    /// discard is just as real a "discard" for Madness's own purposes as a
    /// cost payment or the cleanup-step discard are). A CR 616.1 ordering
    /// choice pauses via `ReplacementResumeTarget::TargetedDiscard`, which
    /// calls back into `finish_or_continue_discard` below once answered -
    /// same "propose/replacement-check-or-pause/commit" shape every other
    /// real zone-move producer in this engine already takes, just for
    /// exactly one object at a time instead of a pre-built batch (there's no
    /// batch here - each object is only known once its own pick is made).
    /// Returns a bare `ProcessState`, same "caller assigns it" convention
    /// `continue_draw_cards`/`continue_scripted_zone_move` already use.
    pub(in crate::engine) fn continue_targeted_discard(
        &mut self,
        chosen: ObjectId,
        player: PlayerId,
        remaining_count: u32,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        let Some(proposed) = commands::propose_zone_move(&self.state, chosen, ZoneKind::Hand, ZoneKind::Graveyard) else {
            // Vanished between being offered as a candidate and being
            // proposed (shouldn't happen synchronously, but same "skip
            // rather than guess" stance every other producer here takes).
            return self.finish_or_continue_discard(player, remaining_count, stack_id, events_so_far);
        };
        // CR 614.8: a discard is never genuine destruction.
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

    /// Once one pick has actually committed (or turned out to be
    /// unreachable), decides what's next: `remaining_count` more picks owed
    /// means raising `AwaitingDiscardChoice` again (or finishing early if
    /// the hand's now empty, via `next_discard_state`); zero means finishing
    /// outright. Shared tail for both `continue_targeted_discard`'s own
    /// unpaused commit and its CR 616.1 resume arm (`replacement_resume.
    /// rs`'s own `TargetedDiscard` dispatch), since both reach the exact
    /// same "one pick just settled, what now" decision point. Bare
    /// `ProcessState` return, same convention `continue_targeted_discard`
    /// above already follows.
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
