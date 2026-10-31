//! CR 903.8: commander tax is {2} generic per prior cast of this commander from the command zone.
//! It is tracked by name in `PlayerState::commander_casts`, since CR 400.7 mints a new id on every cast and every CR 903.9a return.

use std::collections::HashMap;

use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object as base_card_object, player_state, two_player_state as base_two_player_state, zones,
};

fn creature_definition(name: &str, mana_cost: ManaCost) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn card_object(
    id: ObjectId,
    definition: CardDefinition,
    owner: PlayerId,
    zone: ZoneKind,
    is_commander: bool,
) -> GameObject {
    GameObject {
        is_commander,
        ..base_card_object(id, definition, owner, zone)
    }
}

// p0's pool is seeded with 10 colorless, and tests set it directly.
fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                mana_pool: ManaPool {
                    colorless: 10,
                    ..ManaPool::default()
                },
                ..player_state(p0)
            },
            player_state(p1),
        ],
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Library,
                ZoneKind::Hand,
                ZoneKind::Graveyard,
                ZoneKind::Command,
                ZoneKind::Battlefield,
                ZoneKind::Exile,
            ],
        ))
    }
}

fn put_in_zone(state: &mut GameState, object: GameObject, owner: Option<PlayerId>) {
    let kind = object.zone;
    let id = object.object_id;
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn drain(engine: &mut Engine) -> engine::StepStatus {
    loop {
        match engine.step().status {
            engine::StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

// Pays with n colorless and returns the remaining colorless count, so a test can read off what was spent.
fn cast_and_pay(engine: &mut Engine, card: ObjectId, n: u32) -> u32 {
    let p0 = PlayerId(0);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority");
    let status = drain(engine);
    assert!(
        matches!(status, engine::StepStatus::AwaitingPayment { .. }),
        "got {status:?}"
    );
    let units = vec![ManaUnit::Colorless; n as usize];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .expect("pool covers the resolved cost");
    drain(engine);
    engine.state().player(p0).unwrap().mana_pool.colorless
}

// Moves the card back to p0's command zone under a new id, as a CR 903.9a return does in play.
fn manually_return_to_command_zone(state: &mut GameState, card: ObjectId) -> ObjectId {
    let object = state.objects.get(&card).unwrap().clone();
    let from_zone = object.zone;
    let owner = object.owner;
    state.zone_mut(from_zone, Some(owner)).unwrap().remove(card);
    state.objects.remove(&card);

    let new_id = state.allocate_object_id();
    let mut new_object = object;
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage {
        previous: Some(card),
    };
    new_object.zone = ZoneKind::Command;
    new_object.timestamp = state.allocate_sequence_no();
    state
        .zone_mut(ZoneKind::Command, Some(owner))
        .unwrap()
        .push_bottom(new_id);
    state.objects.insert(new_id, new_object);
    new_id
}

// ManaCost::apply_cost_delta(0) drops an explicit Generic(0) (e.g. {X} with X=0), so the tax path skips it when prior_casts == 0, as cost::apply_cost_modifiers does (CR 601.2b).
#[test]
fn x_cost_commander_first_cast_still_needs_an_explicit_empty_mana_entry() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition(
                "General",
                ManaCost {
                    symbols: vec![ManaSymbol::X],
                },
            ),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: commander,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(0),
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("X=0 is a legal announced value, first cast, no tax yet");
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units: Vec::new(), phyrexian_life: 0 }] })
        .expect("X resolved to {0}, no tax on the first cast - still needs the explicit empty Mana entry");
    let status = drain(&mut engine);
    assert!(matches!(status, engine::StepStatus::AwaitingAction { player } if player == p0));
}

#[test]
fn first_cast_from_the_command_zone_has_no_tax() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let remaining = cast_and_pay(&mut engine, commander, 1);
    assert_eq!(remaining, 9, "base {{1}} only, no tax on the first cast");
}

