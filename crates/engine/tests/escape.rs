//! CR 702.138 Escape: a cast from the graveyard through `AlternativeCost::Replacement { from_zone: Graveyard }`, like Flashback, plus the cost "exile N other cards from your graveyard".
//! `AdditionalCost::ExileFromGraveyard { filter, count }` pays with `CostPayment::Exile(ObjectId)`, and `propose_zone_move` takes a real `to` so the exile is replacement-checked.
//! The escaping card is already on the stack when the cost is paid (CR 601.2a), so it cannot be among its own exile candidates. Provenance is in escape_provenance.rs.

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

fn escape_creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Escape Creature".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Black)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Escape—{1}{B}, Exile three other cards from your graveyard.".to_string(),
        power: Some(PTValue::Fixed(3)),
        toughness: Some(PTValue::Fixed(3)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_escape_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Escape—{1}{B}, Exile three other cards from your graveyard.".to_string(),
            cost: CostSpec {
                mana: Some(ManaCost {
                    symbols: vec![
                        ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                        ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Black)),
                    ],
                }),
                additional: vec![AdditionalCost::ExileFromGraveyard {
                    filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
                    count: 3,
                }],
                alternative: Vec::new(),
            },
            from_zone: ZoneKind::Graveyard,
            exile_on_leaving_stack: false,
            escape: true,
        }],
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn filler_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_filler".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn state_with_escaper_and_fodder(pool: ManaPool, card: ObjectId, fodder_ids: &[ObjectId]) -> GameState {
    let p0 = PlayerId(0);
    let mut state = two_player_state(pool);
    state.objects.insert(card, card_object(card, escape_creature_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    for (i, &id) in fodder_ids.iter().enumerate() {
        state.objects.insert(id, card_object(id, filler_definition(10 + i as u64, &format!("Fodder {i}")), p0, ZoneKind::Graveyard));
        state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(id);
    }
    state
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, alternative_cost: Option<usize>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost , pay_buyback: false},
        })
        .expect("player controls the card and holds priority");
}

fn try_cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, alternative_cost: Option<usize>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost , pay_buyback: false},
        })
        
}

// A payment can commit several zone moves, so find the one whose destination is `to`.
fn zone_change_to(engine: &Engine, to: ZoneKind) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to: event_to, .. } = event.kind
            && event_to == to {
                return (object, from, event_to);
            }
    }
    panic!("expected a ZoneChange event with to={to:?} in the ledger");
}

#[test]
fn cast_from_graveyard_with_escape_cost_exiles_the_other_graveyard_cards() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let fodder = [ObjectId(2), ObjectId(3), ObjectId(4)];
    let mut state = state_with_escaper_and_fodder(
        ManaPool { black: 1, colorless: 1, ..ManaPool::default() },
        card,
        &fodder,
    );
    // A fifth graveyard card that is not exiled shows the cost takes exactly `count`.
    let untouched = ObjectId(5);
    state.objects.insert(untouched, card_object(untouched, filler_definition(20, "Untouched"), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(untouched);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Some(0));
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Black), ManaUnit::Colorless], phyrexian_life: 0 },
                CostPayment::Exile(fodder[0]),
                CostPayment::Exile(fodder[1]),
                CostPayment::Exile(fodder[2]),
            ],
        })
        .expect("escape's {1}{B} + exile three other graveyard cards is a legal payment");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let (new_id, from, to) = zone_change_to(&engine, ZoneKind::Stack);
    assert_eq!((from, to), (ZoneKind::Graveyard, ZoneKind::Stack), "the escaping card itself moved to the stack");
    assert_ne!(new_id, card, "CR 400.7 fresh id");

    let exile = engine.state().zone(ZoneKind::Exile, None).unwrap();
    for id in fodder {
        assert!(engine.state().object(id).is_none(), "the old fodder id is retired");
        assert_eq!(exile.contents.iter().filter(|&&o| engine.state().object(o).unwrap().definition.name.starts_with("Fodder")).count(), 3);
    }
    assert!(engine.state().object(untouched).is_some(), "the untouched graveyard card is still there, unexiled");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1, "only the untouched filler remains in the graveyard");
}

#[test]
fn escape_payment_naming_fewer_than_count_exile_targets_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let fodder = [ObjectId(2), ObjectId(3), ObjectId(4)];
    let state = state_with_escaper_and_fodder(ManaPool { black: 1, colorless: 1, ..ManaPool::default() }, card, &fodder);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Some(0));
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Black), ManaUnit::Colorless], phyrexian_life: 0 },
                CostPayment::Exile(fodder[0]),
                CostPayment::Exile(fodder[1]),
            ],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    for id in fodder {
        assert!(engine.state().object(id).is_some(), "no partial mutation - fodder untouched");
    }
}

#[test]
fn escape_payment_cannot_reference_a_duplicate_card() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let fodder = [ObjectId(2), ObjectId(3), ObjectId(4)];
    let state = state_with_escaper_and_fodder(ManaPool { black: 1, colorless: 1, ..ManaPool::default() }, card, &fodder);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Some(0));
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Black), ManaUnit::Colorless], phyrexian_life: 0 },
                CostPayment::Exile(fodder[0]),
                CostPayment::Exile(fodder[0]),
                CostPayment::Exile(fodder[1]),
            ],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
}

#[test]
fn escape_payment_must_reference_cards_actually_in_the_graveyard() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let fodder = [ObjectId(2), ObjectId(3), ObjectId(4)];
    let mut state = state_with_escaper_and_fodder(ManaPool { black: 1, colorless: 1, ..ManaPool::default() }, card, &fodder);
    let battlefield_creature = ObjectId(6);
    state.objects.insert(battlefield_creature, card_object(battlefield_creature, filler_definition(30, "On Battlefield"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(battlefield_creature);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Some(0));
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Black), ManaUnit::Colorless], phyrexian_life: 0 },
                CostPayment::Exile(fodder[0]),
                CostPayment::Exile(fodder[1]),
                CostPayment::Exile(battlefield_creature),
            ],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert!(engine.state().object(battlefield_creature).unwrap().zone == ZoneKind::Battlefield, "no partial mutation");
}

// Casting from hand with the escape alternative is rejected, since from_zone must match where the card is.
#[test]
fn casting_from_hand_while_declaring_escape_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { black: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, escape_creature_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, Some(0)).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "card untouched");
}

// `Game.Costs.ExileFromGraveyard` decodes through the generic `Game.CreateActivatedAbility` builder.
const EXILE_FROM_GRAVEYARD_COST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.ExileFromGraveyard({}, 3))
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

#[test]
fn run_card_script_self_registers_an_exile_from_graveyard_cost() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = two_player_state(ManaPool::default());
    let definition = CardDefinition { script_ref: ScriptReference::Inline(EXILE_FROM_GRAVEYARD_COST.to_string()), ..filler_definition(1, "Cost Test") };
    state.objects.insert(source, card_object(source, definition.clone(), p0, ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    let cost = state.abilities[0].cost.as_ref().expect("AddCost(ExileFromGraveyard) was called");
    match cost.additional.as_slice() {
        [AdditionalCost::ExileFromGraveyard { count, .. }] => assert_eq!(*count, 3),
        other => panic!("expected [ExileFromGraveyard], got {other:?}"),
    }
}
