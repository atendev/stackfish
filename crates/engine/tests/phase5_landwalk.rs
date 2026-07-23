//! Phase 5 vertical slice: CR 702.14 Landwalk, named out of scope by
//! `phase5_combat_evasion_keywords.rs` - unlike Fear/Intimidate/Shadow/
//! Horsemanship (a plain `KeywordSet` bit compared attacker-vs-blocker),
//! landwalk is parametrized by land subtype and checks the *defending
//! player's own battlefield*, not the blocker's characteristics.
//!
//! Same shape as protection (`EffectiveCharacteristics::landwalk_subtypes`,
//! a new layer 6 `EffectPrimitive::AddLandwalkSubtype`): no printed field on
//! `CardDefinition`, always effect-granted - a card with printed Islandwalk
//! self-registers an always-on static ability via its script, same as
//! printed protection does. Enforcement lives in `commands::DeclareBlockers`
//! alongside the other evasion keyword checks.
//!
//! Deliberately narrow, matching CR 702.14a's most common real form: only
//! single-subtype matching is modeled (covers all five basic landwalks).
//! The card-type/supertype-combination forms ("artifact landwalk,"
//! "nonbasic landwalk," "snow swampwalk") are TODO, unmodeled.

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state};

fn creature_definition(id: u64, name: &str, extra_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE | extra_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn land_definition(id: u64, name: &str, subtype: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: vec![Subtype(subtype.to_string())],
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Directly-constructed landwalk grant, standing in for the ETB-registered
/// version - the Lua wiring itself (`Game.Effects.AddLandwalkSubtype`) is
/// proven separately, same convention every other static-ability-adjacent
/// test file in this repo uses (see `phase5_protection_from_color.rs`'s
/// `protection_from_red`).
fn islandwalk(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddLandwalkSubtype(Subtype("Island".to_string()))),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

/// Same game-start defaults `stackfish_testutil::two_player_state` builds,
/// but for a variable player count (this file's multiplayer test needs 3) -
/// every field below is overridden, `two_player_state(Vec::new())` supplies
/// only the shared boilerplate (rng, empty collections, ...).
fn combat_state(players: Vec<PlayerId>) -> GameState {
    let last_active_turn_number = players.iter().enumerate().map(|(i, p)| (*p, if i == 0 { 1 } else { 0 })).collect();
    GameState {
        players: players.iter().map(|p| player_state(*p)).collect(),
        turn_order: players.clone(),
        active_player: players[0],
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        priority_player: players[0],
        last_active_turn_number,
        zones: vec![Zone::new(ZoneKind::Battlefield, None)],
        ..stackfish_testutil::two_player_state(Vec::new())
    }
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached DeclareBlockers - state.step = {:?}", engine.state().step);
}

fn assert_legal_block(status: StepStatus) {
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected a legal block, got {status:?}");
}

fn assert_illegal_block(status: StepStatus) {
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "expected an illegal block, got {status:?}");
}

// ---------------------------------------------------------------------
// Islandwalk (CR 702.14) - two players
// ---------------------------------------------------------------------

#[test]
fn islandwalk_attacker_unblockable_when_defender_controls_an_island() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let island = ObjectId(2);

    let mut state = combat_state(vec![p0, p1]);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Kelpling", CardTypeSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(2, "Bear", CardTypeSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(island, land_definition(3, "Island", "Island"), p1));
    state.continuous_effects.push(islandwalk(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    assert_illegal_block(declare_blockers(&mut engine, p1, vec![(blocker, attacker)]));
}

#[test]
fn islandwalk_attacker_blockable_when_defender_controls_no_island() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let forest = ObjectId(2);

    let mut state = combat_state(vec![p0, p1]);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Kelpling", CardTypeSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(2, "Bear", CardTypeSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(forest, land_definition(3, "Forest", "Forest"), p1));
    state.continuous_effects.push(islandwalk(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    assert_legal_block(declare_blockers(&mut engine, p1, vec![(blocker, attacker)]));
}

#[test]
fn non_landwalk_attacker_blockable_even_when_defender_controls_an_island() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let island = ObjectId(2);

    let mut state = combat_state(vec![p0, p1]);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Bear", CardTypeSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(2, "Bear2", CardTypeSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(island, land_definition(3, "Island", "Island"), p1));
    // No islandwalk grant on the attacker this time.

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    assert_legal_block(declare_blockers(&mut engine, p1, vec![(blocker, attacker)]));
}

// ---------------------------------------------------------------------
// Islandwalk - multiplayer, checked against the actual defending player
// ---------------------------------------------------------------------

#[test]
fn islandwalk_checks_only_the_defending_players_own_battlefield() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let p2 = PlayerId(2);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let bystander_island = ObjectId(2);

    let mut state = combat_state(vec![p0, p1, p2]);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Kelpling", CardTypeSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(2, "Bear", CardTypeSet::empty()), p1));
    // p2 controls an Island but isn't the defending player this attack -
    // p1 (the real defending player) controls no land at all.
    put_on_battlefield(&mut state, permanent(bystander_island, land_definition(3, "Island", "Island"), p2));
    state.continuous_effects.push(islandwalk(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    assert_legal_block(declare_blockers(&mut engine, p1, vec![(blocker, attacker)]));
}
