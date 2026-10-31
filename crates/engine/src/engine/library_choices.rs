//! Answers to scry, surveil and search pauses and the entry-attach choice a
//! battlefield-bound search can raise. See `DESIGN.md`.

use std::collections::{HashSet, VecDeque};

use crate::commands;
use crate::replacement;
use crate::types::ability::SearchDestination;
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, Target};
use crate::types::mana::ColorSet;
use crate::types::zone::ZoneKind;

use super::triggers_pipeline::is_exact_partition;
use super::{
    AmplifyChoiceError, AttachChoiceError, DevourChoiceError, Engine, ProcessState,
    ScriptedZoneMoveKind, ScriptedZoneMoveThen, ScryChoiceError, SearchChoiceError,
    SurveilChoiceError,
};

impl Engine {
    /// Answers a scry pause (CR 701.22). The two piles must partition the offered
    /// cards; `kept_on_top[0]` becomes the top card and `put_bottom`'s last entry
    /// the bottom card. A pure reorder, so replacement effects never see it.
    pub fn submit_scry_choice(
        &mut self,
        kept_on_top: Vec<ObjectId>,
        put_bottom: Vec<ObjectId>,
    ) -> Result<(), ScryChoiceError> {
        let ProcessState::AwaitingScryChoice { cards, .. } = &self.process_state else {
            return Err(ScryChoiceError::NotAwaitingChoice);
        };
        if !is_exact_partition(&kept_on_top, &put_bottom, cards) {
            return Err(ScryChoiceError::WrongCards);
        }
        let ProcessState::AwaitingScryChoice {
            stack_id,
            controller,
            mut events_so_far,
            cards,
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        if let Some(lib) = self.state.zone_mut(ZoneKind::Library, Some(controller)) {
            for id in &cards {
                lib.remove(*id);
            }
            for id in kept_on_top.iter().rev() {
                lib.push_top(*id);
            }
            for id in &put_bottom {
                lib.push_bottom(*id);
            }
        }

        events_so_far.push(GameEventKind::CardsScried {
            player: controller,
            kept_on_top,
            put_bottom,
        });
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state = ProcessState::ReplacementCheck {
            remaining: events_so_far.into(),
            checked: Vec::new(),
        };
        Ok(())
    }

    /// Answers a surveil pause (CR 701.25). Like scry except `put_graveyard` is a
    /// real library-to-graveyard move (CR 400.7).
    pub fn submit_surveil_choice(
        &mut self,
        kept_on_top: Vec<ObjectId>,
        put_graveyard: Vec<ObjectId>,
    ) -> Result<(), SurveilChoiceError> {
        let ProcessState::AwaitingSurveilChoice { cards, .. } = &self.process_state else {
            return Err(SurveilChoiceError::NotAwaitingChoice);
        };
        if !is_exact_partition(&kept_on_top, &put_graveyard, cards) {
            return Err(SurveilChoiceError::WrongCards);
        }
        let ProcessState::AwaitingSurveilChoice {
            stack_id,
            controller,
            mut events_so_far,
            cards,
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // Both piles leave the library, then the kept pile goes back on top.
        if let Some(lib) = self.state.zone_mut(ZoneKind::Library, Some(controller)) {
            for id in &cards {
                lib.remove(*id);
            }
            for id in kept_on_top.iter().rev() {
                lib.push_top(*id);
            }
        }

        let mut new_ids = Vec::with_capacity(put_graveyard.len());
        for card in &put_graveyard {
            let last_known_info = self
                .state
                .object(*card)
                .expect("validated by is_exact_partition above")
                .clone();
            let owner = last_known_info.owner;

            let new_id = self.state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage {
                previous: Some(*card),
            };
            new_object.zone = ZoneKind::Graveyard;
            new_object.timestamp = self.state.allocate_sequence_no();

            let departed = commands::retire_object(&mut self.state, *card);
            self.departed_triggered_abilities.extend(departed);
            self.state
                .zone_mut(ZoneKind::Graveyard, Some(owner))
                .expect("graveyard zone must exist")
                .push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);

            events_so_far.push(GameEventKind::ZoneChange {
                object: new_id,
                from: ZoneKind::Library,
                to: ZoneKind::Graveyard,
                last_known_info: Some(last_known_info),
            });
            new_ids.push(new_id);
        }

        events_so_far.push(GameEventKind::CardsSurveiled {
            player: controller,
            kept_on_top,
            put_graveyard: new_ids,
        });
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state = ProcessState::ReplacementCheck {
            remaining: events_so_far.into(),
            checked: Vec::new(),
        };
        Ok(())
    }

    /// Answers a search pause (CR 701.23). `chosen` must be distinct candidates and
    /// at most `count` of them; fewer, including none, is legal (CR 701.23b). The
    /// library is shuffled afterwards if the search says so (CR 701.24a).
    pub fn submit_search_choice(&mut self, chosen: Vec<ObjectId>) -> Result<(), SearchChoiceError> {
        let ProcessState::AwaitingSearchChoice {
            candidates, count, ..
        } = &self.process_state
        else {
            return Err(SearchChoiceError::NotAwaitingChoice);
        };
        if chosen.len() > *count as usize {
            return Err(SearchChoiceError::TooMany);
        }
        let mut seen: Vec<ObjectId> = Vec::with_capacity(chosen.len());
        for &id in &chosen {
            if seen.contains(&id) || !candidates.contains(&id) {
                return Err(SearchChoiceError::NotACandidate);
            }
            seen.push(id);
        }
        let ProcessState::AwaitingSearchChoice {
            stack_id,
            controller,
            events_so_far,
            destination,
            shuffle_after,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        self.process_state = self.continue_search_battlefield_entry(
            chosen.into(),
            destination,
            controller,
            stack_id,
            shuffle_after,
            events_so_far,
        );
        Ok(())
    }

    /// Moves the found cards one at a time so a pause for an entry-attach choice
    /// or a devour/amplify choice can resume with the unprocessed `remaining`
    /// (CR 303.4f, 702.82a, 702.38a). Returns the state for the caller to install.
    pub(in crate::engine) fn continue_search_battlefield_entry(
        &mut self,
        mut remaining: VecDeque<ObjectId>,
        destination: SearchDestination,
        controller: PlayerId,
        stack_id: ObjectId,
        shuffle_after: bool,
        mut events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        while let Some(card) = remaining.pop_front() {
            let last_known_info = self
                .state
                .object(card)
                .expect("validated against candidates above")
                .clone();
            let owner = last_known_info.owner;

            match destination {
                SearchDestination::Hand => {
                    let new_id = self.state.allocate_object_id();
                    let mut new_object = last_known_info.clone();
                    new_object.object_id = new_id;
                    new_object.lineage = ObjectLineage {
                        previous: Some(card),
                    };
                    new_object.timestamp = self.state.allocate_sequence_no();
                    new_object.zone = ZoneKind::Hand;
                    self.state
                        .zone_mut(ZoneKind::Library, Some(owner))
                        .expect("library zone must exist")
                        .remove(card);
                    let departed = commands::retire_object(&mut self.state, card);
                    self.departed_triggered_abilities.extend(departed);
                    self.state
                        .zone_mut(ZoneKind::Hand, Some(owner))
                        .expect("hand zone must exist")
                        .push_bottom(new_id);
                    self.state.objects.insert(new_id, new_object);
                    // CR 702.29b: a hand-only ability registers when the card reaches hand.
                    self.run_card_script_for(new_id);
                    events_so_far.push(GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Library,
                        to: ZoneKind::Hand,
                        last_known_info: Some(last_known_info),
                    });
                }
                SearchDestination::Battlefield {
                    tapped,
                    enters_attached,
                } => {
                    // CR 303.4f, 301.5e: computed before the permanent gets its new id so
                    // it can't be a candidate of its own scan.
                    let attach_candidates = enters_attached
                        .then(|| {
                            commands::legal_entry_attach_candidates(
                                &self.state,
                                &self.script_runtime,
                                &last_known_info.definition,
                                controller,
                                card,
                            )
                        })
                        .flatten();
                    if let Some(candidates) = &attach_candidates {
                        // CR 303.4g: an Aura with no legal target stays in the library.
                        // Equipment with none enters unattached (CR 301.5e).
                        if candidates.is_empty() && commands::is_aura(&last_known_info.definition) {
                            continue;
                        }
                    }

                    let new_id = self.state.allocate_object_id();
                    let mut new_object = last_known_info.clone();
                    new_object.object_id = new_id;
                    new_object.lineage = ObjectLineage {
                        previous: Some(card),
                    };
                    new_object.timestamp = self.state.allocate_sequence_no();
                    new_object.zone = ZoneKind::Battlefield;
                    new_object.tapped = tapped;
                    new_object.controlled_since_turn = self.state.turn_number;
                    self.state
                        .zone_mut(ZoneKind::Library, Some(owner))
                        .expect("library zone must exist")
                        .remove(card);
                    let definition_for_script = new_object.definition.clone();
                    self.state.objects.remove(&card);
                    self.state
                        .zone_mut(ZoneKind::Battlefield, None)
                        .expect("battlefield zone must exist")
                        .push_bottom(new_id);
                    self.state.objects.insert(new_id, new_object);
                    commands::remap_abilities(&mut self.state, card, new_id);
                    let result = self.script_runtime.run_card_script(
                        &mut self.state,
                        &definition_for_script,
                        new_id,
                    );
                    self.record_script_error(new_id, &definition_for_script.script_ref, result);
                    events_so_far.push(GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Library,
                        to: ZoneKind::Battlefield,
                        last_known_info: Some(last_known_info),
                    });
                    // CR 702.82a, 702.38a: devour is checked before amplify, and both before
                    // the enters-the-battlefield modifiers.
                    if let Some((per_creature, candidates)) = replacement::pending_devour_choice(
                        &self.state,
                        &self.script_runtime,
                        new_id,
                        ZoneKind::Library,
                        controller,
                    ) {
                        return ProcessState::AwaitingSearchDevourChoice {
                            object: new_id,
                            controller,
                            per_creature,
                            candidates,
                            stack_id,
                            events_so_far,
                            remaining,
                            destination,
                            shuffle_after,
                        };
                    }
                    if let Some((per_card, candidates)) = replacement::pending_amplify_choice(
                        &self.state,
                        &self.script_runtime,
                        new_id,
                        ZoneKind::Library,
                        controller,
                    ) {
                        return ProcessState::AwaitingSearchAmplifyChoice {
                            object: new_id,
                            controller,
                            per_card,
                            candidates,
                            stack_id,
                            events_so_far,
                            remaining,
                            destination,
                            shuffle_after,
                        };
                    }
                    // CR 603.6d: `tapped` is the search's own flag, independent of an
                    // enters-tapped ability; applying both is harmless.
                    let counter_events = replacement::apply_enters_battlefield_modifiers(
                        &mut self.state,
                        &self.script_runtime,
                        new_id,
                        ZoneKind::Library,
                        ColorSet::empty(),
                    );
                    events_so_far.extend(counter_events);

                    if let Some(candidates) = attach_candidates {
                        match candidates.len() {
                            // CR 301.5e: no legal object, so it enters unattached. An Aura
                            // with none was skipped above.
                            0 => {}
                            1 => match candidates[0] {
                                Target::Object(target_id) => {
                                    commands::attach_permanent(&mut self.state, new_id, target_id);
                                    events_so_far.push(GameEventKind::Attached {
                                        object: new_id,
                                        attached_to: target_id,
                                    });
                                }
                                Target::Player(player) => {
                                    commands::attach_permanent_to_player(
                                        &mut self.state,
                                        new_id,
                                        player,
                                    );
                                    events_so_far.push(GameEventKind::AttachedToPlayer {
                                        object: new_id,
                                        player,
                                    });
                                }
                                // Attach candidates are objects or players only.
                                Target::Ability(_) => {
                                    unreachable!("attach candidates are never abilities")
                                }
                            },
                            _ => {
                                // CR 303.4f: with 2+ candidates the entering player chooses; the
                                // permanent is already on the battlefield, unattached.
                                return ProcessState::AwaitingEntryAttachChoice {
                                    attaching: new_id,
                                    controller,
                                    candidates,
                                    stack_id,
                                    events_so_far,
                                    remaining,
                                    destination,
                                    shuffle_after,
                                };
                            }
                        }
                    }
                }
            }
        }

        if shuffle_after {
            self.shuffle_library(controller);
        }

        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        ProcessState::ReplacementCheck {
            remaining: events_so_far.into(),
            checked: Vec::new(),
        }
    }

