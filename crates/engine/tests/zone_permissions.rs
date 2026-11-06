//! CR 601.3, 305.1: "play or cast from another zone" permissions in `GameState.zone_permissions`.
//! The cast and land commands and `Engine::legal_actions` read one predicate each, so both agree here. The Lua builder round trip is in script_zone_permission.rs.

use std::collections::HashMap;

use stackfish_engine::decision::Entry;
use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::{
    assert_action_rejected, card_object, drain, last_zone_change, minimal_card_definition,
    pass_both_and_drain, pay_free_and_drain, try_cast_spell, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CRUCIBLE: ObjectId = ObjectId(10);
const LAND: ObjectId = ObjectId(20);
const NO_ABILITIES: &str = "local s = {}\nreturn s";

fn definition(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(NO_ABILITIES.to_string()),
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
    let owner =
        (!matches!(object.zone, ZoneKind::Battlefield | ZoneKind::Exile)).then_some(object.owner);
    let (zone, id) = (object.zone, object.object_id);
    state.zone_mut(zone, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

// Player 0 holds priority in their own main phase with an empty stack.
fn main_phase_state() -> GameState {
    GameState {
        step: Step::PreCombatMain,
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
    }
}

fn with_permission(
    mut state: GameState,
    kind: ZonePermissionKind,
    zone: ZoneKind,
    owners: FilterController,
) -> GameState {
    put(
        &mut state,
        card_object(
            CRUCIBLE,
            definition(1, "Test Crucible", CardTypeSet::ARTIFACT),
            P0,
            ZoneKind::Battlefield,
        ),
    );
    state.zone_permissions.push(ZonePermission {
        source: CRUCIBLE,
        controller: P0,
        affected: FilterController::You,
        zone,
        owners,
        kind,
        cards: FilterSpec::ANY,
        library_top: false,
        linked: false,
        reveals_top: false,
        duration: EffectDuration::WhileSourceOnBattlefield,
    });
    state
}

fn with_land(mut state: GameState, owner: PlayerId, zone: ZoneKind) -> GameState {
    put(
        &mut state,
        card_object(
            LAND,
            definition(2, "Test Land", CardTypeSet::LAND),
            owner,
            zone,
        ),
    );
    state
}

fn with_card(
    mut state: GameState,
    id: ObjectId,
    card: CardDefinition,
    owner: PlayerId,
    zone: ZoneKind,
) -> GameState {
    put(&mut state, card_object(id, card, owner, zone));
    state
}

fn play_land(player: PlayerId, card: ObjectId) -> PlayerAction {
    PlayerAction {
        player,
        kind: PlayerActionKind::PlayLand { card },
    }
}

fn cast_action(player: PlayerId, card: ObjectId, alternative_cost: Option<usize>) -> PlayerAction {
    PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets: Vec::new(),
            x_value: None,
            alternative_cost,
            pay_buyback: false,
        },
    }
}

fn lists_land(engine: &Engine, land: ObjectId) -> bool {
    engine
        .legal_actions()
        .iter()
        .any(|entry| matches!(entry, Entry::PlayLand { card } if *card == land))
}

fn cast_entry(engine: &Engine, card: ObjectId) -> Option<(bool, Vec<usize>)> {
    engine
        .legal_actions()
        .into_iter()
        .find_map(|entry| match entry {
            Entry::Cast(cast) if cast.card == card => Some((cast.normal, cast.alternative_costs)),
            _ => None,
        })
}

fn graveyard_land_permission() -> GameState {
    with_permission(
        main_phase_state(),
        ZonePermissionKind::PlayLand,
        ZoneKind::Graveyard,
        FilterController::You,
    )
}

// CR 305.1
#[test]
fn a_graveyard_land_is_refused_and_not_listed_without_a_permission() {
    let mut engine = Engine::new(with_land(main_phase_state(), P0, ZoneKind::Graveyard));
    assert!(!lists_land(&engine, LAND));
    assert_action_rejected(&mut engine, "no permission", play_land(P0, LAND));
}

// CR 305.1
#[test]
fn a_permission_lets_a_graveyard_land_be_played_and_lists_it() {
    let mut engine = Engine::new(with_land(
        graveyard_land_permission(),
        P0,
        ZoneKind::Graveyard,
    ));
    assert!(lists_land(&engine, LAND));
    engine
        .submit_action(play_land(P0, LAND))
        .expect("the permission opens the graveyard");
    drain(&mut engine);
    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Graveyard, ZoneKind::Battlefield));
    let state = engine.state();
    assert!(
        state
            .zone(ZoneKind::Graveyard, Some(P0))
            .unwrap()
            .contents
            .is_empty()
    );
    assert_eq!(state.object(new_id).unwrap().controller, P0);
    assert_eq!(state.lands_played_this_turn.get(&P0), Some(&1));
}

