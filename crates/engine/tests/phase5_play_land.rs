//! Phase 5 vertical slice: `PlayLand` (CR 305.1 - lands aren't spells, no
//! stack involved) plus the zone helpers (`Zone::remove`/`push_bottom`,
//! `GameState::object_mut`/`zone_mut`) it's the first real caller of. Same
//! struct-literal `GameState` construction pattern as
//! `phase5_pass_priority_loop.rs`, since the engine's own constructors are
//! still `todo!()`.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn land_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Forest".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("forest".to_string()),
    }
}

fn instant_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Shock".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("shock".to_string()),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// Two players, both zone sets present (hand/battlefield) so `PlayLand` has
/// somewhere real to move a card to/from. Mirrors
/// `phase5_pass_priority_loop::two_player_state` otherwise.
fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::PreGame,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        postcombat_main: false,
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

#[test]
fn play_land_moves_card_hand_to_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Battlefield);
    assert!(!engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.contains(&card));
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&card));
}

#[test]
fn play_land_breaks_pass_chain() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    state.objects.insert(land, card_object(land, land_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 1);

    engine
        .submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PlayLand { card: land } })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 0, "PlayLand must reset the pass chain");
}

#[test]
fn play_land_rejects_non_land_card() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.contains(&card));
    assert_eq!(engine.state().ledger.len(), 0, "no ledger growth on a rejected command");
}

#[test]
fn play_land_rejects_card_not_in_hand() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Battlefield, "state untouched");
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&card));
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn play_land_event_gets_causal_ids() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().ledger.len(), 1);
    let event = &engine.state().ledger[0];
    assert!(event.caused_by_command.is_some());
    assert!(event.parent_event.is_none());
    assert!(matches!(
        event.kind,
        GameEventKind::ZoneChange { object, from: ZoneKind::Hand, to: ZoneKind::Battlefield, last_known_info: Some(_) }
            if object == card
    ));
}
