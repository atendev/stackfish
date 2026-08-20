//! Answers to CR 701.22/701.23/701.25 library-manipulation pauses (Scry,
//! Surveil, Search) and the CR 303.4f/301.5e/301.6 entry-attach choice a
//! battlefield-destined search can raise mid-loop — split out of
//! `engine.rs`.

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
    AmplifyChoiceError, AttachChoiceError, DevourChoiceError, Engine, ProcessState, ScriptedZoneMoveKind,
    ScriptedZoneMoveThen, ScryChoiceError, SearchChoiceError, SurveilChoiceError,
};

impl Engine {
    /// Answers a pending CR 701.22 `Game.Ops.Scry(n)` pause raised by
    /// `StepStatus::AwaitingScryChoice` — `kept_on_top`/`put_bottom`
    /// together must contain exactly the pending `cards` set once each (see
    /// `is_exact_partition`), each in the caller's chosen order.
    /// `kept_on_top[0]` becomes the new top of the library; `put_bottom`'s
    /// order stacks upward from the library's existing bottom (so
    /// `put_bottom`'s last entry ends up the new absolute bottom card).
    /// Deliberately a separate entry point from `submit_action`, same reason
    /// `submit_replacement_choice`/`submit_legend_rule_choice` are.
    ///
    /// Pure reorder within the library — no zone change, no new `ObjectId`s
    /// (same "mutate `Zone.contents` directly" convention `shuffle_library`
    /// already uses), so replacement effects never see this at all (there's
    /// nothing in real Magic that replaces a scry anyway).
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
        let ProcessState::AwaitingScryChoice { stack_id, controller, mut events_so_far, cards } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
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
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 701.25 `Game.Ops.Surveil(n)` pause raised by
    /// `StepStatus::AwaitingSurveilChoice` — same shape as
    /// `submit_scry_choice` except `put_graveyard` is a real library ->
    /// graveyard move (CR 400.7 new `ObjectId` per card), same convention
    /// `submit_discard` already established, rather than a bottom-of-library
    /// reorder. Departed triggered abilities are stashed for consistency
    /// (same "captured but practically dormant, no Hand/Library-departure
    /// LKI condition exists yet" stance `retire_object`'s own doc comment
    /// already takes for `draw_step`'s library-side retirement).
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
        let ProcessState::AwaitingSurveilChoice { stack_id, controller, mut events_so_far, cards } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // Remove every offered card from the library up front (both
        // piles), then put the kept-on-top pile straight back - a pure
        // reorder, no zone change, no new ObjectIds (same convention
        // submit_scry_choice's own top pile uses).
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
            let last_known_info =
                self.state.object(*card).expect("validated by is_exact_partition above").clone();
            let owner = last_known_info.owner;