// CR 305.2a
#[test]
fn a_graveyard_land_play_counts_toward_the_land_limit() {
    let mut state = with_land(graveyard_land_permission(), P0, ZoneKind::Graveyard);
    state.lands_played_this_turn.insert(P0, 1);
    let mut engine = Engine::new(state);
    assert!(!lists_land(&engine, LAND));
    assert_action_rejected(&mut engine, "land limit", play_land(P0, LAND));
}

// CR 305.3
#[test]
fn a_permission_does_not_let_a_graveyard_land_be_played_on_an_opponents_turn() {
    let mut state = with_land(graveyard_land_permission(), P0, ZoneKind::Graveyard);
    state.active_player = P1;
    let mut engine = Engine::new(state);
    assert!(!lists_land(&engine, LAND));
    assert_action_rejected(&mut engine, "off turn", play_land(P0, LAND));
}

// CR 305.1
#[test]
fn a_permission_for_your_own_graveyard_does_not_open_an_opponents() {
    let mut engine = Engine::new(with_land(
        graveyard_land_permission(),
        P1,
        ZoneKind::Graveyard,
    ));
    assert!(!lists_land(&engine, LAND));
    assert_action_rejected(&mut engine, "opponent's graveyard", play_land(P0, LAND));
}

// CR 305.1, 110.2
#[test]
fn a_permission_over_opponents_graveyards_plays_their_land_under_your_control() {
    let state = with_permission(
        main_phase_state(),
        ZonePermissionKind::PlayLand,
        ZoneKind::Graveyard,
        FilterController::Opponent,
    );
    let mut engine = Engine::new(with_land(state, P1, ZoneKind::Graveyard));
    assert!(lists_land(&engine, LAND));
    engine
        .submit_action(play_land(P0, LAND))
        .expect("the permission covers P1's graveyard");
    drain(&mut engine);
    let (new_id, from, _) = last_zone_change(&engine);
    assert_eq!(from, ZoneKind::Graveyard);
    let state = engine.state();
    assert!(
        state
            .zone(ZoneKind::Graveyard, Some(P1))
            .unwrap()
            .contents
            .is_empty()
    );
    let land = state.object(new_id).unwrap();
    assert_eq!((land.owner, land.controller), (P1, P0));
}

// CR 601.3
#[test]
fn a_play_land_permission_does_not_let_a_spell_be_cast() {
    let sorcery = ObjectId(21);
    let state = with_card(
        graveyard_land_permission(),
        sorcery,
        definition(3, "Test Sorcery", CardTypeSet::SORCERY),
        P0,
        ZoneKind::Graveyard,
    );
    let mut engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, sorcery), None);
    assert_action_rejected(&mut engine, "wrong kind", cast_action(P0, sorcery, None));
}

