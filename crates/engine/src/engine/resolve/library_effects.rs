//! `resolve_ability_effect`'s library-touching arms — `DrawCards`,
//! `Shuffle`, `Scry`, `Surveil`, `Search`. `DrawCards` delegates to
//! `Engine::continue_draw_cards`, `Shuffle` to `Engine::shuffle_library`
//! (both already `pub(in crate::engine)`, in `replacement_resume.rs` and
//! `pregame.rs` respectively); `Scry`/`Surveil`/`Search` each peek their own
//! candidates and pause on a dedicated `AwaitingXChoice` `ProcessState` only
//! when there's a real decision to make (CR 701.22b/701.25c: an empty peek
//! or `n == 0` is a no-op, no pause, no event).

use std::ops::ControlFlow;

use crate::types::ability::SearchDestination;
use crate::types::cost::FilterSpec;
use crate::types::event::GameEventKind;
use crate::types::ids::ObjectId;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    pub(super) fn resolve_draw_cards_effect(
        &mut self,
        top: &StackObject,
        amount: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        self.process_state = self.continue_draw_cards(amount, top.controller, top.stack_id, events);
        ControlFlow::Break(StepStatus::Progressed)
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
            .map_or(Vec::new(), |z| z.contents.iter().take(n as usize).copied().collect());
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
            .map_or(Vec::new(), |z| z.contents.iter().take(n as usize).copied().collect());
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
                // A library card's `.controller`
                // always equals its owner in this
                // engine (no GainControl effect
                // targets a non-battlefield card,
                // CR 108.4) - the raw field is
                // exactly correct here.
                self.state.object(*id).is_some_and(|obj| filter.matches(obj, obj.controller, top.controller))
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
