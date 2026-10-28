//! CR 107.3, 601.2b: {X} costs on `CastSpell`. The announced `x_value` must agree with whether the mana cost has an `{X}`, then `ManaCost::resolve_x` substitutes it as generic before `PaymentPlan::validate`.

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, single_player_state};

fn spell_definition(mana_cost: ManaCost) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test X Spell".to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_x_spell".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    GameState {
        step: Step::Untap,
        players: vec![PlayerState { mana_pool: pool, ..stackfish_testutil::player_state(p0) }],
        ..single_player_state(
            p0,
            vec![
                Zone::new(ZoneKind::Hand, Some(p0)),
                Zone::new(ZoneKind::Battlefield, None),
                Zone::new(ZoneKind::Graveyard, Some(p0)),
            ],
        )
    }
}

#[test]
fn x_spell_cast_with_announced_value_is_payable() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
    };
    let mut state =
        state_with_pool(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(3),
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("X announced matches the cost's {X}");
    let status = drain(&mut engine);
    let _ = match status {
        StepStatus::AwaitingPayment { player, stack_id } if player == p0 => stack_id,
        other => panic!("expected AwaitingPayment, got {other:?}"),
    };

    let units = vec![
        ManaUnit::Colored(Color::Red),
        ManaUnit::Colorless,
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("3 generic (for X=3) + 1 red covers {X}{R} exactly");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.red, pool.colorless), (0, 0), "pool fully debited");
}

// CR 601.2b
// Casting an {X}{R} spell without announcing X is rejected before anything is mutated.
#[test]
fn x_spell_cast_without_announcing_x_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
    };
    let mut state = state_with_pool(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "card never left hand");
}

// Announcing X for a spell whose cost has no {X} is rejected.
#[test]
fn announcing_x_for_a_non_x_spell_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let mut state = state_with_pool(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(1),
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

// CR 107.3, 118.5a
// {X} alone with x_value: Some(0) is legal, paid with an explicit empty-units Mana entry.
#[test]
fn x_only_cost_with_zero_value_needs_an_explicit_empty_mana_entry() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::X] };
    let mut state = state_with_pool(ManaPool::default());
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(0),
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("X=0 is a legal announced value");
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: Vec::new(), phyrexian_life: 0 }],
        })
        .expect("X resolved to {0}, paid with an explicit empty Mana entry");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}
