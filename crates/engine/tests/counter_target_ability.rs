//! CR 113.9, 701.6a: `TargetFilter::Ability` and `Target::Ability` for "counter target activated or triggered ability" (Stifle, Trickbind).
//! An ability's `StackObject` has no `GameObject`, so `target_is_legal` matches it against `state.stack` and `legal_target_candidates` scans for it.
//! Resolution is `AbilityEffect::Counter`, covered in counter_target_spell.rs.

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
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

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn stifle_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Stifle".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Counter target activated or triggered ability.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_stifle".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Ability,
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Counter),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn damage_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Player { controller: None },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Damage(1)),
        intervening_if: None,
    }
}

// Pushes a triggered ability directly onto the stack, since CR 113.9 draws no distinction between it and an activated one as a Counter target.
fn push_triggered_ability_on_stack(state: &mut GameState, stack_id: ObjectId, source: ObjectId, owner: PlayerId) {
    state.stack.push(StackObject::new(
        stack_id,
        StackEntryKind::TriggeredAbility,
        source,
        owner,
        owner,
        Vec::new(),
        vec![Target::Player(owner)],
        Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Player { controller: None },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        None,
        false,
        false,
        false,
        ScriptHandle(0),
        Some(AbilityEffect::Damage(1)),
        None,
        None,
        None,
    ));
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        
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
fn stifle_counters_a_real_activated_ability_before_it_deals_its_damage() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    // p0 holds priority at game start, so p0's creature activates and p1 responds with Stifle.
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear"), p0));
    state.abilities.push(damage_ability(bear));

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, stifle_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // p0 activates the damage ability targeting p1.
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: bear,
                ability_index: 0,
                targets: vec![Target::Player(p1)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("p0 holds priority, ability is available");
    let status = drain(&mut engine);
    let status = match status {
        StepStatus::AwaitingPayment { .. } => {
            engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
            drain(&mut engine)
        }
        other => other,
    };
    let ability_stack_id = engine.state().stack.last().expect("the ability is on the stack").stack_id;
    assert_eq!(engine.state().stack.len(), 1);

    // CR 117.3c: p0 retains priority after the activation settles and must pass before p1 can respond.
    let StepStatus::AwaitingAction { player: awaiting } = status else {
        panic!("unexpected status after activation: {status:?}")
    };
    if awaiting == p0 {
        engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(&mut engine);
    }

    // p1 responds with Stifle, targeting the ability.
    cast_spell(&mut engine, p1, card, vec![Target::Ability(ability_stack_id)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2, "both the ability and Stifle are on the stack");

    // Stifle resolves first (LIFO). CR 117.3c: p1 retains priority after casting, so p1 passes first.
    let status = pass_both_and_drain(&mut engine, p1, p0);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1 || player == p0));
    assert!(engine.state().stack.is_empty(), "the countered ability never gets its own resolution turn");
    assert_eq!(engine.state().player(p1).unwrap().life, 20, "the ability's damage never happened");
    let countered = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Countered { stack_id } if stack_id == ability_stack_id));
    assert!(countered, "expected a Countered event for the ability's own stack_id");
}

#[test]
fn stifle_counters_a_triggered_ability_with_no_zone_change_either() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear"), p1));

    let ability_stack_id = ObjectId(1);
    push_triggered_ability_on_stack(&mut state, ability_stack_id, bear, p1);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, stifle_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Ability(ability_stack_id)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "the triggered ability's damage never happened");
    // Stifle resolved normally and sits in its owner's graveyard. No card exists for the countered ability.
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1);
    assert!(engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.is_empty());
    let countered = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Countered { stack_id } if stack_id == ability_stack_id));
    assert!(countered);
}

#[test]
fn stifle_rejects_a_spell_as_a_target_at_cast_time() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Stack));
    state.stack.push(StackObject::new(
        target,
        StackEntryKind::Spell,
        target,
        p1,
        p1,
        Vec::new(),
        Vec::new(),
        None,
        None,
        false,
        false,
        false,
        ScriptHandle(0),
        None,
        None,
        None,
        None,
    ));

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, stifle_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // CR 113.9: a spell is never a legal target for "counter target activated or triggered ability".
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(target)]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a spell is not a legal target for \"counter target ability\", got {err:?}");
}

#[test]
fn stifle_has_no_legal_target_when_nothing_is_on_the_stack() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let ability_stack_id = ObjectId(99);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, stifle_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // No ability has this stack_id, so it is rejected like any bogus id.
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Ability(ability_stack_id)]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)), "got {err:?}");
}

#[test]
fn ability_type_target_filter_decodes_correctly() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { type = "ability", count = 1 }
        end
        return s
    "#;
    let spec = runtime.build_target_requirement(source).expect("script should load").expect("declares a target_requirement");
    let TargetSpec::Primitive(req) = spec else { panic!("expected a Primitive requirement") };
    assert!(matches!(req.filter, TargetFilter::Ability), "expected TargetFilter::Ability, got {:?}", req.filter);
}
