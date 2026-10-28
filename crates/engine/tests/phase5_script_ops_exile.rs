//! `AbilityEffect::Exile` (`Game.Ops.Exile()`) consults `apply_replacements` and then inlines its own zone move through `state.zone_mut(Exile, None)`, since `sba::commit_death` hardcodes `Some(owner)` and Exile is shared. It ignores indestructibility, which is Destroy-specific (CR 702.12b).
// TODO: exiling from the graveyard or hand, which needs the zone to remove from at resolution time

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(definition_id: u64, name: &str, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn exile_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Exile".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Exile target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_exile".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Exile),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn leaves_battlefield_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::LeavesBattlefield)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn cast_exile_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, target: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: vec![Target::Object(target)],
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn exile_spell_moves_target_from_battlefield_to_exile() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Target Bear", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "exiled creature left the battlefield");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "exiled creature is in the exile zone");
}

#[test]
fn exile_activated_ability_also_works() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let target = ObjectId(1);
    state.objects.insert(source, card_object(source, creature_definition(1, "Exiler", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(target, card_object(target, creature_definition(2, "Prey", KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Exile),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: vec![Target::Object(target)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] }).expect("Tap pays TapSelf");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert!(!bf.contains(target), "target was exiled off the battlefield");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "target is in exile");
}

#[test]
fn exile_fires_leaves_battlefield_trigger() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Watchful Bear", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    // "When this leaves the battlefield, gain 1 life." Exile is a Battlefield to Exile departure, so LeavesBattlefield fires though the creature did not die.
    state.abilities.push(leaves_battlefield_ability(target));
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);

    // Exile resolves — creature departs via exile, not death.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "creature is in exile");
    assert_eq!(engine.state().stack.len(), 1, "LeavesBattlefield trigger was collected off the exile ZoneChange");

    // Let the trigger resolve.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the LeavesBattlefield trigger");
}

#[test]
fn exile_ignores_indestructible() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    // Exile bypasses indestructibility, which would leave a destroy target untouched (CR 702.12b).
    state.objects.insert(target, card_object(target, creature_definition(1, "Indestructible Bear", KeywordSet::INDESTRUCTIBLE), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "indestructible creature was still exiled");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1);
}

#[test]
fn exile_leaves_unrelated_permanent_untouched() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let bystander = ObjectId(1);
    state.objects.insert(target, card_object(target, creature_definition(1, "Target", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.objects.insert(bystander, card_object(bystander, creature_definition(2, "Bystander", KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bystander);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert!(!bf.contains(target), "exiled creature left the battlefield");
    assert!(bf.contains(bystander), "bystander remains on the battlefield");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "only one object in exile");
}