// CR 601.3, 117.1a
// Player 0 holds priority on player 1's turn, so only instant speed is open.
#[test]
fn a_cast_permission_keeps_the_cards_own_timing() {
    let (instant, sorcery) = (ObjectId(21), ObjectId(22));
    let mut state = with_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        ZoneKind::Graveyard,
        FilterController::You,
    );
    state = with_card(
        state,
        instant,
        definition(3, "Test Instant", CardTypeSet::INSTANT),
        P0,
        ZoneKind::Graveyard,
    );
    state = with_card(
        state,
        sorcery,
        definition(4, "Test Sorcery", CardTypeSet::SORCERY),
        P0,
        ZoneKind::Graveyard,
    );
    state.active_player = P1;
    let mut engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, sorcery), None);
    assert_action_rejected(
        &mut engine,
        "sorcery off turn",
        cast_action(P0, sorcery, None),
    );
    assert_eq!(cast_entry(&engine, instant), Some((true, Vec::new())));
    try_cast_spell(&mut engine, P0, instant, Vec::new()).expect("an instant from the graveyard");
    drain(&mut engine);
    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Graveyard, ZoneKind::Stack));
}

// CR 601.3, 702.34a
// Both ways to cast stay open, and the chosen cost decides which one is checked.
#[test]
fn a_flashback_card_under_a_cast_permission_lists_both_casts() {
    let flashback = ObjectId(21);
    let mut card = definition(3, "Test Flashback", CardTypeSet::SORCERY);
    card.alternative_costs = vec![AlternativeCost::Replacement {
        description: "Flashback {0}".to_string(),
        cost: CostSpec::mana_only(ManaCost {
            symbols: Vec::new(),
        }),
        from_zone: ZoneKind::Graveyard,
        exile_on_leaving_stack: true,
        escape: false,
    }];
    let state = with_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        ZoneKind::Graveyard,
        FilterController::You,
    );
    let mut engine = Engine::new(with_card(state, flashback, card, P0, ZoneKind::Graveyard));
    assert_eq!(cast_entry(&engine, flashback), Some((true, vec![0])));
    engine
        .submit_action(cast_action(P0, flashback, None))
        .expect("the permission allows the normal cost");
}

// CR 601.3
#[test]
fn an_exile_permission_casts_from_the_shared_exile_zone() {
    let sorcery = ObjectId(21);
    let state = with_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        ZoneKind::Exile,
        FilterController::You,
    );
    let state = with_card(
        state,
        sorcery,
        definition(3, "Test Sorcery", CardTypeSet::SORCERY),
        P0,
        ZoneKind::Exile,
    );
    let mut engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, sorcery), Some((true, Vec::new())));
    try_cast_spell(&mut engine, P0, sorcery, Vec::new()).expect("a sorcery from exile");
    drain(&mut engine);
    let (_, from, _) = last_zone_change(&engine);
    assert_eq!(from, ZoneKind::Exile);
    assert!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty()
    );
}

// CR 601.3
#[test]
fn a_graveyard_permission_does_not_open_exile() {
    let sorcery = ObjectId(21);
    let state = with_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        ZoneKind::Graveyard,
        FilterController::You,
    );
    let state = with_card(
        state,
        sorcery,
        definition(3, "Test Sorcery", CardTypeSet::SORCERY),
        P0,
        ZoneKind::Exile,
    );
    let mut engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, sorcery), None);
    assert_action_rejected(&mut engine, "wrong zone", cast_action(P0, sorcery, None));
}

// CR 611.3b
#[test]
fn a_bounced_source_takes_its_zone_permission_with_it() {
    let bounce = ObjectId(21);
    let mut bounce_card = definition(3, "Test Bounce", CardTypeSet::INSTANT);
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
    let state = with_land(graveyard_land_permission(), P0, ZoneKind::Graveyard);
    let mut engine = Engine::new(with_card(state, bounce, bounce_card, P0, ZoneKind::Hand));
    try_cast_spell(&mut engine, P0, bounce, vec![Target::Object(CRUCIBLE)]).expect("instant");
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    drain(&mut engine);

    assert_eq!(engine.state().object(CRUCIBLE), None);
    assert!(engine.state().zone_permissions.is_empty());
    assert!(!lists_land(&engine, LAND));
}

