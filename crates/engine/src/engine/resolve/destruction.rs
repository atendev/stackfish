//! The always-pausing zone-move arms: sacrifice, destroy, destroy-all, exile. Each hands
//! its queue to `continue_scripted_zone_move`, which sets `process_state`. See `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::types::ability::EntryControl;
use crate::types::effect::{matches_effect_filter, EffectFilter};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};

impl Engine {
    pub(super) fn resolve_sacrifice_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Sacrifice, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 701.21a: the resolving ability's own source. A stale id does nothing
    /// (CR 400.7), and a source under another player's control isn't sacrificed.
    pub(super) fn resolve_sacrifice_self_effect(&mut self, top: &StackObject, events: Vec<GameEventKind>) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let on_battlefield = self.state.object(top.source).is_some_and(|o| o.zone == ZoneKind::Battlefield);
        let controlled = !on_battlefield
            || crate::layers::effective_controller(&self.state, &self.script_runtime, top.source) == top.controller;
        let targets: VecDeque<ObjectId> = if controlled { VecDeque::from([top.source]) } else { VecDeque::new() };
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Sacrifice, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    pub(super) fn resolve_destroy_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    pub(super) fn resolve_destroy_all_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = self
            .state
            .zone(ZoneKind::Battlefield, None)
            .map(|zone| {
                zone.contents
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.state
                            .object(*id)
                            .is_some_and(|object| matches_effect_filter(filter, object, top.source, top.controller))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    pub(super) fn resolve_exile_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Exile, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 610.3: `resolve_exile_effect` plus a link to `top.source`. If the source
    /// already left the battlefield the "until" event has happened, so nothing is
    /// exiled (CR 610.3a).
    pub(super) fn resolve_exile_until_source_leaves_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let source_on_battlefield = self.state.object(top.source).is_some_and(|o| o.zone == ZoneKind::Battlefield);
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter(|_| source_on_battlefield)
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::ExileUntilLeaves { source: top.source },
            targets,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            events,
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 400.3: moves each legal battlefield target to its owner's hand.
    pub(super) fn resolve_return_to_hand_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Bounce, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 400.7: the resolving ability's own source; a stale id finds nothing.
    pub(super) fn resolve_return_self_to_hand_effect(&mut self, top: &StackObject, events: Vec<GameEventKind>) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = VecDeque::from([top.source]);
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Bounce, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 400.3: every object in `zone` matching `filter`.
    pub(super) fn resolve_return_all_to_hand_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        zone: ZoneKind,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets = self.objects_in_zone_matching(top, filter, zone);
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Bounce, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 406.2: the resolving ability's own source; a stale id finds nothing.
    pub(super) fn resolve_exile_self_effect(&mut self, top: &StackObject, events: Vec<GameEventKind>) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = VecDeque::from([top.source]);
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Exile, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 406.2: every object in `zone` matching `filter`, moved simultaneously.
    pub(super) fn resolve_exile_all_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        zone: ZoneKind,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets = self.objects_in_zone_matching(top, filter, zone);
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Exile, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 110.2a: moves each legal graveyard or exile target onto the battlefield. Players are skipped.
    pub(super) fn resolve_return_to_battlefield_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        control: EntryControl,
        tapped: bool,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state = self.continue_scripted_zone_move(
            reanimate_kind(top, control, tapped),
            targets,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            events,
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 110.2a: every object in `zone` matching `filter`, no targets.
    pub(super) fn resolve_return_all_to_battlefield_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        zone: ZoneKind,
        control: EntryControl,
        tapped: bool,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets = self.objects_in_zone_matching(top, filter, zone);
        self.process_state = self.continue_scripted_zone_move(
            reanimate_kind(top, control, tapped),
            targets,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            events,
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// Every object in any zone of `kind` that `filter` matches relative to the ability.
    fn objects_in_zone_matching(&self, top: &StackObject, filter: &EffectFilter, zone: ZoneKind) -> VecDeque<ObjectId> {
        self.state
            .zones
            .iter()
            .filter(|z| z.kind == zone)
            .flat_map(|z| z.contents.iter().copied())
            .filter(|id| {
                self.state
                    .object(*id)
                    .is_some_and(|object| matches_effect_filter(filter, object, top.source, top.controller))
            })
            .collect()
    }

    /// CR 701.21: draws via `pick_sacrifice_random_ids`, then hands the result to
    /// `continue_scripted_zone_move`. `ForEachPlayer` calls the picking half directly
    /// so every player's picks merge before the one zone move (CR 101.4).
    pub(super) fn resolve_sacrifice_random_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        count: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let chosen = self.pick_sacrifice_random_ids(top.source, top.controller, filter, count);
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Sacrifice,
            chosen,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            events,
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// The picking half of `resolve_sacrifice_random_effect`. Collects matching
    /// candidates into a `Vec` so `next_rng` can index it, then draws `count`
    /// without replacement, or fewer if fewer matched, logging each as `RngDrawn`.
    /// Synchronous, so a caller merging several players' picks never sees a pause.
    pub(in crate::engine) fn pick_sacrifice_random_ids(
        &mut self,
        source: ObjectId,
        controller: PlayerId,
        filter: &EffectFilter,
        count: u32,
    ) -> VecDeque<ObjectId> {
        let mut candidates: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Battlefield, None)
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
                kind: GameEventKind::RngDrawn { purpose: "sacrifice_random".to_string(), value: rng_val },
                caused_by_command: None,
                parent_event: None,
            });
        }
        chosen
    }
}

/// CR 110.2a: `You` enters under the ability's controller; `Owner` under each card's owner.
fn reanimate_kind(top: &StackObject, control: EntryControl, tapped: bool) -> ScriptedZoneMoveKind {
    let controller = match control {
        EntryControl::You => Some(top.controller),
        EntryControl::Owner => None,
    };
    ScriptedZoneMoveKind::Reanimate { controller, tapped }
}
