use std::collections::HashMap;

use stackfish_engine::*;

use crate::player_state;

/// A two-player `GameState` at the start of the active player's turn 1 main
/// phase (stack empty, active player holding priority), the shared skeleton
/// every direct-Rust integration test builds a `GameState` from. `zones` is
/// the one part every test genuinely differs on (which zone kinds it needs
/// present), so it's the only required parameter; every other field is at
/// its game-start default. Override anything else with struct update syntax,
/// e.g.: `GameState { cost_modifiers, ..testutil::two_player_state(zones) }`
///
/// `next_object_id` starts at 1000, not 0/1 — tests assign low literal
/// `ObjectId`s (0, 1, 99, ...) to their own fixture objects, so a distinct
/// starting value makes a real id-collision bug visible instead of
/// accidentally passing.
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
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

/// A one-player `GameState`, same game-start defaults as `two_player_state` -
/// for tests that only ever act from a single player's perspective (an
/// activated ability's cost/payment, say) and would otherwise pay for a
/// second `PlayerState`/turn-order entry they never touch.
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
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

/// The most common zone set: each player's hand plus a shared battlefield —
/// enough for any test moving a card hand-to-battlefield (land plays,
/// creature casts) with nowhere else in play.
pub fn hand_and_battlefield_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    vec![Zone::new(ZoneKind::Hand, Some(p0)), Zone::new(ZoneKind::Hand, Some(p1)), Zone::new(ZoneKind::Battlefield, None)]
}

/// `hand_and_battlefield_zones` plus each player's graveyard — for tests that
/// also need a card to resolve/die/discard somewhere (spell resolution,
/// cost-modifier tests that send a card to the graveyard, ...).
pub fn hand_battlefield_graveyard_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    let mut zones = hand_and_battlefield_zones(p0, p1);
    zones.push(Zone::new(ZoneKind::Graveyard, Some(p0)));
    zones.push(Zone::new(ZoneKind::Graveyard, Some(p1)));
    zones
}

/// Shared battlefield plus each player's graveyard, plus the single shared
/// exile zone (CR 400.10) — for tests whose creatures die or get exiled and
/// need somewhere real to land.
pub fn battlefield_graveyard_exile_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    vec![
        Zone::new(ZoneKind::Battlefield, None),
        Zone::new(ZoneKind::Graveyard, Some(p0)),
        Zone::new(ZoneKind::Graveyard, Some(p1)),
        Zone::new(ZoneKind::Exile, None),
    ]
}

/// One `Zone` per requested kind — a per-player pair for zone kinds that are
/// per-player (Hand, Library, Graveyard, Command), a single shared entry for
/// zone kinds that aren't (Battlefield, Exile, Stack). For zone-set shapes
/// the named presets above don't cover, rather than minting a new named
/// preset for every combination a test happens to need.
pub fn zones(p0: PlayerId, p1: PlayerId, kinds: &[ZoneKind]) -> Vec<Zone> {
    kinds
        .iter()
        .flat_map(|&kind| match kind {
            ZoneKind::Battlefield | ZoneKind::Exile | ZoneKind::Stack => vec![Zone::new(kind, None)],
            _ => vec![Zone::new(kind, Some(p0)), Zone::new(kind, Some(p1))],
        })
        .collect()
}
