//! CR 103.5 pregame setup: building a fresh `Engine` from a `GameConfig`,
//! shuffling/opening-hand-drawing, and the mulligan/bottoming decision loop
//! — split out of `engine.rs`.

use std::collections::{HashMap, VecDeque};

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::script::ScriptRuntime;
use crate::types::command::{PlayerAction, PlayerActionKind};
use crate::types::config::GameConfig;
use crate::types::counter::CounterSet;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::{GameState, RngState};
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, SequenceNo};
use crate::types::mana::ManaPool;
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::player::PlayerState;
use crate::types::turn::{Phase, Step};
use crate::types::zone::{Zone, ZoneKind};

use super::{BottomCardsError, Engine, MulliganError, ProcessState};

impl Engine {
    /// Builds a fresh `Engine` from a `GameConfig`: creates zones and
    /// `GameObject`s for each player's library, sets starting life from
    /// `config.settings.starting_life` (or per-player `life_override`), and
    /// starts in `Phase::PreGame` / `ProcessState::PriorityWindow`. The first
    /// `step()` call will enter `run_pregame_setup`.
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

            // CR 903.6: commander(s) go to the command zone, not the
            // library - minted the same way library cards just were above,
            // just with is_commander: true and a different destination.
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
                    // CR 408: the command zone's contents are public, same
                    // default as the battlefield/graveyard/exile.
                    visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
                    copy_generation: 0,
                    entered_via_escape: false,
                };
                command_zone.contents.push_back(obj_id);
                objects.insert(obj_id, obj);
            }
            zones.push(command_zone);
        }

        // Shared zones
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
            pending_timing_bypass: None,
            pending_free_cast: None,
        };

        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            departed_triggered_abilities: Vec::new(),
            next_command_id: 0,
            pending_payment: None,
            suspended_payments: Vec::new(),
            settings,
            pregame_setup_done: false,
            script_runtime: ScriptRuntime::new(),
        }
    }

    /// Accepts a `KeepHand` or `TakeMulligan` action from the player whose
    /// turn it is to decide during pregame. Only valid when
    /// `process_state == AwaitingMulliganDecision { player }` and
    /// `action.player == player`. On `TakeMulligan`, increments
    /// `mulligans_taken`, shuffles the hand back into the library, and draws a
    /// new opening hand immediately. Advances to the next deciding player or
    /// (if all have kept) to the bottoming phase.
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
                if let Some(max) = self.settings.max_mulligans {
                    if taken >= max {
                        return Err(MulliganError::MaxMulligansReached);
                    }
                }
                self.state.player_mut(player).expect("valid player").mulligans_taken += 1;
                self.return_hand_to_library(player);
                self.shuffle_library(player);
                self.draw_opening_hand(player);
                // Advance to the next undecided player after this one (circular).
                // Using next_after rather than next_mulligan_player because this
                // player is still !hand_kept (they mulliganed, not kept), so
                // next_mulligan_player would return them again immediately.
                let next = self.next_mulligan_player_after(player);
                self.process_state =
                    ProcessState::AwaitingMulliganDecision { player: next.expect("player must be in turn_order") };
            }
            _ => return Err(MulliganError::InvalidAction),
        }
        Ok(())
    }

    /// Accepts a `BottomCards` action from the player who must bottom cards
    /// after keeping with paid mulligans. Only valid when
    /// `process_state == AwaitingBottomCardChoice { player, .. }` and
    /// `action.player == player`. Validates count and card membership, moves
    /// the cards to the bottom of the player's library in submission order,
    /// then advances to the next bottoming player or to `AdvanceTurnStep`.
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
        // Duplicate check
        let mut seen: Vec<ObjectId> = Vec::with_capacity(cards.len());
        for &card in &cards {
            if seen.contains(&card) {
                return Err(BottomCardsError::DuplicateCard);
            }
            seen.push(card);
        }
        // Validate each card is in the player's hand
        for &card in &cards {
            let obj = self.state.object(card).ok_or(BottomCardsError::CardNotInHand)?;
            if obj.zone != ZoneKind::Hand || obj.owner != player {
                return Err(BottomCardsError::CardNotInHand);
            }
        }
        // Move each card hand → library bottom (CR 400.7 new ObjectId per move)
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
        // Advance the bottoming queue
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

    /// Called from `step()` when `phase == PreGame && process_state ==
    /// PriorityWindow`. Runs all automatic pregame actions: shuffles libraries
    /// (if `settings.shuffle_libraries`), draws opening hands for all players,
    /// then either transitions to `AwaitingMulliganDecision` for the first
    /// player in turn order (normal path) or directly to `AdvanceTurnStep`
    /// (if `settings.skip_mulligan`).
    ///
    /// `pub(super)`: `mod.rs`'s inline `PriorityWindow` arm calls this.
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

    /// Shuffles `player`'s library zone in-place using `state.rng`. Logs a
    /// `LibraryShuffled` event and an `RngDrawn` event per shuffle draw so
    /// the replay is fully deterministic. Uses Fisher-Yates over the zone's
    /// `VecDeque` contents.
    ///
    /// `pub(in crate::engine)`: `library_choices.rs`'s
    /// `continue_search_battlefield_entry` also calls this (CR 701.24a), and
    /// `mod.rs`'s still-inlined `ResolveStackTop` arm calls it for
    /// `AbilityEffect::Shuffle`. A later pass will also call it from
    /// `engine/resolve/library_effects.rs`/`engine/resolve/direct_effects.rs`
    /// once `ResolveStackTop` itself moves — making it accessible now avoids
    /// needing to touch this file again for that.
    pub(in crate::engine) fn shuffle_library(&mut self, player: PlayerId) {
        let len = self.state.zone(ZoneKind::Library, Some(player)).map_or(0, |z| z.contents.len());
        // Generate all swap targets first — can't hold &mut zone while calling next_rng()
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

    /// CR 702.85a Cascade's own "put all cards exiled this way that weren't
    /// cast on the bottom of your library in a random order" — `cards` are
    /// currently sitting in exile (`resolve_cascade_effect`'s own library
    /// walk); each is moved Exile -> Library (per-player, so
    /// `commit_zone_move`'s own owner derivation reads it off the card's
    /// own `last_known_info.owner`, no separate `player` parameter needed)
    /// in a freshly Fisher-Yates-shuffled order — same swap-and-log
    /// technique `shuffle_library` above already establishes, just applied
    /// to this small input list instead of the whole library, so the *rest*
    /// of the library's own order is left untouched. Same "no replacement-
    /// check, direct commit" posture every other library-zone mover in this
    /// engine already takes (`continue_search_battlefield_entry`) - library
    /// moves aren't replaceable anywhere in this codebase today.
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

    /// Returns all cards from `player`'s hand to the top of their library
    /// (no particular order required by CR 103.5 — placed top for consistency).
    /// Logs a ZoneChange per card. Used before a mulligan redraw.
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

    /// Moves `settings.starting_hand_size` cards from the top of `player`'s
    /// library to their hand zone, logging a `ZoneChange` per card.
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
            // CR 702.29b: an opening-hand card's own script self-registers
            // any hand-only ability now, same as every other Library->Hand
            // producer - remap_abilities just above is a no-op today (no
            // producer ever registers an ability against a still-in-library
            // object), so this is what actually populates it.
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

    /// Returns the first player in `state.turn_order` whose `hand_kept` is
    /// `false`. Returns `None` when all players have kept (mulligan phase
    /// complete).
    fn next_mulligan_player(&self) -> Option<PlayerId> {
        self.state.turn_order.iter().copied()
            .find(|&pid| !self.state.player(pid).map_or(false, |p| p.hand_kept))
    }

    /// Returns the first `!hand_kept` player found in circular turn order
    /// starting AFTER `current`. Used after `TakeMulligan` to advance to the
    /// next player who hasn't kept, wrapping around to the current player if
    /// they're the only undecided one remaining.
    fn next_mulligan_player_after(&self, current: PlayerId) -> Option<PlayerId> {
        let order = &self.state.turn_order;
        let n = order.len();
        let cur_pos = order.iter().position(|&p| p == current)?;
        for i in 1..=n {
            let pid = order[(cur_pos + i) % n];
            if !self.state.player(pid).map_or(false, |p| p.hand_kept) {
                return Some(pid);
            }
        }
        None // all players have kept (shouldn't be reachable after a TakeMulligan)
    }

    /// Advances `state.rng` by one step and returns the raw u64. Uses
    /// splitmix64 so the sequence is deterministic from `seed` alone and
    /// every output is logged as `RngDrawn`. `pub(in crate::engine)` (widened
    /// by `phase5/mass-choice-effects`) rather than private to this file —
    /// `engine::resolve::destruction`/`discard`'s own `SacrificeRandom`/
    /// `DiscardRandom` need the same deterministic, ledger-logged draws
    /// `shuffle_library` already uses, not a second RNG source.
    pub(in crate::engine) fn next_rng(&mut self) -> u64 {
        self.state.rng.draws_consumed += 1;
        let mut z = self.state.rng.seed.wrapping_add(
            0x9e3779b97f4a7c15u64.wrapping_mul(self.state.rng.draws_consumed),
        );
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Builds the bottoming queue: collects every player (in turn order) whose
    /// paid mulligan count (`mulligans_taken` minus `settings.free_mulligans`,
    /// clamped to 0) is > 0. If the queue is empty, transitions directly to
    /// `AdvanceTurnStep`; otherwise sets `process_state` to
    /// `AwaitingBottomCardChoice` for the first player in the queue, with
    /// `remaining` holding the rest.
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
