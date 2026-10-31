//! CR 502-504: Beginning phase steps (untap, upkeep, draw), driven through `AdvanceTurnStep` because `untap_step` and `draw_step` are private.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield],
        ))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

fn untapped_event_ids(engine: &Engine) -> Vec<ObjectId> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::Untapped { object } => Some(object),
            _ => None,
        })
        .collect()
}

fn zone_change_events(engine: &Engine) -> Vec<(ObjectId, ZoneKind, ZoneKind)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::ZoneChange {
                object, from, to, ..
            } => Some((object, from, to)),
            _ => None,
        })
        .collect()
}

#[test]
fn untap_only_untaps_active_players_tapped_permanents() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let p0_creature = ObjectId(0);
    let p1_creature = ObjectId(1);

    let mut p0_obj = card_object(
        p0_creature,
        creature_definition("Bear"),
        p0,
        ZoneKind::Battlefield,
    );
    p0_obj.tapped = true;
    state.objects.insert(p0_creature, p0_obj);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(p0_creature);

    let mut p1_obj = card_object(
        p1_creature,
        creature_definition("Wall"),
        p1,
        ZoneKind::Battlefield,
    );
    p1_obj.tapped = true;
    state.objects.insert(p1_creature, p1_obj);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(p1_creature);

    // active_player defaults to p0, and CR 502.3 untaps only that player's permanents.
    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        !engine.state().object(p0_creature).unwrap().tapped,
        "active player's permanent untaps"
    );
    assert!(
        engine.state().object(p1_creature).unwrap().tapped,
        "non-active player's permanent stays tapped"
    );

    let untapped = untapped_event_ids(&engine);
    assert_eq!(
        untapped,
        vec![p0_creature],
        "exactly one Untapped event, for the active player's permanent"
    );
}

#[test]
fn no_priority_during_untap() {
    let mut engine = Engine::new(two_player_state());
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let status = pass_both(&mut engine, p0, p1);

    // CR 502.4: the untap step opens no priority window, so one pass-round from PreGame lands on Upkeep.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
}

#[test]
fn draw_on_turn_2_or_later() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.phase = Phase::Beginning;
    state.step = Step::Upkeep;
    state.turn_number = 2;

    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, land_definition("Forest"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Draw);

    let changes = zone_change_events(&engine);
    assert_eq!(changes.len(), 1, "exactly one ZoneChange for the draw");
    let (new_id, from, to) = changes[0];
    assert_eq!(from, ZoneKind::Library);
    assert_eq!(to, ZoneKind::Hand);
    assert_ne!(new_id, card, "CR 400.7: drawing mints a new ObjectId");

    assert!(
        engine.state().object(card).is_none(),
        "old id retired per CR 400.7"
    );
    assert_eq!(engine.state().object(new_id).unwrap().zone, ZoneKind::Hand);
    assert!(
        engine
            .state()
            .zone(ZoneKind::Library, Some(p0))
            .unwrap()
            .contents
            .is_empty()
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .contains(&new_id)
    );
}

#[test]
fn draw_skipped_on_turn_1_two_player() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.phase = Phase::Beginning;
    state.step = Step::Upkeep;
    state.turn_number = 1; // CR 103.8a: starting player's first turn.
    // The fixture starts at Upkeep, so the PreGame arm's skip is seeded by hand.
    state.pending_step_skips.push((p0, Step::Draw));

    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, land_definition("Forest"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    // CR 500.7, 500.8: a skipped step has no turn-based actions and no priority window, so priority opens at PreCombatMain.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Main);
    assert_eq!(
        engine.state().step,
        Step::PreCombatMain,
        "skipped Draw, no priority window there"
    );

    assert!(
        zone_change_events(&engine).is_empty(),
        "no draw on the starting player's first turn"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Library, Some(p0))
            .unwrap()
            .contents
            .contains(&card)
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .is_empty()
    );
}

#[test]
fn draw_from_empty_library_is_a_noop_draw_not_a_panic() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.phase = Phase::Beginning;
    state.step = Step::Upkeep;
    state.turn_number = 2; // not the skip case - library is just empty.

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);

    // CR 104.3c, 704.5b: an empty library is a state-based loss and not a panic. With two players the SBA pass ends the game before Draw opens priority (CR 104.2a).
    assert!(
        matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p1),
        "p0 attempted to draw from an empty library - p1 is the last player standing"
    );
    assert!(zone_change_events(&engine).is_empty());
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .is_empty()
    );
}
