use std::collections::HashMap;

use stackfish_engine::*;

use crate::player_state;

/// A two-player `GameState` at the start of turn 1 with an empty stack. Only `zones`
/// is a parameter; override anything else with `GameState { cost_modifiers, ..testutil::two_player_state(zones) }`.
///
/// `next_object_id` starts at 1000 so engine-assigned ids cannot collide with the low
/// literal ids tests give their own fixture objects.
pub fn two_player_state(zones: Vec<Zone>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
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
        rng: RngState {
            seed: 1,
            draws_consumed: 0,
        },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
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
    }
}

/// A one-player `GameState` with the same defaults as `two_player_state`, in the precombat main step.
pub fn single_player_state(player: PlayerId, zones: Vec<Zone>) -> GameState {
    GameState {
        players: vec![player_state(player)],
        turn_order: vec![player],
        active_player: player,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: player,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(player, 1)]),
        objects: HashMap::new(),
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
        rng: RngState {
            seed: 1,
            draws_consumed: 0,
        },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
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
    }
}

/// Each player's hand plus a shared battlefield.
pub fn hand_and_battlefield_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    vec![
        Zone::new(ZoneKind::Hand, Some(p0)),
        Zone::new(ZoneKind::Hand, Some(p1)),
        Zone::new(ZoneKind::Battlefield, None),
    ]
}

/// `hand_and_battlefield_zones` plus each player's graveyard.
pub fn hand_battlefield_graveyard_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    let mut zones = hand_and_battlefield_zones(p0, p1);
    zones.push(Zone::new(ZoneKind::Graveyard, Some(p0)));
    zones.push(Zone::new(ZoneKind::Graveyard, Some(p1)));
    zones
}

/// Shared battlefield, each player's graveyard and one shared exile zone. CR 400.10
pub fn battlefield_graveyard_exile_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    vec![
        Zone::new(ZoneKind::Battlefield, None),
        Zone::new(ZoneKind::Graveyard, Some(p0)),
        Zone::new(ZoneKind::Graveyard, Some(p1)),
        Zone::new(ZoneKind::Exile, None),
    ]
}

/// One `Zone` per requested kind: a pair for per-player kinds, a single entry for
/// Battlefield, Exile and Stack. Use it for sets the presets above do not cover.
pub fn zones(p0: PlayerId, p1: PlayerId, kinds: &[ZoneKind]) -> Vec<Zone> {
    kinds
        .iter()
        .flat_map(|&kind| match kind {
            ZoneKind::Battlefield | ZoneKind::Exile | ZoneKind::Stack => {
                vec![Zone::new(kind, None)]
            }
            _ => vec![Zone::new(kind, Some(p0)), Zone::new(kind, Some(p1))],
        })
        .collect()
}
