//! The library arms: `DrawCards`, `Shuffle`, `Scry`, `Surveil`, `Search`, `Mill`.
//! Scry, surveil and search pause only when a real decision exists. See `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::types::ability::SearchDestination;
use crate::types::cost::FilterSpec;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::StackObject;
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};
use super::direct_effects::affected_player;

/// Whether a target word can pick a player (a player word, a scripted player
/// word or "any target").
fn word_names_a_player(req: &TargetRequirement) -> bool {
    matches!(
        req.filter,
        TargetFilter::Player { .. } | TargetFilter::PlayerScript(_) | TargetFilter::AnyTarget
    )
}

impl Engine {
    pub(super) fn resolve_draw_cards_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        amount: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // "Target player draws N" draws for the targeted player; a gone target draws
        // nobody. Words that name no player leave the controller drawing.
        let names_a_player = match &top.target_requirement {
            None => false,
            Some(TargetSpec::Primitive(req)) => word_names_a_player(req),
            Some(TargetSpec::Multiple(reqs)) => reqs.iter().any(word_names_a_player),
        };
        let player = if names_a_player {
            affected_player(top, legal_targets)
        } else {
            Some(top.controller)
        };
        let Some(player) = player else {
            return ControlFlow::Continue(events);
        };
        self.process_state = self.continue_draw_cards(amount, player, top.stack_id, events);
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 701.17a: the top `min(n, library size)` cards of the affected player,
    /// moved to the graveyard. An empty library moves nothing and isn't a draw, so
    /// CR 704.5b never applies.
    pub(super) fn resolve_mill_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        amount: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let Some(player) = affected_player(top, legal_targets) else {
            return ControlFlow::Continue(events);
        };
        let ids = self.top_of_library_ids(player, amount);
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Mill,
            ids,
            ScriptedZoneMoveThen::StackResolution {
                stack_id: top.stack_id,
            },
            events,
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// The first `n` cards of `player`'s library, top first; fewer if the
    /// library is smaller (CR 701.17b).
    pub(in crate::engine) fn top_of_library_ids(
        &self,
        player: PlayerId,
        n: u32,
    ) -> VecDeque<ObjectId> {
        self.state
            .zone(ZoneKind::Library, Some(player))
            .map(|z| z.contents.iter().take(n as usize).copied().collect())
            .unwrap_or_default()
    }

    pub(super) fn resolve_shuffle_effect(
        &mut self,
        top: &StackObject,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        self.shuffle_library(top.controller);
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_scry_effect(
        &mut self,
        top: &StackObject,
        n: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let cards: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Library, Some(top.controller))
            .map_or(Vec::new(), |z| {
                z.contents.iter().take(n as usize).copied().collect()
            });
        if !cards.is_empty() {
            self.process_state = ProcessState::AwaitingScryChoice {
                stack_id: top.stack_id,
                controller: top.controller,
                events_so_far: events,
                cards,
            };
            return ControlFlow::Break(StepStatus::Progressed);
        }
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_surveil_effect(
        &mut self,
        top: &StackObject,
        n: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let cards: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Library, Some(top.controller))
            .map_or(Vec::new(), |z| {
                z.contents.iter().take(n as usize).copied().collect()
            });
        if !cards.is_empty() {
            self.process_state = ProcessState::AwaitingSurveilChoice {
                stack_id: top.stack_id,
                controller: top.controller,
                events_so_far: events,
                cards,
            };
            return ControlFlow::Break(StepStatus::Progressed);
        }
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_search_effect(
        &mut self,
        top: &StackObject,
        filter: &FilterSpec,
        count: u32,
        destination: SearchDestination,
        shuffle_after: bool,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let candidates: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Library, Some(top.controller))
            .map(|z| z.contents.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|id| {
                // CR 108.4: a library card's controller is its owner.
                self.state
                    .object(*id)
                    .is_some_and(|obj| filter.matches(obj, obj.controller, top.controller))
            })
            .collect();
        if !candidates.is_empty() {
            self.process_state = ProcessState::AwaitingSearchChoice {
                stack_id: top.stack_id,
                controller: top.controller,
                events_so_far: events,
                candidates,
                count,
                destination,
                shuffle_after,
            };
            return ControlFlow::Break(StepStatus::Progressed);
        }
        ControlFlow::Continue(events)
    }
}
