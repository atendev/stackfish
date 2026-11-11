//! CR 603.4: an intervening-if clause is checked twice: when the trigger fires (in `collect_triggers`) and again as it would resolve (`ResolveStackTop`, CR 608.2a), and if false the ability is removed and does nothing.
//! `ControllerAtOrBelowLife(n)` is the shape tested, and a response spell that changes life between the two checks proves they are independent.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, creature_definition, hand_battlefield_graveyard_zones, pass_both_and_drain,
    pay_free_and_drain, player_state, two_player_state as base_two_player_state,
};

fn heal_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        oracle_id: None,
        name: "Test Heal".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "You gain 10 life.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_heal".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::GainLife(10)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn interveningif_upkeep_ability(source: ObjectId, threshold: i32) -> RegisteredAbility {
    RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: Some(InterveningIfSpec::Primitive(
            InterveningIfPrimitive::ControllerAtOrBelowLife(threshold),
        )),
    }
}

fn two_player_state(p0_life: i32) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        players: vec![
            PlayerState {
                life: p0_life,
                ..player_state(p0)
            },
            player_state(p1),
        ],
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

#[test]
fn intervening_if_false_at_trigger_time_never_triggers_at_all() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 10 > threshold 5, so the intervening-if is false when upkeep begins.
    let mut state = two_player_state(10);
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    state
        .abilities
        .push(interveningif_upkeep_ability(permanent, 5));

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Upkeep);
    assert!(
        engine.state().stack.is_empty(),
        "intervening-if false at trigger time - never collected at all"
    );
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        10,
        "no GainLife applied"
    );
}

#[test]
fn intervening_if_true_throughout_resolves_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 5 == threshold 5 - holds ("at or below").
    let mut state = two_player_state(5);
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    state
        .abilities
        .push(interveningif_upkeep_ability(permanent, 5));

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().stack.len(),
        1,
        "intervening-if true at trigger time - collected and pushed"
    );

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().stack.is_empty(),
        "resolved (still true at resolution too)"
    );
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        6,
        "GainLife(1) applied"
    );
}

#[test]
fn a_response_spell_invalidating_the_condition_fizzles_the_trigger_at_resolution() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 3 <= threshold 5 - true at trigger time.
    let mut state = two_player_state(3);
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    state
        .abilities
        .push(interveningif_upkeep_ability(permanent, 5));
    let heal = ObjectId(1);
    state.objects.insert(
        heal,
        card_object(heal, heal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(heal);

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().stack.len(),
        1,
        "intervening-if true at trigger time - collected and pushed"
    );

    // p0 holds priority with the trigger on the stack and responds with Test Heal, which goes on top (CR 601.2i, 608.1).
    cast_spell(&mut engine, p0, heal);
    pay_free_and_drain(&mut engine);
    assert_eq!(
        engine.state().stack.len(),
        2,
        "the heal spell sits on top of the still-pending upkeep trigger"
    );

    // The heal resolves first and raises life to 13, above the threshold, before the trigger resolves.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        13,
        "Test Heal resolved first"
    );
    assert_eq!(
        engine.state().stack.len(),
        1,
        "only the upkeep trigger remains"
    );

    // The trigger's CR 608.2a recheck sees life 13 > threshold 5, so it is removed from the stack and does nothing.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        13,
        "GainLife(1) did NOT apply - the intervening-if was false at resolution"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "a Fizzled event was logged for the invalidated trigger"
    );
}
