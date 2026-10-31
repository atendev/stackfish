//! CR 400.7: battlefield-only state is dropped on every zone move (`GameObject::reset_for_new_object`), and `Exile` and `ReturnToHand` read the target's real source zone at resolution.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const TARGET: ObjectId = ObjectId(0);
const SPELL: ObjectId = ObjectId(1);

fn creature() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            1,
            "Bear",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn spell(effect: AbilityEffect, zone: Option<ZoneKind>) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        ..minimal_card_definition(
            2,
            "Test Spell",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

fn setup(effect: AbilityEffect, filter_zone: Option<ZoneKind>, zone: ZoneKind) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
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
    let mut target = card_object(TARGET, creature(), P1, zone);
    target.controller = P0;
    target.tapped = true;
    target.damage_marked = 1;
    target
        .counters
        .add(CounterKind::PlusOnePlusOne, 2, SequenceNo(0));
    state.objects.insert(TARGET, target);
    let owner = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) {
        None
    } else {
        Some(P1)
    };
    state.zone_mut(zone, owner).unwrap().push_bottom(TARGET);
    state.objects.insert(
        SPELL,
        card_object(SPELL, spell(effect, filter_zone), P0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(SPELL);
    Engine::new(state)
}

fn cast(engine: &mut Engine) -> Result<(), ()> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: SPELL,
                modes: Vec::new(),
                targets: vec![Target::Object(TARGET)],
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .map_err(|_| ())?;
    assert!(matches!(drain(engine), StepStatus::AwaitingPayment { .. }));
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
    for player in [P0, P1] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        drain(engine);
    }
    Ok(())
}

fn the_bear(engine: &Engine, zone: ZoneKind) -> GameObject {
    engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Bear" && o.zone == zone)
        .unwrap_or_else(|| panic!("no Bear in {zone:?}"))
        .clone()
}

fn assert_clean(bear: &GameObject) {
    assert!(!bear.tapped);
    assert_eq!(bear.damage_marked, 0);
    assert_eq!(bear.counters.count(&CounterKind::PlusOnePlusOne), 0);
    assert_eq!(bear.controller, bear.owner, "CR 108.4");
}

#[test]
fn destroy_resets_state_on_the_graveyard_object() {
    let mut engine = setup(AbilityEffect::Destroy, None, ZoneKind::Battlefield);
    cast(&mut engine).unwrap();
    assert_clean(&the_bear(&engine, ZoneKind::Graveyard));
}

#[test]
fn exile_resets_state_on_the_exiled_object() {
    let mut engine = setup(AbilityEffect::Exile, None, ZoneKind::Battlefield);
    cast(&mut engine).unwrap();
    assert_clean(&the_bear(&engine, ZoneKind::Exile));
}

#[test]
fn return_to_hand_can_target_a_card_in_a_graveyard() {
    let mut engine = setup(
        AbilityEffect::ReturnToHand,
        Some(ZoneKind::Graveyard),
        ZoneKind::Graveyard,
    );
    cast(&mut engine).unwrap();
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(P1))
            .unwrap()
            .contents
            .is_empty()
    );
    let bear = the_bear(&engine, ZoneKind::Hand);
    assert_eq!(bear.owner, P1);
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(P1))
            .unwrap()
            .contents
            .contains(&bear.object_id)
    );
    assert_clean(&bear);
}

#[test]
fn return_to_hand_can_target_an_exiled_card() {
    let mut engine = setup(
        AbilityEffect::ReturnToHand,
        Some(ZoneKind::Exile),
        ZoneKind::Exile,
    );
    cast(&mut engine).unwrap();
    assert!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty()
    );
    assert_clean(&the_bear(&engine, ZoneKind::Hand));
}

#[test]
fn exile_can_target_a_card_in_a_graveyard() {
    let mut engine = setup(
        AbilityEffect::Exile,
        Some(ZoneKind::Graveyard),
        ZoneKind::Graveyard,
    );
    cast(&mut engine).unwrap();
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(P1))
            .unwrap()
            .contents
            .is_empty()
    );
    assert_clean(&the_bear(&engine, ZoneKind::Exile));
}

#[test]
fn a_battlefield_filter_cannot_hit_a_graveyard_card() {
    let mut engine = setup(AbilityEffect::ReturnToHand, None, ZoneKind::Graveyard);
    let submitted = engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::CastSpell {
            card: SPELL,
            modes: Vec::new(),
            targets: vec![Target::Object(TARGET)],
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    });
    let status = drain(&mut engine);
    let rejected = submitted.is_err() || !matches!(status, StepStatus::AwaitingPayment { .. });
    assert!(
        rejected,
        "default (battlefield) filter must reject a graveyard target, got {status:?}"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(P1))
            .unwrap()
            .contents
            .len(),
        1,
        "card stays put"
    );
}