            let new_id = self.state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(*card) };
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
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 701.23 `Game.Ops.Search(...)` pause raised by
    /// `StepStatus::AwaitingSearchChoice` — `chosen` must be a subset of the
    /// pending `candidates` (already pre-filtered against the search's own
    /// `FilterSpec`, so no re-filtering happens here), no duplicates, and no
    /// more than `count` entries (CR 701.23b: fewer, including zero, is
    /// always legal). Moves each found card Library -> the pause's own
    /// `destination` (CR 400.7 new `ObjectId` per card): `Hand` mirrors
    /// `submit_surveil_choice`'s graveyard-move convention (`retire_object`,
    /// no script); `Battlefield` mirrors `move_resolving_spell`'s own
    /// battlefield-entry convention instead (`remap_abilities`, a real ETB
    /// `run_card_script`, `controlled_since_turn` set to the current turn,
    /// `tapped` applied as requested). Shuffles the library afterward iff
    /// `shuffle_after` (CR 701.24a), via the existing `shuffle_library`
    /// helper — same "already logs its own events" convention
    /// `AbilityEffect::Shuffle` itself already relies on.
    pub fn submit_search_choice(&mut self, chosen: Vec<ObjectId>) -> Result<(), SearchChoiceError> {
        let ProcessState::AwaitingSearchChoice { candidates, count, .. } = &self.process_state else {
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
            stack_id, controller, events_so_far, destination, shuffle_after, ..
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

    /// The real body of `submit_search_choice`'s battlefield/hand-entry
    /// loop, factored out so `submit_entry_attach_choice` can resume it
    /// after answering a CR 303.4f/301.5e/301.6 pause raised partway
    /// through. Processes `remaining` one card at a time (`VecDeque::
    /// pop_front`, so a card already processed before a pause is never
    /// revisited); if a `SearchDestination::Battlefield { enters_attached:
    /// true, .. }` Aura/Equipment/Fortification entry finds 2+ legal attach
    /// candidates (`commands::legal_entry_attach_candidates`), this returns
    /// `ProcessState::AwaitingEntryAttachChoice` with everything still needed
    /// to pick back up (`remaining`, `destination`, the accumulated
    /// `events_so_far`, ...) - `submit_entry_attach_choice` calls back in
    /// with the same `remaining` queue once the choice is answered. A CR
    /// 702.82a Devour / CR 702.38a Amplify choice (`phase5/devour-amplify-
    /// search-entry`) pauses the same way, via `AwaitingSearchDevourChoice`/
    /// `AwaitingSearchAmplifyChoice` - see `replacement::pending_devour_
    /// choice`'s own doc comment for why this producer, unlike `PlayLand`/
    /// `CreateToken`, is wired up. Reaching the end of `remaining` with no
    /// pause finalizes exactly like the old inline loop did: `shuffle_after`,
    /// then `StackObjectResolved`, then `ReplacementCheck`.
    ///
    /// Returns the `ProcessState` to install - callers assign it to
    /// `self.process_state` themselves, same convention `continue_scripted_
    /// zone_move`/`continue_create_token` already use (rather than mutating
    /// `self.process_state` directly), since `Engine::continue_scripted_
    /// zone_move`'s own `ScriptedZoneMoveThen::SearchResume` arm needs to
    /// call this and use the result without an intervening `self.process_
    /// state` write of its own.
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
            let last_known_info =
                self.state.object(card).expect("validated against candidates above").clone();
            let owner = last_known_info.owner;

            match destination {
                SearchDestination::Hand => {
                    let new_id = self.state.allocate_object_id();
                    let mut new_object = last_known_info.clone();
                    new_object.object_id = new_id;
                    new_object.lineage = ObjectLineage { previous: Some(card) };
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
                    // CR 702.29b: a tutored-to-hand card's own script
                    // self-registers any hand-only ability now, same as
                    // every other Library->Hand producer.
                    self.run_card_script_for(new_id);
                    events_so_far.push(GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Library,
                        to: ZoneKind::Hand,
                        last_known_info: Some(last_known_info),
                    });
                }
                SearchDestination::Battlefield { tapped, enters_attached } => {
                    // CR 303.4f/g/301.5e/301.6: computed *before* any state
                    // mutation, and before this permanent's own new
                    // `ObjectId` even exists - a not-yet-inserted permanent
                    // can never appear as a candidate of its own scan (see
                    // `legal_entry_attach_candidates`'s own doc comment for
                    // why that's the right call, not a gap).
                    // `card` (the pre-move library id) is passed as the
                    // "source" for CR 115.4's "another target" exclusion -
                    // harmless either way, since a library object is never
                    // battlefield-zoned and so never a legal candidate in
                    // the first place. `new_id` doesn't exist yet at this
                    // point, so there's nothing more precise to pass.
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
                        // CR 303.4g: an Aura with no legal enchant target,
                        // whose current zone (library) isn't the stack,
                        // remains right where it is - nothing to move; skip
                        // straight to the next card. Equipment/Fortification
                        // with zero legal candidates instead still enters
                        // the battlefield unattached (CR 301.5e/301.6) - that
                        // case falls through to the normal move below and is
                        // handled by the `0 =>` arm further down.
                        if candidates.is_empty() && commands::is_aura(&last_known_info.definition) {
                            continue;
                        }
                    }

                    let new_id = self.state.allocate_object_id();
                    let mut new_object = last_known_info.clone();
                    new_object.object_id = new_id;
                    new_object.lineage = ObjectLineage { previous: Some(card) };
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
                    let result = self.script_runtime.run_card_script(&mut self.state, &definition_for_script, new_id);
                    self.record_script_error(new_id, &definition_for_script.script_ref, result);
                    events_so_far.push(GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Library,
                        to: ZoneKind::Battlefield,
                        last_known_info: Some(last_known_info),
                    });
                    // CR 702.82a/702.38a (`phase5/devour-amplify-search-
                    // entry`): checked before `apply_enters_battlefield_
                    // modifiers`, same "only the call site that can actually
                    // pause should call this" posture `continue_permanent_
                    // entry`'s own resolving-spell path already established
                    // - see `pending_devour_choice`/`pending_amplify_choice`'s
                    // own doc comments. Devour checked first, matching that
                    // same sibling ordering.
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
                    // CR 603.6d/614.1c: same post-script apply step
                    // `move_resolving_spell`'s caller and the PlayLand
                    // battlefield_entries loop both take - `tapped` above is
                    // this search's own explicit destination flag (CR
                    // 701.23), unrelated to and independent of a card's own
                    // "enters tapped" ability, so both can stack (either
                    // already tapping it is harmless - EntersTapped is
                    // idempotent).
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
                            // CR 301.5e/301.6: no legal object to attach
                            // to - the Equipment/Fortification (an Aura
                            // never reaches here with an empty list, see the
                            // early `continue` above) just enters unattached.
                            // Nothing more to do.
                            0 => {}
                            1 => match candidates[0] {
                                Target::Object(target_id) => {
                                    commands::attach_permanent(&mut self.state, new_id, target_id);
                                    events_so_far
                                        .push(GameEventKind::Attached { object: new_id, attached_to: target_id });
                                }
                                Target::Player(player) => {
                                    commands::attach_permanent_to_player(&mut self.state, new_id, player);
                                    events_so_far.push(GameEventKind::AttachedToPlayer { object: new_id, player });
                                }
                                // Attach candidates are only ever objects or
                                // players (legal_entry_attach_candidates),
                                // never an ability.
                                Target::Ability(_) => unreachable!("attach candidates are never abilities"),
                            },
                            _ => {
                                // CR 303.4f: 2+ legal candidates - the
                                // entering player chooses. `new_id` is
                                // already a real battlefield permanent
                                // (unattached) at this point, same "already
                                // a permanent, then gets attached" order the
                                // cast-time Aura-ETB-attach path uses.
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
        ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
    }

    /// Answers a pending CR 303.4f/301.5e/301.6 entry attach choice raised by
    /// `StepStatus::AwaitingEntryAttachChoice` — `chosen` must be one of
    /// that status's `candidates`. Unlike `submit_attach_choice`, this
    /// resumes a real continuation: `continue_search_battlefield_entry`
    /// picks back up with whatever cards were still left in the search that
    /// raised this pause. `chosen` is `Target`-shaped since
    /// `phase5/aura-enters-attached-player` — an Aura's own candidates can
    /// be players (CR 303.4f/b); dispatches to `attach_permanent`/
    /// `attach_permanent_to_player` the same way `ResolveStackTop`'s own
    /// cast-time attach branch already does.
    pub fn submit_entry_attach_choice(&mut self, chosen: Target) -> Result<(), AttachChoiceError> {
        let ProcessState::AwaitingEntryAttachChoice { candidates, .. } = &self.process_state else {
            return Err(AttachChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(AttachChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingEntryAttachChoice {
            attaching, controller, stack_id, mut events_so_far, remaining, destination, shuffle_after, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        match chosen {
            Target::Object(target_id) => {
                commands::attach_permanent(&mut self.state, attaching, target_id);
                events_so_far.push(GameEventKind::Attached { object: attaching, attached_to: target_id });
            }
            Target::Player(player) => {
                commands::attach_permanent_to_player(&mut self.state, attaching, player);
                events_so_far.push(GameEventKind::AttachedToPlayer { object: attaching, player });
            }
            // `candidates` only ever holds Object/Player entries (checked
            // above via candidates.contains).
            Target::Ability(_) => unreachable!("attach candidates are never abilities"),
        }
        self.process_state =
            self.continue_search_battlefield_entry(remaining, destination, controller, stack_id, shuffle_after, events_so_far);
        Ok(())
    }

    /// Answers a pending CR 702.82a Devour choice raised mid-`Search` by
    /// `StepStatus::AwaitingSearchDevourChoice` (`phase5/devour-amplify-
    /// search-entry`) — same validation shape as `submit_devour_choice`
    /// (`sacrificed` any subset of `candidates`, no duplicates), and the same
    /// `apply_one_replacement`/`CounterAdded`/`continue_scripted_zone_move`
    /// sequence, except the `candidate_event`'s `from` is `ZoneKind::Library`
    /// (matching how `pending_devour_choice` found this effect in the first
    /// place, not `ZoneKind::Stack`), and the sacrifice's own finalization is
    /// `ScriptedZoneMoveThen::SearchResume` rather than `StackResolution` -
    /// once the sacrifices (and any nested CR 616.1 ordering choice they
    /// themselves raise) settle, control returns to `continue_search_
    /// battlefield_entry`'s own still-pending candidates instead of
    /// finalizing early.
    pub fn submit_search_devour_choice(&mut self, sacrificed: Vec<ObjectId>) -> Result<(), DevourChoiceError> {
        let ProcessState::AwaitingSearchDevourChoice { candidates, .. } = &self.process_state else {
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
        let candidate_event =
            GameEventKind::ZoneChange { object, from: ZoneKind::Library, to: ZoneKind::Battlefield, last_known_info: None };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        let amount = per_creature * sacrificed.len() as i32;
        if amount > 0 {
            events_so_far.push(GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount });
        }
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Sacrifice,
            sacrificed.into(),
            ScriptedZoneMoveThen::SearchResume { controller, remaining, destination, stack_id, shuffle_after },
            events_so_far,
        );
        Ok(())
    }

    /// Answers a pending CR 702.38a Amplify choice raised mid-`Search` by
    /// `StepStatus::AwaitingSearchAmplifyChoice` (`phase5/devour-amplify-
    /// search-entry`) — same relationship to `submit_amplify_choice` that
    /// `submit_search_devour_choice` above has to `submit_devour_choice`.
    /// Unlike Devour, revealing doesn't move anything, so this resumes
    /// straight into `continue_search_battlefield_entry` rather than
    /// `continue_scripted_zone_move`.
    pub fn submit_search_amplify_choice(&mut self, revealed: Vec<ObjectId>) -> Result<(), AmplifyChoiceError> {
        let ProcessState::AwaitingSearchAmplifyChoice { candidates, .. } = &self.process_state else {
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
        let candidate_event =
            GameEventKind::ZoneChange { object, from: ZoneKind::Library, to: ZoneKind::Battlefield, last_known_info: None };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        let amount = per_card * revealed.len() as i32;
        if amount > 0 {
            events_so_far.push(GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount });
        }
        self.process_state =
            self.continue_search_battlefield_entry(remaining, destination, controller, stack_id, shuffle_after, events_so_far);
        Ok(())
    }
}
