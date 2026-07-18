//! Phase 5 vertical slice: the Beginning phase (CR 502-504 - Untap, Upkeep,
//! Draw). `Engine::untap_step`/`draw_step` (engine.rs) are exercised
//! end-to-end here via the `AdvanceTurnStep` pipeline, not called directly -
//! both are private and only ever invoked from that arm. Same struct-literal
//! `GameState` construction pattern as `phase5_pass_priority_loop.rs`/
//! `phase5_play_land.rs`, since the engine's own constructors are still
//! `todo!()`.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
        effect: None,
        modes: Vec::new(),
    }
}

fn land_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
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
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
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
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// Two players with hand/library/battlefield zones present, defaulting to
/// `Phase::PreGame`/`Step::Untap`/turn 1 (mirrors
/// `phase5_pass_priority_loop::two_player_state`) - tests that need to start
/// mid-Beginning-phase override `phase`/`step`/`turn_number` directly after
/// construction, same as `phase5_play_land.rs`'s rejection tests do.
fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::PreGame,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
        ],
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
        // Deliberately not 0/1 - those collide with the literal ObjectIds
        // fixtures below assign to test cards.
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
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

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
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
            GameEventKind::ZoneChange { object, from, to, .. } => Some((object, from, to)),
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

    let mut p0_obj = card_object(p0_creature, creature_definition("Bear"), p0, ZoneKind::Battlefield);
    p0_obj.tapped = true;
    state.objects.insert(p0_creature, p0_obj);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(p0_creature);

    let mut p1_obj = card_object(p1_creature, creature_definition("Wall"), p1, ZoneKind::Battlefield);
    p1_obj.tapped = true;
    state.objects.insert(p1_creature, p1_obj);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(p1_creature);

    // active_player defaults to p0 - CR 502.3 untaps only the active
    // player's permanents.
    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(!engine.state().object(p0_creature).unwrap().tapped, "active player's permanent untaps");
    assert!(engine.state().object(p1_creature).unwrap().tapped, "non-active player's permanent stays tapped");

    let untapped = untapped_event_ids(&engine);
    assert_eq!(untapped, vec![p0_creature], "exactly one Untapped event, for the active player's permanent");
}

#[test]
fn no_priority_during_untap() {
    let mut engine = Engine::new(two_player_state());
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let status = pass_both(&mut engine, p0, p1);

    // CR 502.4: the untap step opens no priority window - one full 2-player
    // pass-round from PreGame should land directly on Upkeep, never pausing
    // at Untap.
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
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);

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

    assert!(engine.state().object(card).is_none(), "old id retired per CR 400.7");
    assert_eq!(engine.state().object(new_id).unwrap().zone, ZoneKind::Hand);
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.contains(&new_id));
}

#[test]
fn draw_skipped_on_turn_1_two_player() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.phase = Phase::Beginning;
    state.step = Step::Upkeep;
    state.turn_number = 1; // CR 103.8a: starting player's first turn.
    // Fixture starts at Upkeep, bypassing PreGame — seed the skip manually,
    // since it's normally seeded by advance_turn_step's PreGame arm.
    state.pending_step_skips.push((p0, Step::Draw));

    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    // CR 500.7/500.8: a skipped step has no turn-based actions AND no priority
    // window — the engine skips past Draw entirely and opens priority at
    // PreCombatMain instead (old behavior opened priority at Draw with just the
    // action omitted; this is the correct per-rule behavior).
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Main);
    assert_eq!(engine.state().step, Step::PreCombatMain, "skipped Draw, no priority window there");

    assert!(zone_change_events(&engine).is_empty(), "no draw on the starting player's first turn");
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.contains(&card));
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty());
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

    // CR 104.3c/704.5b: no panic on an empty library - it's a real state-
    // based loss, not a crash. In a two-player game, the SBA pass that
    // catches it (before the Draw step's priority window would otherwise
    // open) immediately ends the game (CR 104.2a: sole opponent wins) -
    // backlog item 11b closing the "eliminated player keeps getting turns"
    // gap this test used to assert (see docs/implementation.md).
    assert!(
        matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p1),
        "p0 attempted to draw from an empty library - p1 is the last player standing"
    );
    assert!(zone_change_events(&engine).is_empty());
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty());
}
