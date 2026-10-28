//! CR 601.2b, 702.34a: alternative costs and flashback. `CardDefinition::alternative_costs` holds the data, and `CastSpell::alternative_cost` announces which one is used.
//! Paying a flashback cost replaces the mana cost, casts from the graveyard, and exiles the spell when it leaves the stack, whether it resolves or fizzles (CR 608.2b).


use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Flashback Bolt".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Flashback Bolt deals 3 damage to target creature. Flashback {R}."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_flashback_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Flashback {R}".to_string(),
            cost: CostSpec::mana_only(ManaCost {
                symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
            }),
            from_zone: ZoneKind::Graveyard,
            exile_on_leaving_stack: true,
            escape: false,
        }],
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bear".to_string()),
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

fn cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
    alternative_cost: Option<usize>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets,
            x_value: None,
            alternative_cost, pay_buyback: false,
        },
    })
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

#[test]
fn cast_from_hand_without_alternative_cost_uses_mana_cost() {
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(creature, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], None)
        .expect("cast from hand, no alternative announced");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{1}{R} paid with 1 red + 1 generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Hand, ZoneKind::Stack));
}

// The flashback cost {R} is paid and not the mana cost {1}{R}, which the pool could not cover.
#[test]
fn cast_from_graveyard_with_alternative_cost_uses_flashback_cost() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], Some(0))
        .expect("flashback cast from graveyard with the alternative announced");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .expect("flashback's {R} paid with 1 red - the {1}{R} mana cost is not in play here");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Graveyard, ZoneKind::Stack));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(pool.red, 0, "the single red unit paid the flashback cost");
}

// A card in the graveyard with no alternative announced is rejected like any card outside the zone required to cast it.
#[test]
fn cast_from_graveyard_without_declaring_alternative_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = cast_spell(&mut engine, p0, card, Vec::new(), None).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Graveyard, "card untouched");
}

// CR 702.34a
#[test]
fn resolving_flashback_spell_exiles_instead_of_graveyard() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], Some(0)).unwrap();
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .unwrap();
    drain(&mut engine);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Exile));
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.contains(&new_id));
    assert!(!engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.contains(&new_id));
}

// CR 702.34a, 608.2b
// A fizzled flashback spell is also exiled, not sent to the graveyard.
#[test]
fn fizzled_flashback_spell_also_exiles() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], Some(0)).unwrap();
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .unwrap();
    drain(&mut engine);

    // The target leaves the battlefield before this resolves.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(creature);
    engine.state_mut().objects.remove(&creature);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let fizzled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(fizzled, "expected a Fizzled event");

    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Exile));
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.contains(&new_id));
    assert!(!engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.contains(&new_id));
}

// An out-of-range alternative_cost index is rejected before anything is mutated.
#[test]
fn out_of_range_alternative_cost_index_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = cast_spell(&mut engine, p0, card, Vec::new(), Some(5)).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Graveyard, "card untouched");
}

// The announced alternative's from_zone must match where the card is.
#[test]
fn casting_from_hand_while_declaring_the_alternative_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = cast_spell(&mut engine, p0, card, Vec::new(), Some(0)).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "card untouched");
}