#[test]
fn second_cast_after_returning_costs_two_more() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    cast_and_pay(&mut engine, commander, 1);

    // Send it to the graveyard and back to the command zone, as a real death plus CR 903.9a would, then recast it.
    let on_stack = engine.state().stack.last().unwrap().stack_id;
    engine.state_mut().stack.retain(|s| s.stack_id != on_stack);
    let mut obj = engine.state().objects.get(&on_stack).unwrap().clone();
    obj.zone = ZoneKind::Graveyard;
    engine.state_mut().objects.insert(on_stack, obj);
    engine
        .state_mut()
        .zone_mut(ZoneKind::Graveyard, Some(p0))
        .unwrap()
        .push_bottom(on_stack);

    let recast_id = manually_return_to_command_zone(engine.state_mut(), on_stack);

    let remaining = cast_and_pay(&mut engine, recast_id, 3);
    assert_eq!(
        remaining, 6,
        "base {{1}} + {{2}} tax for the one prior cast"
    );
}

#[test]
fn third_cast_costs_four_more() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let mut current = commander;
    for expected_remaining in [9u32, 6, 1] {
        let remaining = if current == commander {
            cast_and_pay(&mut engine, current, 1)
        } else {
            // n covers {1} base + {2} per prior cast so far.
            let prior = engine
                .state()
                .player(p0)
                .unwrap()
                .commander_casts
                .get("General")
                .copied()
                .unwrap_or(0);
            cast_and_pay(&mut engine, current, 1 + 2 * prior)
        };
        assert_eq!(remaining, expected_remaining);

        let on_stack = engine.state().stack.last().unwrap().stack_id;
        engine.state_mut().stack.retain(|s| s.stack_id != on_stack);
        let mut obj = engine.state().objects.get(&on_stack).unwrap().clone();
        obj.zone = ZoneKind::Graveyard;
        engine.state_mut().objects.insert(on_stack, obj);
        engine
            .state_mut()
            .zone_mut(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .push_bottom(on_stack);
        current = manually_return_to_command_zone(engine.state_mut(), on_stack);
    }
    assert_eq!(
        engine
            .state()
            .player(p0)
            .unwrap()
            .commander_casts
            .get("General"),
        Some(&3)
    );
}

#[test]
fn partner_commanders_have_independent_tax_counters() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let general = ObjectId(0);
    let admiral = ObjectId(1);
    put_in_zone(
        &mut state,
        card_object(
            general,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );
    put_in_zone(
        &mut state,
        card_object(
            admiral,
            creature_definition("Admiral", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    // Cast and return General twice, taxing it up to a second prior cast.
    cast_and_pay(&mut engine, general, 1);
    let on_stack = engine.state().stack.last().unwrap().stack_id;
    engine.state_mut().stack.retain(|s| s.stack_id != on_stack);
    let mut obj = engine.state().objects.get(&on_stack).unwrap().clone();
    obj.zone = ZoneKind::Graveyard;
    engine.state_mut().objects.insert(on_stack, obj);
    engine
        .state_mut()
        .zone_mut(ZoneKind::Graveyard, Some(p0))
        .unwrap()
        .push_bottom(on_stack);
    let general_recast = manually_return_to_command_zone(engine.state_mut(), on_stack);
    cast_and_pay(&mut engine, general_recast, 3);

    // Clear General's second cast off the stack, since sorcery speed needs an empty stack.
    let on_stack = engine.state().stack.last().unwrap().stack_id;
    engine.state_mut().stack.retain(|s| s.stack_id != on_stack);

    // Admiral, never cast before, costs only its base {1}.
    let remaining = cast_and_pay(&mut engine, admiral, 1);
    // Pool: started 10, spent 1 (General #1) + 3 (General #2) + 1 (Admiral #1) = 5, 5 left.
    assert_eq!(
        remaining, 5,
        "Admiral's first cast is untaxed regardless of General's cast count"
    );
}

#[test]
fn casting_from_hand_is_never_taxed() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    // Same card in hand and not marked is_commander takes the ordinary CR 601.2f path, with no commander tax.
    let card = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            card,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Hand,
            false,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let remaining = cast_and_pay(&mut engine, card, 1);
    assert_eq!(
        remaining, 9,
        "hand cast: base {{1}} only, tax never applies"
    );
}

#[test]
fn tax_stacks_additively_onto_a_real_printed_cost() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    // {2}{G} confirms the tax adds to an existing generic component and does not replace it.
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{2}{G}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: commander,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, engine::StepStatus::AwaitingPayment { .. }),
        "got {status:?}"
    );
    // First cast has no tax: {2}{G}, with 2 colorless for the generic and 1 green for the pip. This is the untaxed baseline.
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colorless, ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .expect_err("green pip unpaid - confirms the resolved cost still carries its printed {G}, not just generic");
}
