//! `resolve_ability_effect`'s discard arm — `DiscardRandom`
//! (`phase5/mass-choice-effects`). CR 701.9's discard verb had no
//! resolution-time producer at all before this slice (only the cost-payment
//! shape, `AdditionalCost::Discard`, existed) — see `AbilityEffect::
//! DiscardRandom`'s own doc comment for the full picture, including why
//! `Game.Ops.DiscardChosen` (the player-choice half of CR 701.9b) isn't a
//! resolver here at all: it's a script-function result-table key, handled by
//! `misc_effects::resolve_misc_or_script_effect`/`each_player::
//! resolve_for_each_player_effect` via `OperationOutcome::PendingZoneMoves`,
//! not a standalone `AbilityEffect` variant.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::ObjectId;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};

impl Engine {
    /// CR 701.9b's "require a random discard" shape — `Game.Ops.
    /// DiscardRandom(count)`. Same "draw without replacement via `next_rng`,
    /// log each as `RngDrawn`, fewer than `count` if the hand is smaller"
    /// shape `destruction::resolve_sacrifice_random_effect` uses, from the
    /// resolving `StackObject`'s own controller's hand instead of the
    /// battlefield.
    pub(super) fn resolve_discard_random_effect(
        &mut self,
        top: &StackObject,
        count: u32,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let mut candidates: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Hand, Some(top.controller))
            .map(|zone| zone.contents.iter().copied().collect())
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
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Discard,
            chosen,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            Vec::new(),
        );
        ControlFlow::Break(StepStatus::Progressed)
    }
}
