//! CR 704.5a, 704.5b, 704.5c, 704.5f, 704.5g and CR 903.10a: a player at 0 life, who drew from an empty library, with 10 poison counters or dealt 21 commander damage loses, and a creature with 0 toughness or lethal damage dies.
// TODO: the other CR 704.5 SBAs (see sba.rs)

use std::collections::VecDeque;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, single_player_state};

// No fixture registers a ReplacementEffect on a dying creature, so a CR 616.1 pause here would mean the fixture is wrong.
fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
        SbaOutcome::NeedsLegendRuleChoice { .. } => {
            panic!("fixture didn't expect a legend-rule choice pause")
        }
        SbaOutcome::NeedsCommanderReturnChoice { .. } => {
            panic!("fixture didn't expect a commander-return choice pause")
        }
    }
}

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject { damage_marked, ..card_object(id, creature_definition(2, toughness), owner, ZoneKind::Battlefield) }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        ..single_player_state(p0, vec![Zone::new(ZoneKind::Battlefield, None), Zone::new(ZoneKind::Graveyard, Some(p0))])
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

// One solo-player PassPriority plus drain advances exactly one step transition.
fn pass(engine: &mut Engine, player: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority })
        .expect("solo player always holds priority when awaiting an action");
    drain(engine)
}

#[test]
fn lethal_marked_damage_destroys_creature() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(bear).is_none(), "old id should no longer resolve");
    assert!(!state.zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&bear));
    let gy = state.zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert_eq!(gy.contents.len(), 1);
    let new_id = gy.contents[0];
    assert_eq!(state.object(new_id).unwrap().zone, ZoneKind::Graveyard);

    assert_eq!(events.len(), 1);
    match &events[0].kind {
        GameEventKind::ZoneChange { object, from, to, last_known_info } => {
            assert_eq!(*object, new_id);
            assert_eq!(*from, ZoneKind::Battlefield);
            assert_eq!(*to, ZoneKind::Graveyard);
            assert_eq!(last_known_info.as_ref().unwrap().object_id, bear);
        }
        other => panic!("expected ZoneChange, got {other:?}"),
    }
}

#[test]
fn zero_toughness_creature_dies_without_any_damage_marked() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let weenie = ObjectId(0);
    put_on_battlefield(&mut state, creature(weenie, p0, 0, 0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(weenie).is_none());
    assert_eq!(events.len(), 1);
}

#[test]
fn sub_lethal_damage_creature_survives() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 3, 2));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(bear).is_some());
    assert!(events.is_empty());
}

#[test]
fn two_lethally_damaged_creatures_die_in_the_same_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    put_on_battlefield(&mut state, creature(a, p0, 2, 2));
    put_on_battlefield(&mut state, creature(b, p0, 1, 5));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(a).is_none());
    assert!(state.object(b).is_none());
    assert_eq!(events.len(), 2);
    assert_eq!(state.zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 2);
}

// Creatures that die together get new ids in ascending order of their old ids, since an unordered walk of the objects HashMap would fail this about 11 times in 12.
#[test]
fn creatures_that_die_together_leave_in_id_order() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    for id in 0..12 {
        put_on_battlefield(&mut state, creature(ObjectId(id), p0, 1, 5));
    }

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    let old_ids: Vec<ObjectId> = events
        .iter()
        .map(|e| match &e.kind {
            GameEventKind::ZoneChange { last_known_info, .. } => last_known_info.as_ref().unwrap().object_id,
            other => panic!("expected ZoneChange, got {other:?}"),
        })
        .collect();
    assert_eq!(old_ids, (0..12).map(ObjectId).collect::<Vec<_>>());
}

#[test]
fn creature_with_characteristic_defining_toughness_is_skipped() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let mut object = creature(star, p0, 0, 0);
    object.definition.toughness =
        Some(PTValue::CharacteristicDefining { display: "*".to_string(), script: None });
    put_on_battlefield(&mut state, object);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    // Can't resolve a CDA without the script bridge - must not guess and
    // must not panic.
    assert!(state.object(star).is_some());
    assert!(events.is_empty());
}

#[test]
fn player_at_zero_life_is_eliminated() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().life = 0;

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.player(p0).unwrap().eliminated);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::PlayerEliminated { player } if player == p0));
}

#[test]
fn player_above_zero_life_is_untouched() {
    let mut state = base_state();
    let p0 = PlayerId(0);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(!state.player(p0).unwrap().eliminated);
    assert!(events.is_empty());
}

#[test]
fn already_eliminated_player_does_not_re_emit_on_next_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().life = -5;

    let first = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(first.len(), 1);

    let second = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(second.is_empty(), "fixed-point loop must terminate, not re-eliminate every pass");
}