    /// Answers an entry-attach pause (CR 303.4f) and resumes the search with the
    /// cards still unprocessed. `chosen` is a candidate object or player.
    pub fn submit_entry_attach_choice(&mut self, chosen: Target) -> Result<(), AttachChoiceError> {
        let ProcessState::AwaitingEntryAttachChoice { candidates, .. } = &self.process_state else {
            return Err(AttachChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(AttachChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingEntryAttachChoice {
            attaching,
            controller,
            stack_id,
            mut events_so_far,
            remaining,
            destination,
            shuffle_after,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        match chosen {
            Target::Object(target_id) => {
                commands::attach_permanent(&mut self.state, attaching, target_id);
                events_so_far.push(GameEventKind::Attached {
                    object: attaching,
                    attached_to: target_id,
                });
            }
            Target::Player(player) => {
                commands::attach_permanent_to_player(&mut self.state, attaching, player);
                events_so_far.push(GameEventKind::AttachedToPlayer {
                    object: attaching,
                    player,
                });
            }
            // Candidates hold objects and players only.
            Target::Ability(_) => unreachable!("attach candidates are never abilities"),
        }
        self.process_state = self.continue_search_battlefield_entry(
            remaining,
            destination,
            controller,
            stack_id,
            shuffle_after,
            events_so_far,
        );
        Ok(())
    }

    /// Answers a devour pause raised mid-search (CR 702.82a). The sacrifices
    /// resume the search afterwards instead of finalizing the resolution.
    pub fn submit_search_devour_choice(
        &mut self,
        sacrificed: Vec<ObjectId>,
    ) -> Result<(), DevourChoiceError> {
        let ProcessState::AwaitingSearchDevourChoice { candidates, .. } = &self.process_state
        else {
            return Err(DevourChoiceError::NotAwaitingChoice);
        };
        if !sacrificed.iter().all(|id| candidates.contains(id)) {
            return Err(DevourChoiceError::InvalidCandidate);
        }
        let mut seen = HashSet::new();
        if !sacrificed.iter().all(|id| seen.insert(*id)) {
            return Err(DevourChoiceError::DuplicateCandidate);
        }
        let ProcessState::AwaitingSearchDevourChoice {
            object,
            controller,
            per_creature,
            mut events_so_far,
            remaining,
            destination,
            stack_id,
            shuffle_after,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        let candidate_event = GameEventKind::ZoneChange {
            object,
            from: ZoneKind::Library,
            to: ZoneKind::Battlefield,
            last_known_info: None,
        };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        let amount = per_creature * sacrificed.len() as i32;
        if amount > 0 {
            events_so_far.push(GameEventKind::CounterAdded {
                object,
                kind: CounterKind::PlusOnePlusOne,
                amount,
            });
        }
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Sacrifice,
            sacrificed.into(),
            ScriptedZoneMoveThen::SearchResume {
                controller,
                remaining,
                destination,
                stack_id,
                shuffle_after,
            },
            events_so_far,
        );
        Ok(())
    }

    /// Answers an amplify pause raised mid-search (CR 702.38a). Revealing moves
    /// nothing, so it resumes the search directly.
    pub fn submit_search_amplify_choice(
        &mut self,
        revealed: Vec<ObjectId>,
    ) -> Result<(), AmplifyChoiceError> {
        let ProcessState::AwaitingSearchAmplifyChoice { candidates, .. } = &self.process_state
        else {
            return Err(AmplifyChoiceError::NotAwaitingChoice);
        };
        if !revealed.iter().all(|id| candidates.contains(id)) {
            return Err(AmplifyChoiceError::InvalidCandidate);
        }
        let mut seen = HashSet::new();
        if !revealed.iter().all(|id| seen.insert(*id)) {
            return Err(AmplifyChoiceError::DuplicateCandidate);
        }
        let ProcessState::AwaitingSearchAmplifyChoice {
            object,
            controller,
            per_card,
            mut events_so_far,
            remaining,
            destination,
            stack_id,
            shuffle_after,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        let candidate_event = GameEventKind::ZoneChange {
            object,
            from: ZoneKind::Library,
            to: ZoneKind::Battlefield,
            last_known_info: None,
        };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        let amount = per_card * revealed.len() as i32;
        if amount > 0 {
            events_so_far.push(GameEventKind::CounterAdded {
                object,
                kind: CounterKind::PlusOnePlusOne,
                amount,
            });
        }
        self.process_state = self.continue_search_battlefield_entry(
            remaining,
            destination,
            controller,
            stack_id,
            shuffle_after,
            events_so_far,
        );
        Ok(())
    }
}
