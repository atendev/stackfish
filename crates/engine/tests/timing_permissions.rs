//! CR 601.3b: "cast spells as though they had flash" permissions in `GameState.timing_permissions`.
//! The cast command and `Engine::legal_actions` read one predicate, so both agree here. The Lua builder round trip is in script_timing_permission.rs.

use std::collections::HashMap;

use stackfish_engine::decision::Entry;
use stackfish_engine::engine::{ActionError, Engine};
use stackfish_engine::*;
use stackfish_testutil::{
    assert_action_rejected, card_object, drain, minimal_card_definition, pass_both_and_drain,
    pay_free_and_drain, try_cast_spell, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const ORRERY: ObjectId = ObjectId(10);
const SORCERY: ObjectId = ObjectId(20);
const NO_ABILITIES: &str = "local s = {}\nreturn s";
const SORCERY_SPEED: &str = "this spell can only be cast at sorcery speed";

fn definition(id: u64, name: &str, types: CardTypeSet, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            id,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn put(state: &mut GameState, object: GameObject) {
    let owner = (object.zone != ZoneKind::Battlefield).then_some(object.owner);
    let (zone, id) = (object.zone, object.object_id);
    state.zone_mut(zone, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

// Player 1 holds priority in player 0's main phase with an empty stack, with a sorcery in hand.
fn off_turn_state() -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        priority_player: P1,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    put(
        &mut state,
        card_object(
            SORCERY,
            definition(2, "Test Sorcery", CardTypeSet::SORCERY, NO_ABILITIES),
            P1,
            ZoneKind::Hand,
        ),
    );
    state
}

fn with_orrery(
    mut state: GameState,
    controller: PlayerId,
    affected: FilterController,
) -> GameState {
    put(
        &mut state,
        card_object(
            ORRERY,
            definition(1, "Test Orrery", CardTypeSet::ARTIFACT, NO_ABILITIES),
            controller,
            ZoneKind::Battlefield,
        ),
    );
    state.timing_permissions.push(TimingPermission {
        source: ORRERY,
        controller,
        affected,
        spells: FilterSpec::ANY,
        duration: EffectDuration::WhileSourceOnBattlefield,
    });
    state
}

fn cast_action(card: ObjectId, alternative_cost: Option<usize>) -> PlayerAction {
    PlayerAction {
        player: P1,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets: Vec::new(),
            x_value: None,
            alternative_cost,
            pay_buyback: false,
            division: Vec::new(),
        },
    }
}

fn lists_cast(engine: &Engine, card: ObjectId) -> bool {
    engine
        .legal_actions()
        .iter()
        .any(|entry| matches!(entry, Entry::Cast(cast) if cast.card == card))
}

// CR 117.1a
#[test]
fn a_sorcery_off_turn_is_refused_and_not_listed_without_a_permission() {
    let mut engine = Engine::new(off_turn_state());
    assert!(!lists_cast(&engine, SORCERY));
    let err = assert_action_rejected(&mut engine, "no permission", cast_action(SORCERY, None));
    assert!(format!("{err:?}").contains(SORCERY_SPEED), "{err:?}");
}

// CR 601.3b
#[test]
fn a_permission_lets_a_sorcery_be_cast_off_turn_and_lists_it() {
    let mut engine = Engine::new(with_orrery(off_turn_state(), P1, FilterController::You));
    assert!(lists_cast(&engine, SORCERY));
    try_cast_spell(&mut engine, P1, SORCERY, Vec::new())
        .expect("the permission covers the sorcery");
}

// CR 601.3b
#[test]
fn an_opponents_permission_affecting_only_its_controller_does_not_help() {
    let mut engine = Engine::new(with_orrery(off_turn_state(), P0, FilterController::You));
    assert!(!lists_cast(&engine, SORCERY));
    assert_action_rejected(
        &mut engine,
        "P0's own permission",
        cast_action(SORCERY, None),
    );
}

// CR 601.3b
#[test]
fn a_permission_affecting_opponents_helps_the_opponent() {
    let mut engine = Engine::new(with_orrery(
        off_turn_state(),
        P0,
        FilterController::Opponent,
    ));
    assert!(lists_cast(&engine, SORCERY));
    try_cast_spell(&mut engine, P1, SORCERY, Vec::new()).expect("P0's permission affects P1");
}

// CR 117.1a
// The engine refuses an actor without priority before the cast rules run.
#[test]
fn a_permission_does_not_grant_priority() {
    let mut state = with_orrery(off_turn_state(), P1, FilterController::You);
    state.priority_player = P0;
    let mut engine = Engine::new(state);
    let err = assert_action_rejected(&mut engine, "no priority", cast_action(SORCERY, None));
    assert!(matches!(err, ActionError::NotYourTurn), "{err:?}");
}

// CR 601.3
// The madness waiver skips timing on its own, with no permission in play.
#[test]
fn the_timing_bypass_still_lets_a_sorcery_through_off_turn() {
    let mut state = off_turn_state();
    state.pending_timing_bypass = Some(SORCERY);
    let mut engine = Engine::new(state);
    try_cast_spell(&mut engine, P1, SORCERY, Vec::new()).expect("the bypass waives timing");
}

// CR 601.3b
#[test]
fn a_spell_filter_limits_the_permission() {
    let mut state = with_orrery(off_turn_state(), P1, FilterController::You);
    state.timing_permissions[0].spells = FilterSpec {
        card_types: Some(CardTypeSet::CREATURE),
        ..FilterSpec::ANY
    };
    let creature = ObjectId(21);
    put(
        &mut state,
        card_object(
            creature,
            definition(3, "Test Creature", CardTypeSet::CREATURE, NO_ABILITIES),
            P1,
            ZoneKind::Hand,
        ),
    );
    let mut engine = Engine::new(state);
    assert!(lists_cast(&engine, creature));
    assert!(!lists_cast(&engine, SORCERY));
    assert_action_rejected(&mut engine, "filtered out", cast_action(SORCERY, None));
}

// CR 601.3b
#[test]
fn a_permission_lets_a_flashback_sorcery_be_cast_off_turn() {
    let mut state = with_orrery(off_turn_state(), P1, FilterController::You);
    let flashback = ObjectId(21);
    let mut card = definition(3, "Test Flashback", CardTypeSet::SORCERY, NO_ABILITIES);
    card.alternative_costs = vec![AlternativeCost::Replacement {
        description: "Flashback {0}".to_string(),
        cost: CostSpec::mana_only(ManaCost {
            symbols: Vec::new(),
        }),
        from_zone: ZoneKind::Graveyard,
        exile_on_leaving_stack: true,
        escape: false,
    }];
    put(
        &mut state,
        card_object(flashback, card, P1, ZoneKind::Graveyard),
    );
    let mut engine = Engine::new(state);
    assert!(lists_cast(&engine, flashback));
    engine
        .submit_action(cast_action(flashback, Some(0)))
        .expect("the permission covers a cast from the graveyard");
}

// CR 305.1
#[test]
fn a_permission_does_not_let_a_land_be_played_off_turn() {
    let mut state = with_orrery(off_turn_state(), P1, FilterController::You);
    let land = ObjectId(21);
    put(
        &mut state,
        card_object(
            land,
            definition(3, "Test Land", CardTypeSet::LAND, NO_ABILITIES),
            P1,
            ZoneKind::Hand,
        ),
    );
    let mut engine = Engine::new(state);
    assert!(
        !engine
            .legal_actions()
            .iter()
            .any(|entry| matches!(entry, Entry::PlayLand { card } if *card == land))
    );
    assert_action_rejected(
        &mut engine,
        "land off turn",
        PlayerAction {
            player: P1,
            kind: PlayerActionKind::PlayLand { card: land },
        },
    );
}

// CR 611.3b
// The source is cast from hand, which moves it without `retire_object`.
#[test]
fn a_hand_sourced_permission_is_dropped_when_its_source_is_cast() {
    let mut state = off_turn_state();
    let source = ObjectId(21);
    put(
        &mut state,
        card_object(
            source,
            definition(3, "Test Hand Source", CardTypeSet::SORCERY, NO_ABILITIES),
            P1,
            ZoneKind::Hand,
        ),
    );
    state.timing_permissions.push(TimingPermission {
        source,
        controller: P1,
        affected: FilterController::You,
        spells: FilterSpec::ANY,
        duration: EffectDuration::WhileSourceOnBattlefield,
    });
    let mut engine = Engine::new(state);
    try_cast_spell(&mut engine, P1, source, Vec::new()).expect("the source covers itself");
    drain(&mut engine);
    assert!(engine.state().timing_permissions.is_empty());
    assert!(!lists_cast(&engine, SORCERY));
}

// CR 611.3b
#[test]
fn a_bounced_source_takes_its_permission_with_it() {
    let mut state = with_orrery(off_turn_state(), P1, FilterController::You);
    let bounce = ObjectId(21);
    let mut bounce_card = definition(3, "Test Bounce", CardTypeSet::INSTANT, NO_ABILITIES);
    bounce_card.effect = Some(AbilityEffect::ReturnToHand);
    bounce_card.target_requirement = Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            card_types: Some(CardTypeSet::ARTIFACT),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }));
    put(
        &mut state,
        card_object(bounce, bounce_card, P1, ZoneKind::Hand),
    );
    let mut engine = Engine::new(state);
    try_cast_spell(&mut engine, P1, bounce, vec![Target::Object(ORRERY)]).expect("instant");
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P1, P0);

    assert_eq!(
        engine.state().object(ORRERY),
        None,
        "the Orrery left the battlefield"
    );
    assert!(engine.state().timing_permissions.is_empty());
    assert!(!lists_cast(&engine, SORCERY));
}
