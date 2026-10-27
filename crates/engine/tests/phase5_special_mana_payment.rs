//! Phase 5 vertical slice: paying `{C}`, hybrid, `{X}` and `{S}` costs. The
//! engine already validated hybrid, Phyrexian and `{S}` plans in unit tests.
//! This file runs them through `submit_action` and `submit_payment`, and
//! covers the two pieces that were missing: the `{C}` cost symbol (CR 107.4c)
//! and snow mana produced by a snow source (CR 106.3).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, player_state, single_player_state};

const P0: PlayerId = PlayerId(0);

fn spell(mana_cost: ManaCost) -> CardDefinition {
    CardDefinition {
        card_types: CardTypeSet::SORCERY,
        ..minimal_card_definition(1, "Test Spell", "test_spell", mana_cost, CardTypeSet::SORCERY)
    }
}

fn state_with_pool(pool: ManaPool) -> GameState {
    GameState {
        step: Step::Untap,
        last_active_turn_number: HashMap::new(),
        players: vec![PlayerState { mana_pool: pool, ..player_state(P0) }],
        ..single_player_state(
            P0,
            vec![
                Zone::new(ZoneKind::Hand, Some(P0)),
                Zone::new(ZoneKind::Battlefield, None),
                Zone::new(ZoneKind::Graveyard, Some(P0)),
            ],
        )
    }
}

fn engine_with_spell(cost: ManaCost, pool: ManaPool) -> Engine {
    let card = ObjectId(0);
    let mut state = state_with_pool(pool);
    state.objects.insert(card, card_object(card, spell(cost), P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(card);
    Engine::new(state)
}

fn cast(engine: &mut Engine, x_value: Option<u32>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: ObjectId(0),
                modes: Vec::new(),
                targets: Vec::new(),
                x_value,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("the spell is castable");
    assert!(matches!(drain(engine), StepStatus::AwaitingPayment { .. }));
}

fn pay(engine: &mut Engine, units: Vec<ManaUnit>) -> Result<(), PaymentError> {
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
}

fn colorless_cost() -> ManaCost {
    ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2)), ManaSymbol::Atom(AtomicManaSymbol::Colorless)] }
}

#[test]
fn colorless_symbol_is_paid_with_colorless_mana() {
    let mut engine = engine_with_spell(colorless_cost(), ManaPool { colorless: 3, ..ManaPool::default() });
    cast(&mut engine, None);
    pay(&mut engine, vec![ManaUnit::Colorless; 3]).expect("three colorless pay {2}{C}");
    assert_eq!(engine.state().player(P0).unwrap().mana_pool.colorless, 0);
}

#[test]
fn colorless_symbol_refuses_colored_mana() {
    let mut engine = engine_with_spell(colorless_cost(), ManaPool { white: 3, ..ManaPool::default() });
    cast(&mut engine, None);
    let err = pay(&mut engine, vec![ManaUnit::Colored(Color::White); 3]).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(P0).unwrap().mana_pool.white, 3, "a refused plan changes nothing");
}

#[test]
fn colorless_symbol_beside_colored_mana_for_the_generic_part() {
    let mut engine = engine_with_spell(colorless_cost(), ManaPool { white: 2, colorless: 1, ..ManaPool::default() });
    cast(&mut engine, None);
    let units = vec![ManaUnit::Colored(Color::White), ManaUnit::Colored(Color::White), ManaUnit::Colorless];
    pay(&mut engine, units).expect("the colorless unit pays {C} and white pays {2}");
}

#[test]
fn hybrid_symbol_is_paid_with_either_half() {
    let hybrid = ManaCost {
        symbols: vec![ManaSymbol::Hybrid(AtomicManaSymbol::Colored(Color::White), AtomicManaSymbol::Colored(Color::Blue))],
    };
    let mut engine = engine_with_spell(hybrid.clone(), ManaPool { blue: 1, ..ManaPool::default() });
    cast(&mut engine, None);
    pay(&mut engine, vec![ManaUnit::Colored(Color::Blue)]).expect("blue pays {W/U}");

    let mut engine = engine_with_spell(hybrid, ManaPool { red: 1, ..ManaPool::default() });
    cast(&mut engine, None);
    assert!(matches!(pay(&mut engine, vec![ManaUnit::Colored(Color::Red)]), Err(PaymentError::InvalidPlan)));
}

#[test]
fn x_spell_pays_the_announced_value() {
    let cost = ManaCost { symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let pool = ManaPool { red: 1, colorless: 2, ..ManaPool::default() };

    let mut engine = engine_with_spell(cost.clone(), pool.clone());
    cast(&mut engine, Some(2));
    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless, ManaUnit::Colorless];
    pay(&mut engine, units).expect("X=2 and {R} cost three mana");

    let mut engine = engine_with_spell(cost, pool);
    cast(&mut engine, Some(0));
    assert!(
        matches!(pay(&mut engine, vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless]), Err(PaymentError::InvalidPlan)),
        "X=0 costs only {{R}}, so two units are too many"
    );
}

#[test]
fn two_x_symbols_each_take_the_announced_value() {
    let cost = ManaCost { symbols: vec![ManaSymbol::X, ManaSymbol::X] };
    let mut engine = engine_with_spell(cost, ManaPool { colorless: 4, ..ManaPool::default() });
    cast(&mut engine, Some(2));
    pay(&mut engine, vec![ManaUnit::Colorless; 4]).expect("{X}{X} with X=2 costs four");
}

fn snow_land(id: ObjectId, supertypes: SupertypeSet) -> GameObject {
    let definition = CardDefinition {
        supertypes,
        oracle_text: "{T}: Add {G}.".to_string(),
        ..minimal_card_definition(2, "Test Land", "test_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    };
    card_object(id, definition, P0, ZoneKind::Battlefield)
}

fn engine_with_land(land: GameObject) -> Engine {
    let id = land.object_id;
    let mut objects = HashMap::new();
    objects.insert(id, land);
    Engine::new(GameState {
        objects,
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source: id,
            kind: AbilityKind::Activated,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)])),
            intervening_if: None,
        }],
        ..single_player_state(P0, vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::from([id]) }])
    })
}

fn tap_for_mana(engine: &mut Engine, source: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("the land can be tapped");
    drain(engine);
}

#[test]
fn a_snow_land_produces_snow_mana() {
    let land = ObjectId(0);
    let mut engine = engine_with_land(snow_land(land, SupertypeSet::SNOW));
    tap_for_mana(&mut engine, land);

    let pool = &engine.state().player(P0).unwrap().mana_pool;
    assert_eq!((pool.green, pool.snow_green), (1, 1), "the unit is green and snow-tagged");
    assert!(engine.state().ledger.iter().any(|e| matches!(
        &e.kind,
        GameEventKind::ManaAdded { units, .. } if units == &[ManaUnit::SnowColored(Color::Green)]
    )));
}

#[test]
fn a_land_without_the_snow_supertype_produces_plain_mana() {
    let land = ObjectId(0);
    let mut engine = engine_with_land(snow_land(land, SupertypeSet::empty()));
    tap_for_mana(&mut engine, land);

    let pool = &engine.state().player(P0).unwrap().mana_pool;
    assert_eq!((pool.green, pool.snow_green), (1, 0));
}

#[test]
fn snow_mana_from_a_land_pays_a_snow_symbol() {
    let cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Snow)] };
    let mut engine = engine_with_spell(cost, ManaPool { green: 1, snow_green: 1, ..ManaPool::default() });
    cast(&mut engine, None);
    pay(&mut engine, vec![ManaUnit::SnowColored(Color::Green)]).expect("snow green pays {S}");
}