// CR 704.5b
#[test]
fn drawing_from_an_empty_library_sets_the_flag_but_does_not_eliminate_yet() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.phase = Phase::PreGame;
    state.zones.push(Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() });
    // A 1-player turn order reaches a real Draw-step draw on turn 1, since the CR 103.8a skip applies only to a 2-player game.

    let mut engine = Engine::new(state);
    // PreGame to Main takes 3 passes (Untap chains into Upkeep, then Draw).
    pass(&mut engine, p0);
    pass(&mut engine, p0);
    let status = pass(&mut engine, p0);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    // The draw only sets the flag, and a real SBA pass eliminates, as with the 704.5a life check.
    assert!(engine.state().player(p0).unwrap().drew_from_empty_library);
}

#[test]
fn drew_from_empty_library_flag_eliminates_on_the_next_sba_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().drew_from_empty_library = true;

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.player(p0).unwrap().eliminated);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::PlayerEliminated { player } if player == p0));
}

#[test]
fn drawing_a_real_card_does_not_set_the_flag() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.phase = Phase::PreGame;
    state.zones.push(Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() });
    let card = ObjectId(0);
    let mut library = Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() };
    library.push_bottom(card);
    state.zones.push(library);
    let mut card_object = creature(card, p0, 2, 0);
    card_object.zone = ZoneKind::Library;
    state.objects.insert(card, card_object);

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    pass(&mut engine, p0);
    let status = pass(&mut engine, p0);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert!(!engine.state().player(p0).unwrap().drew_from_empty_library);
    assert!(!engine.state().player(p0).unwrap().eliminated);
}

// CR 704.5c
#[test]
fn nine_poison_counters_is_untouched() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().counters.add(CounterKind::Poison, 9, SequenceNo(0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(!state.player(p0).unwrap().eliminated);
    assert!(events.is_empty());
}

#[test]
fn ten_poison_counters_eliminates() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().counters.add(CounterKind::Poison, 10, SequenceNo(0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.player(p0).unwrap().eliminated);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::PlayerEliminated { player } if player == p0));
}

#[test]
fn infect_combat_damage_accumulates_poison_and_eventually_eliminates_through_the_real_pipeline() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let infector = ObjectId(0);
    let mut state = base_state();
    state.turn_order = vec![p0, p1];
    state.players.push(player_state(p1));
    state.combat = Some(CombatState {
        attackers: vec![(infector, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    let mut object = creature(infector, p0, 3, 0);
    // 10 power in one unblocked AssignCombatDamage pass reaches the 10-poison threshold.
    object.definition.power = Some(PTValue::Fixed(10));
    object.definition.keywords = KeywordSet::INFECT;
    put_on_battlefield(&mut state, object);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::AssignCombatDamage { assignments: Vec::new() },
        })
        .expect("turn-based action, no priority required");
    let status = loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => break other,
        }
    };
    // CR 704.5c eliminates p1, which ends a two-player game (CR 104.2a).
    assert!(matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p0));

    assert_eq!(engine.state().player(p1).unwrap().life, 20, "702.90b: infect damage doesn't cause life loss");
    assert_eq!(engine.state().player(p1).unwrap().counters.count(&CounterKind::Poison), 10);
    assert!(engine.state().player(p1).unwrap().eliminated, "704.5c should fire through the real SbaLoop pipeline");
}

// CR 903.10a
#[test]
fn commander_damage_from_finds_the_matching_source_or_zero() {
    let mut player = player_state(PlayerId(0));
    let commander = "Test Commander";
    player.commander_damage_taken.push((commander.to_string(), 15));

    assert_eq!(player.commander_damage_from(commander), 15);
    assert_eq!(player.commander_damage_from("Some Other Commander"), 0);
}

#[test]
fn twenty_commander_damage_is_untouched() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let commander = "Test Commander";
    state.player_mut(p0).unwrap().commander_damage_taken.push((commander.to_string(), 20));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(!state.player(p0).unwrap().eliminated);
    assert!(events.is_empty());
}

#[test]
fn twenty_one_commander_damage_eliminates() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let commander = "Test Commander";
    state.player_mut(p0).unwrap().commander_damage_taken.push((commander.to_string(), 21));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.player(p0).unwrap().eliminated);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::PlayerEliminated { player } if player == p0));
}

#[test]
fn nothing_applies_returns_empty() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(events.is_empty());
    assert!(state.object(bear).is_some());
}

// SbaLoop is wired to this function inside Engine::step: a single PassPriority kills a lethally damaged creature.
#[test]
fn lethal_damage_dies_through_the_real_engine_pipeline() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.turn_order = vec![p0, PlayerId(1)];
    state.players.push(player_state(PlayerId(1)));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority })
        .expect("p0 holds priority");
    let status = loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => break other,
        }
    };
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert!(engine.state().object(bear).is_none());
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1);
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }))
    );
}