fn with_top_permission(
    mut state: GameState,
    kind: ZonePermissionKind,
    affected: FilterController,
    reveals_top: bool,
) -> GameState {
    state = with_permission(state, kind, ZoneKind::Library, FilterController::You);
    let permission = &mut state.zone_permissions[0];
    permission.affected = affected;
    permission.library_top = true;
    permission.reveals_top = reveals_top;
    state
}

fn with_linked_permission(state: GameState) -> GameState {
    let mut state = with_permission(
        state,
        ZonePermissionKind::Cast,
        ZoneKind::Exile,
        FilterController::Any,
    );
    state.zone_permissions[0].linked = true;
    state
}

// CR 401.5, 305.2a
#[test]
fn a_library_top_permission_plays_the_top_land_and_counts_the_land_play() {
    let state = with_top_permission(
        main_phase_state(),
        ZonePermissionKind::PlayLand,
        FilterController::You,
        false,
    );
    let mut engine = Engine::new(with_land(state, P0, ZoneKind::Library));
    assert!(lists_land(&engine, LAND));
    engine
        .submit_action(play_land(P0, LAND))
        .expect("the permission opens the top card");
    drain(&mut engine);
    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Library, ZoneKind::Battlefield));
    assert_eq!(engine.state().lands_played_this_turn.get(&P0), Some(&1));
}

// CR 401.5
#[test]
fn a_library_top_permission_casts_the_top_card() {
    let sorcery = ObjectId(21);
    let state = with_top_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        FilterController::You,
        true,
    );
    let mut engine = Engine::new(with_card(
        state,
        sorcery,
        definition(3, "Test Sorcery", CardTypeSet::SORCERY),
        P0,
        ZoneKind::Library,
    ));
    assert_eq!(cast_entry(&engine, sorcery), Some((true, Vec::new())));
    try_cast_spell(&mut engine, P0, sorcery, Vec::new()).expect("a sorcery from the top");
    drain(&mut engine);
    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Library, ZoneKind::Stack));
}

// CR 401.5
#[test]
fn only_the_top_card_is_open_and_the_next_one_opens_after_a_play() {
    let second = ObjectId(21);
    let state = with_top_permission(
        main_phase_state(),
        ZonePermissionKind::PlayLand,
        FilterController::You,
        false,
    );
    let state = with_land(state, P0, ZoneKind::Library);
    let mut state = with_card(
        state,
        second,
        definition(3, "Second Land", CardTypeSet::LAND),
        P0,
        ZoneKind::Library,
    );
    let mut engine = Engine::new(state.clone());
    assert!(!lists_land(&engine, second));
    assert_action_rejected(&mut engine, "not the top", play_land(P0, second));

    state.land_play_modifiers.push(LandPlayModifier {
        source: CRUCIBLE,
        controller: P0,
        affected: LandPlayAffected::Filter(FilterController::You),
        additional: 1,
    });
    let mut engine = Engine::new(state);
    engine
        .submit_action(play_land(P0, LAND))
        .expect("the top land");
    drain(&mut engine);
    assert!(lists_land(&engine, second));
}

// CR 401.5
#[test]
fn an_opponents_entries_never_hold_your_library_top() {
    let instant = ObjectId(21);
    for reveals_top in [false, true] {
        let state = with_top_permission(
            main_phase_state(),
            ZonePermissionKind::Cast,
            FilterController::You,
            reveals_top,
        );
        let mut state = with_card(
            state,
            instant,
            definition(3, "Test Instant", CardTypeSet::INSTANT),
            P0,
            ZoneKind::Library,
        );
        state.priority_player = P1;
        let mut engine = Engine::new(state);
        assert_eq!(
            cast_entry(&engine, instant),
            None,
            "reveals_top {reveals_top}"
        );
        assert_action_rejected(&mut engine, "opponent", cast_action(P1, instant, None));
    }
}

