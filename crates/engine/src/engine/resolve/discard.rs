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

use crate::types::effect::{matches_effect_filter, EffectFilter};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};

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
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let chosen = self.pick_discard_random_ids(top.source, top.controller, filter, count);
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Discard,
            chosen,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            Vec::new(),
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
}
