//! Pregame setup: building an `Engine` from a `GameConfig`, opening hands and
//! the mulligan loop. See `DESIGN.md`.

use std::collections::{HashMap, VecDeque};

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::card::ScriptReference;
use crate::types::command::{PlayerAction, PlayerActionKind};
use crate::types::config::GameConfig;
use crate::types::counter::CounterSet;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::{GameState, RngState};
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, SequenceNo};
use crate::types::mana::ManaPool;
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::player::PlayerState;
use crate::types::target::TargetSpec;
use crate::types::turn::{Phase, Step};
use crate::types::zone::{Zone, ZoneKind};

use super::{BottomCardsError, Engine, MulliganError, ProcessState};

impl Engine {
    /// Builds an `Engine` from `config`, starting in `Phase::PreGame`. The first
    /// `step()` runs `run_pregame_setup`.
    pub fn new_game(config: GameConfig) -> Engine {
        let settings = config.settings;
        let starting_player = config.turn_order[0];

        let mut players: Vec<PlayerState> = Vec::new();
        let mut zones: Vec<Zone> = Vec::new();
        let mut objects: HashMap<ObjectId, GameObject> = HashMap::new();
        let mut next_object_id: u64 = 0;
        let mut next_sequence_no: u64 = 0;

        for player_cfg in &config.players {
            let pid = player_cfg.player_id;
            let life = player_cfg
                .life_override
                .unwrap_or(settings.starting_life) as i32;

            players.push(PlayerState {
                player_id: pid,
                life,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            });

            let mut lib_zone = Zone::new(ZoneKind::Library, Some(pid));
            for card_def in &player_cfg.library {
                let obj_id = ObjectId(next_object_id);
                next_object_id += 1;
                let ts = SequenceNo(next_sequence_no);
                next_sequence_no += 1;
                let obj = GameObject {
                    object_id: obj_id,
                    lineage: ObjectLineage { previous: None },
                    definition: card_def.clone(),
                    copiable_override: None,
                    owner: pid,
                    controller: pid,
                    zone: ZoneKind::Library,
                    tapped: false,
                    flipped: false,
                    face_down: false,
                    phased_out: false,
                    damage_marked: 0,
                    deathtouched: false,
                    counters: CounterSet::new(),
                    attachments: Vec::new(),
                    attached_to: None, attached_to_player: None,
                    timestamp: ts,
                    controlled_since_turn: 1,
                    is_token: false,
                    is_commander: false,
                    visibility: VisibilityPolicy { public: false, visible_to: Vec::new() },
                    copy_generation: 0,
                    entered_via_escape: false,
                };
                lib_zone.contents.push_back(obj_id);
                objects.insert(obj_id, obj);
            }
            zones.push(lib_zone);
            zones.push(Zone::new(ZoneKind::Hand, Some(pid)));
            zones.push(Zone::new(ZoneKind::Graveyard, Some(pid)));

            // CR 903.6: commanders start in the command zone, not the library.
            let mut command_zone = Zone::new(ZoneKind::Command, Some(pid));
            for card_def in &player_cfg.commanders {
                let obj_id = ObjectId(next_object_id);
                next_object_id += 1;
                let ts = SequenceNo(next_sequence_no);
                next_sequence_no += 1;
                let obj = GameObject {
                    object_id: obj_id,
                    lineage: ObjectLineage { previous: None },
                    definition: card_def.clone(),
                    copiable_override: None,
                    owner: pid,
                    controller: pid,
                    zone: ZoneKind::Command,
                    tapped: false,
                    flipped: false,
                    face_down: false,
                    phased_out: false,
                    damage_marked: 0,
                    deathtouched: false,
                    counters: CounterSet::new(),
                    attachments: Vec::new(),
                    attached_to: None, attached_to_player: None,
                    timestamp: ts,
                    controlled_since_turn: 1,
                    is_token: false,
                    is_commander: true,
                    // CR 408: command zone contents are public.
                    visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
                    copy_generation: 0,
                    entered_via_escape: false,
                };
                command_zone.contents.push_back(obj_id);
                objects.insert(obj_id, obj);
            }
            zones.push(command_zone);
        }

        zones.push(Zone::new(ZoneKind::Battlefield, None));
        zones.push(Zone::new(ZoneKind::Exile, None));

        let state = GameState {
            players,
            turn_order: config.turn_order.clone(),
            active_player: starting_player,
            turn_number: 1,
            phase: Phase::PreGame,
            step: Step::Untap,
            priority_player: starting_player,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects,
            zones,
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: config.rng_seed, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no,
            next_object_id,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        };

        let mut engine = Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            departed_triggered_abilities: Vec::new(),
            recollect_triggers_since: None,
            next_command_id: 0,
            fatal: None,
            pending_payment: None,
            modal_continuation: None,
            suspended_payments: Vec::new(),
            settings,
            pregame_setup_done: false,
            script_runtime: ScriptRuntime::new(),
            decisions: crate::decision::DecisionTracker::new(),
        };
        engine.load_script_declarations();
        engine
    }

    /// Copies a script's `operation()` and `target_requirement()` onto each card's
    /// definition once per distinct script, since a spell is cast from hand before
    /// any script has run. A value already set in Rust wins. Values must come from
    /// this engine's runtime because a custom-function `ScriptHandle` is only valid there.
    // TODO: load `alternative_costs()` and `buyback_cost()` script declarations
    fn load_script_declarations(&mut self) {
        // Sorted: `objects` is a `HashMap`, and a failing script logs an event.
        let mut ids: Vec<ObjectId> = self.state.objects.keys().copied().collect();
        ids.sort();
        let mut loaded: HashMap<String, (Option<AbilityEffect>, Option<TargetSpec>)> = HashMap::new();
        for id in ids {
            let definition = &self.state.objects[&id].definition;
            let ScriptReference::Inline(source) = &definition.script_ref else { continue };
            if source.trim().is_empty() || (definition.effect.is_some() && definition.target_requirement.is_some()) {
                continue;
            }
            let (script_ref, source) = (definition.script_ref.clone(), source.clone());
            let (effect, requirement) = match loaded.get(&source) {
                Some(found) => found.clone(),
                None => {
                    let operation = self.script_runtime.build_operation(&source);
                    let requirement = self.script_runtime.build_target_requirement(&source);
                    // A `ChainEach` must own exactly the words the card declares.
                    let words_check = match (&operation, &requirement) {
                        (Ok(Some((Some(effect), _))), Ok(requirement)) => {
                            crate::script::check_chain_words(Some(effect), requirement.as_ref())
                        }
                        _ => Ok(()),
                    };
                    let failed = operation.is_err() || requirement.is_err() || words_check.is_err();
                    let effect = match &operation {
                        // A chain whose word counts don't match resolves nothing.
                        _ if words_check.is_err() => None,
                        Ok(Some((primitive, handle))) => primitive.clone().or(Some(AbilityEffect::Script(*handle))),
                        _ => None,
                    };
                    let requirement_ok = requirement.as_ref().ok().cloned().flatten();
                    self.record_script_error(id, &script_ref, operation.map(|_| ()));
                    self.record_script_error(id, &script_ref, requirement.map(|_| ()));
                    self.record_script_error(id, &script_ref, words_check);
                    // A script that failed is not cached, so every copy logs it.
                    if !failed {
                        loaded.insert(source, (effect.clone(), requirement_ok.clone()));
                    }
                    (effect, requirement_ok)
                }
            };
            let definition = &mut self.state.objects.get_mut(&id).expect("id came from the map").definition;
            if definition.effect.is_none() {
                definition.effect = effect;
            }
            if definition.target_requirement.is_none() {
                definition.target_requirement = requirement;
            }
        }
    }

    /// Accepts keep or mulligan from the player being asked (CR 103.5). A mulligan
    /// reshuffles and redraws immediately.
    pub fn submit_mulligan_decision(&mut self, action: PlayerAction) -> Result<(), MulliganError> {
        let ProcessState::AwaitingMulliganDecision { player } = self.process_state else {
            return Err(MulliganError::NotAwaitingDecision);
        };
        if action.player != player {
            return Err(MulliganError::WrongPlayer);
        }
        match action.kind {
            PlayerActionKind::KeepHand => {
                self.state.player_mut(player).expect("valid player").hand_kept = true;
                match self.next_mulligan_player() {
                    Some(next) => {
                        self.process_state = ProcessState::AwaitingMulliganDecision { player: next };
                    }
                    None => self.transition_to_bottoming(),
                }
            }
            PlayerActionKind::TakeMulligan => {
                let taken = self.state.player(player).expect("valid player").mulligans_taken;
                if let Some(max) = self.settings.max_mulligans
                    && taken >= max {
                        return Err(MulliganError::MaxMulligansReached);
                    }
                self.state.player_mut(player).expect("valid player").mulligans_taken += 1;
                self.return_hand_to_library(player);
                self.shuffle_library(player);
                self.draw_opening_hand(player);
                // The mulliganing player is still undecided, so `next_mulligan_player`
                // would return them again.
                let next = self.next_mulligan_player_after(player);
                self.process_state =
                    ProcessState::AwaitingMulliganDecision { player: next.expect("player must be in turn_order") };
            }
            _ => return Err(MulliganError::InvalidAction),
        }
        Ok(())
    }

    /// Accepts the cards a player puts on the bottom after keeping a paid mulligan.
    /// Validates count and membership, then advances to the next bottoming player.
    pub fn submit_bottom_cards(&mut self, action: PlayerAction) -> Result<(), BottomCardsError> {
        let ProcessState::AwaitingBottomCardChoice { player, count, .. } = self.process_state else {
            return Err(BottomCardsError::NotAwaitingChoice);
        };
        if action.player != player {
            return Err(BottomCardsError::WrongPlayer);
        }
        let PlayerActionKind::BottomCards { cards } = action.kind else {
            return Err(BottomCardsError::WrongCount { expected: count, got: 0 });
        };
        if cards.len() != count {
            return Err(BottomCardsError::WrongCount { expected: count, got: cards.len() });
        }
        let mut seen: Vec<ObjectId> = Vec::with_capacity(cards.len());
        for &card in &cards {
            if seen.contains(&card) {
                return Err(BottomCardsError::DuplicateCard);
            }
            seen.push(card);
        }
        for &card in &cards {
            let obj = self.state.object(card).ok_or(BottomCardsError::CardNotInHand)?;
            if obj.zone != ZoneKind::Hand || obj.owner != player {
                return Err(BottomCardsError::CardNotInHand);
            }
        }
        // CR 400.7: each move mints a new id.
        for card in &cards {
            let card = *card;
            let last_known = self.state.object(card).expect("validated above").clone();
            let owner = last_known.owner;
            let new_id = self.state.allocate_object_id();
            let mut new_obj = last_known.clone();
            new_obj.object_id = new_id;
            new_obj.lineage = ObjectLineage { previous: Some(card) };
            new_obj.zone = ZoneKind::Library;
            new_obj.timestamp = self.state.allocate_sequence_no();
            self.state.zone_mut(ZoneKind::Hand, Some(player)).expect("hand zone").remove(card);
            self.state.objects.remove(&card);
            self.state.zone_mut(ZoneKind::Library, Some(owner)).expect("library zone").push_bottom(new_id);
            self.state.objects.insert(new_id, new_obj);
            commands::remap_abilities(&mut self.state, card, new_id);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Library,
                    last_known_info: Some(last_known),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
        let ProcessState::AwaitingBottomCardChoice { mut remaining, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        match remaining.pop_front() {
            Some((next_player, next_count)) => {
                self.process_state = ProcessState::AwaitingBottomCardChoice {
                    player: next_player,
                    count: next_count,
                    remaining,
                };
            }
            None => {
                self.process_state = ProcessState::AdvanceTurnStep;
            }
        }
        Ok(())
    }

    /// Shuffles, draws opening hands, then asks for mulligans or skips straight
    /// to the first turn when `settings.skip_mulligan` is set.
    pub(super) fn run_pregame_setup(&mut self) {
        let turn_order: Vec<PlayerId> = self.state.turn_order.clone();
        if self.settings.shuffle_libraries {
            for &pid in &turn_order {
                self.shuffle_library(pid);
            }
        }
        for &pid in &turn_order {
            self.draw_opening_hand(pid);
        }
        if self.settings.skip_mulligan {
            for &pid in &turn_order {
                self.state.player_mut(pid).expect("valid player").hand_kept = true;
            }
            self.process_state = ProcessState::AdvanceTurnStep;
        } else {
            let first = turn_order[0];
            self.process_state = ProcessState::AwaitingMulliganDecision { player: first };
        }
    }

    /// Fisher-Yates shuffle of `player`'s library from `state.rng`, logging an
    /// `RngDrawn` per draw so a replay is deterministic (CR 701.24a).
    pub(in crate::engine) fn shuffle_library(&mut self, player: PlayerId) {
        let len = self.state.zone(ZoneKind::Library, Some(player)).map_or(0, |z| z.contents.len());
        // Swaps are drawn first because `next_rng` can't run while the zone is borrowed.
        let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(len);
        for i in (1..len).rev() {
            let rng_val = self.next_rng();
            let j = (rng_val % (i as u64 + 1)) as usize;
            swaps.push((i, j));
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::RngDrawn { purpose: "shuffle".to_string(), value: rng_val },
                caused_by_command: None,
                parent_event: None,
            });
        }
        let lib = self.state.zone_mut(ZoneKind::Library, Some(player)).expect("library zone");
        for (i, j) in swaps {
            lib.contents.swap(i, j);
        }
        let event_id = self.state.allocate_event_id();
        let seq = self.state.allocate_sequence_no();
        self.state.ledger.push(GameEvent {
            event_id,
            sequence_no: seq,
            kind: GameEventKind::LibraryShuffled { player },
            caused_by_command: None,
            parent_event: None,
        });
    }

    /// CR 702.85a: puts the exiled `cards` on the bottom of their owners' libraries
    /// in random order. Library moves aren't replaceable, so there is no
    /// replacement check.
    pub(in crate::engine) fn bottom_cards_in_random_order(&mut self, mut cards: Vec<ObjectId>) {
        for i in (1..cards.len()).rev() {
            let rng_val = self.next_rng();
            let j = (rng_val % (i as u64 + 1)) as usize;
            cards.swap(i, j);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::RngDrawn { purpose: "cascade-bottom".to_string(), value: rng_val },
                caused_by_command: None,
                parent_event: None,
            });
        }
        for card in cards {
            if let Some(proposed) = commands::propose_zone_move(&self.state, card, ZoneKind::Exile, ZoneKind::Library) {
                let (committed, departed) = commands::commit_zone_move(&mut self.state, proposed);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(committed);
            }
        }
    }

    /// Puts the hand back on top of the library before a mulligan redraw; CR 103.5
    /// doesn't constrain the order.
    fn return_hand_to_library(&mut self, player: PlayerId) {
        let hand_ids: Vec<ObjectId> = self.state
            .zone(ZoneKind::Hand, Some(player))
            .map_or(Vec::new(), |z| z.contents.iter().copied().collect());
        for card in hand_ids {
            let last_known = self.state.object(card).expect("hand card must exist").clone();
            let owner = last_known.owner;
            let new_id = self.state.allocate_object_id();
            let mut new_obj = last_known.clone();
            new_obj.object_id = new_id;
            new_obj.lineage = ObjectLineage { previous: Some(card) };
            new_obj.zone = ZoneKind::Library;
            new_obj.timestamp = self.state.allocate_sequence_no();
            self.state.zone_mut(ZoneKind::Hand, Some(player)).expect("hand zone").remove(card);
            self.state.objects.remove(&card);
            self.state.zone_mut(ZoneKind::Library, Some(owner)).expect("library zone").push_top(new_id);
            self.state.objects.insert(new_id, new_obj);
            commands::remap_abilities(&mut self.state, card, new_id);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Library,
                    last_known_info: Some(last_known),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
    }

    fn draw_opening_hand(&mut self, player: PlayerId) {
        for _ in 0..self.settings.starting_hand_size {
            let Some(card) = self.state.zone(ZoneKind::Library, Some(player))
                .and_then(|z| z.contents.front().copied())
            else {
                break;
            };
            let last_known = self.state.object(card).expect("library card must exist").clone();
            let new_id = self.state.allocate_object_id();
            let mut new_obj = last_known.clone();
            new_obj.object_id = new_id;
            new_obj.lineage = ObjectLineage { previous: Some(card) };
            new_obj.zone = ZoneKind::Hand;
            new_obj.timestamp = self.state.allocate_sequence_no();
            self.state.zone_mut(ZoneKind::Library, Some(player)).expect("library zone").remove(card);
            self.state.objects.remove(&card);
            self.state.zone_mut(ZoneKind::Hand, Some(player)).expect("hand zone").push_bottom(new_id);
            self.state.objects.insert(new_id, new_obj);
            commands::remap_abilities(&mut self.state, card, new_id);
            // CR 702.29b: a hand-only ability registers when the card reaches hand.
            self.run_card_script_for(new_id);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Library,
                    to: ZoneKind::Hand,
                    last_known_info: Some(last_known),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
    }

    fn next_mulligan_player(&self) -> Option<PlayerId> {
        self.state.turn_order.iter().copied()
            .find(|&pid| !self.state.player(pid).is_some_and(|p| p.hand_kept))
    }

    /// The next undecided player after `current` in turn order, wrapping to
    /// `current` if they are the only one left.
    fn next_mulligan_player_after(&self, current: PlayerId) -> Option<PlayerId> {
        let order = &self.state.turn_order;
        let n = order.len();
        let cur_pos = order.iter().position(|&p| p == current)?;
        for i in 1..=n {
            let pid = order[(cur_pos + i) % n];
            if !self.state.player(pid).is_some_and(|p| p.hand_kept) {
                return Some(pid);
            }
        }
        None
    }

    /// Advances `state.rng` (splitmix64) and returns the draw. Every consumer goes
    /// through here so the sequence stays deterministic from the seed.
    pub(in crate::engine) fn next_rng(&mut self) -> u64 {
        self.state.rng.draws_consumed += 1;
        let mut z = self.state.rng.seed.wrapping_add(
            0x9e3779b97f4a7c15u64.wrapping_mul(self.state.rng.draws_consumed),
        );
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Queues each player with paid mulligans (taken minus free) to bottom cards,
    /// or goes straight to the first turn when nobody owes any.
    fn transition_to_bottoming(&mut self) {
        let free = self.settings.free_mulligans;
        let mut queue: VecDeque<(PlayerId, usize)> = self.state.turn_order.iter().copied()
            .filter_map(|pid| {
                let taken = self.state.player(pid).map_or(0, |p| p.mulligans_taken);
                let paid = taken.saturating_sub(free) as usize;
                if paid > 0 { Some((pid, paid)) } else { None }
            })
            .collect();
        match queue.pop_front() {
            Some((first, count)) => {
                self.process_state = ProcessState::AwaitingBottomCardChoice {
                    player: first,
                    count,
                    remaining: queue,
                };
            }
            None => {
                self.process_state = ProcessState::AdvanceTurnStep;
            }
        }
    }
}