// CR 401.5
#[test]
fn a_permission_that_affects_every_player_lists_the_top_for_an_opponent() {
    let instant = ObjectId(21);
    let state = with_top_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        FilterController::Any,
        true,
    );
    let mut state = with_card(
        state,
        instant,
        definition(3, "Test Instant", CardTypeSet::INSTANT),
        P0,
        ZoneKind::Library,
    );
    state.priority_player = P1;
    let engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, instant), Some((true, Vec::new())));
}

// CR 401.5
#[test]
fn a_permission_over_your_library_does_not_open_an_opponents_top() {
    let state = with_top_permission(
        main_phase_state(),
        ZonePermissionKind::PlayLand,
        FilterController::You,
        true,
    );
    let mut engine = Engine::new(with_land(state, P1, ZoneKind::Library));
    assert!(!lists_land(&engine, LAND));
    assert_action_rejected(&mut engine, "opponent's library", play_land(P0, LAND));
}

// CR 607.2a
#[test]
fn a_linked_permission_casts_only_cards_exiled_with_its_source() {
    let (linked, unlinked) = (ObjectId(21), ObjectId(22));
    let state = with_linked_permission(main_phase_state());
    let state = with_card(
        state,
        linked,
        definition(3, "Linked Sorcery", CardTypeSet::SORCERY),
        P1,
        ZoneKind::Exile,
    );
    let mut state = with_card(
        state,
        unlinked,
        definition(4, "Unlinked Sorcery", CardTypeSet::SORCERY),
        P1,
        ZoneKind::Exile,
    );
    state.exiled_with.push(LinkedExile {
        source: CRUCIBLE,
        exiled: linked,
    });
    let mut engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, unlinked), None);
    assert_action_rejected(&mut engine, "not linked", cast_action(P0, unlinked, None));
    assert_eq!(cast_entry(&engine, linked), Some((true, Vec::new())));
    try_cast_spell(&mut engine, P0, linked, Vec::new()).expect("a linked card");
    drain(&mut engine);
    let (_, from, _) = last_zone_change(&engine);
    assert_eq!(from, ZoneKind::Exile);
    assert!(engine.state().exiled_with.is_empty());
}

// CR 607.2a, 611.3b
#[test]
fn a_source_leaving_drops_its_linked_cards() {
    let (bounce, linked) = (ObjectId(21), ObjectId(22));
    let mut bounce_card = definition(3, "Test Bounce", CardTypeSet::INSTANT);
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
    let state = with_linked_permission(main_phase_state());
    let state = with_card(state, bounce, bounce_card, P0, ZoneKind::Hand);
    let mut state = with_card(
        state,
        linked,
        definition(4, "Linked Sorcery", CardTypeSet::SORCERY),
        P1,
        ZoneKind::Exile,
    );
    state.exiled_with.push(LinkedExile {
        source: CRUCIBLE,
        exiled: linked,
    });
    let mut engine = Engine::new(state);
    try_cast_spell(&mut engine, P0, bounce, vec![Target::Object(CRUCIBLE)]).expect("instant");
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    drain(&mut engine);

    assert!(engine.state().exiled_with.is_empty());
    assert_eq!(cast_entry(&engine, linked), None);
}

// CR 601.3f
#[test]
fn a_face_down_exiled_card_is_never_offered() {
    let sorcery = ObjectId(21);
    let state = with_permission(
        main_phase_state(),
        ZonePermissionKind::Cast,
        ZoneKind::Exile,
        FilterController::You,
    );
    let mut state = with_card(
        state,
        sorcery,
        definition(3, "Test Sorcery", CardTypeSet::SORCERY),
        P0,
        ZoneKind::Exile,
    );
    state.objects.get_mut(&sorcery).unwrap().face_down = true;
    let mut engine = Engine::new(state);
    assert_eq!(cast_entry(&engine, sorcery), None);
    assert_action_rejected(&mut engine, "face down", cast_action(P0, sorcery, None));
}
