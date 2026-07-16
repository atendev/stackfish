//! Phase 5 vertical slice: alternative costs / flashback (CR 601.2b, 702.34a).
//! `CardDefinition::alternative_costs` is the persistent data a card carries
//! (previously nothing populated `CostSpec::alternative`/`AlternativeCost`,
//! left over from Phase 1); `PlayerActionKind::CastSpell::alternative_cost`
//! is the cast-time announcement (same phase as `x_value`, see
//! `phase5_x_costs.rs`) of which one, if any, is being invoked.
//!
//! Scoped to flashback specifically: paying the alternative cost entirely
//! replaces the mana cost, casting from the alternative's own `from_zone`
//! (graveyard) rather than hand is what the alternative grants, and the
//! spell exiles instead of going to the graveyard the next time it leaves
//! the stack - whether by resolving normally or by fizzling (CR 608.2b).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;

/// "Test Flashback Bolt": `{1}{R}` instant, targets a creature, with a
/// flashback alternative cost of a bare `{R}` from the graveyard.
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
            filter: TargetFilter::Object { card_types: Some(CardTypeSet::CREATURE), controller: None, zone: None },
            count: 1,
        })),
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Flashback {R}".to_string(),
            cost: CostSpec::mana_only(ManaCost {
                symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
            }),
            from_zone: ZoneKind::Graveyard,
            exile_on_leaving_stack: true,
        }],
        effect: None,
        modes: Vec::new(),
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
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn two_player_state(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: pool,
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        // Deliberately not 0/1/2 - those collide with the literal ObjectIds
        // the tests below assign to fixture cards, same reasoning as
        // phase5_cast_spell.rs's two_player_state.
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
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
            alternative_cost,
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

/// Test case 1: cast from hand with no `alternative_cost` announced -
/// entirely unaffected by this slice, still pays the printed `{1}{R}`.
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

/// Test case 2: cast from the graveyard with the flashback index announced -
/// legal, paid with the flashback cost (`{R}`), not the mana cost
/// (`{1}{R}` would need the colorless too, which the pool below lacks).
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

/// Test case 3: the card is in the graveyard, but no alternative is
/// announced - rejected the same way as any other "card is not in the zone
/// required to cast it this way" case (plain hand-casting still requires
/// hand).
#[test]
fn cast_from_graveyard_without_declaring_alternative_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), None).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Graveyard, "card untouched");
}

/// Test case 4: resolving a flashback-cast spell exiles it rather than
/// sending it to the graveyard (CR 702.34a).
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

/// Test case 5: a flashback-cast spell that fizzles (CR 608.2b, its only
/// target became illegal) also lands in exile, not the graveyard - CR
/// 702.34a's "as it resolves or as it leaves the stack otherwise" covers
/// both paths.
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

    // Simulate the target leaving the battlefield before this resolves -
    // same technique phase5_targeting_basics.rs's fizzle test uses.
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

/// Test case 6: an out-of-range `alternative_cost` index is rejected before
/// anything is mutated.
#[test]
fn out_of_range_alternative_cost_index_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Some(5)).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Graveyard, "card untouched");
}

/// Test case 7: the card is in hand (not the alternative's `from_zone`) but
/// the alternative is announced anyway - rejected. Simplest correct rule
/// this slice implements: the announced alternative's `from_zone` must
/// match where the card actually is, full stop (see commands.rs's
/// `CastSpell` arm doc comment).
#[test]
fn casting_from_hand_while_declaring_the_alternative_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Some(0)).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "card untouched");
}
