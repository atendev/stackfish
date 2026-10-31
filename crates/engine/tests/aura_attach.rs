//! CR 303.4: Aura attach machinery. `commands::attach_permanent` attaches on resolution (CR 303.4a/b).
//! `sba::collect_illegally_attached_auras` drops an Aura whose target is gone or no longer passes its filter (CR 704.5m, 303.4c).
//! The CR 303.4d self-enchant check is reachable only by hand-built fixtures.
// TODO: non-spell attachment entry and "enchanted X" references (CR 303.4f/h/i/k/m)
// TODO: enchant-subtype restrictions such as "Enchant Human", which need a subtype target filter

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{SbaOutcome, check_state_based_actions};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

fn creature_definition(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
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

fn aura_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Aura".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: Some(ColorSet::GREEN),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Test Aura: enchant creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_aura".to_string()),
        keywords: KeywordSet::empty(),
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
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn plain_enchantment_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 31,
        name: "Test Plain Enchantment".to_string(),
        subtypes: Vec::new(),
        ..aura_definition()
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, obj: GameObject) {
    let id = obj.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, obj);
}

fn protection_from_green(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionColor(ColorSet::GREEN)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

fn find_battlefield_object_named<'a>(state: &'a GameState, name: &str) -> Option<&'a GameObject> {
    state
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Battlefield && o.definition.name == name)
}

fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
        SbaOutcome::NeedsLegendRuleChoice { .. } => {
            panic!("fixture didn't expect a legend-rule choice pause")
        }
        SbaOutcome::NeedsCommanderReturnChoice { .. } => {
            panic!("fixture didn't expect a commander-return choice pause")
        }
    }
}

// CR 303.4a/b
#[test]
fn casting_an_aura_at_a_legal_creature_attaches_it_on_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(
        &mut state,
        permanent(bear, creature_definition(1, "Bear", 2, 2), p1),
    );
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, aura_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let aura = find_battlefield_object_named(engine.state(), "Test Aura")
        .expect("the Aura should have resolved onto the battlefield");
    assert_eq!(
        aura.attached_to,
        Some(bear),
        "the Aura should be attached to its target on resolution"
    );
    let bear_now = engine
        .state()
        .object(bear)
        .expect("the target is still on the battlefield");
    assert!(
        bear_now.attachments.contains(&aura.object_id),
        "the target's own attachments list should include the Aura"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Attached { object, attached_to } if object == aura.object_id && attached_to == bear)),
        "an Attached event should be logged"
    );
}

#[test]
fn casting_an_aura_at_a_target_that_becomes_illegal_before_resolution_fizzles_and_never_attaches() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(
        &mut state,
        permanent(bear, creature_definition(1, "Bear", 2, 2), p1),
    );
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, aura_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);

    // CR 608.2b: protection from the Aura's own color granted mid-stack makes the target illegal by resolution.
    engine
        .state_mut()
        .continuous_effects
        .push(protection_from_green(bear));

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: the Aura's only target became illegal, so it must fizzle rather than resolve"
    );
    assert!(
        find_battlefield_object_named(engine.state(), "Test Aura").is_none(),
        "a fizzled Aura never enters the battlefield, so there's nothing to attach"
    );
    assert!(
        engine.state().object(bear).unwrap().attachments.is_empty(),
        "the target's attachments list stays empty"
    );
}

// CR 704.5m/303.4c
#[test]
fn an_aura_not_attached_to_anything_dies_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(0);
    put_on_battlefield(&mut state, permanent(aura, aura_definition(), p0));
    // attached_to stays None, as permanent() defaults it.

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura),
        "an unattached Aura should die (CR 704.5m), minting a new graveyard-side id, got {:?}",
        events[0].kind
    );
}

#[test]
fn an_auras_target_dying_makes_it_fall_off_on_the_next_sba_pass_not_this_one() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.damage_marked = 2; // lethal
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    // Pass 1: the creature dies (CR 704.5g), but the Aura is checked against the pass's starting state (CR 704.3), so it waits one pass.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(
        events.len(),
        1,
        "only the creature's death this pass, got {:?}",
        events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    assert!(matches!(
        events[0].kind,
        GameEventKind::ZoneChange {
            to: ZoneKind::Graveyard,
            ..
        }
    ));
    assert!(
        find_battlefield_object_named(&state, "Test Aura").is_some(),
        "the Aura is still on the battlefield after pass 1"
    );

    // Pass 2: the creature id no longer resolves, so the Aura is attached to an illegal target (CR 704.5m).
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura)
    );
    assert!(
        find_battlefield_object_named(&state, "Test Aura").is_none(),
        "the Aura has now fallen off"
    );
}

#[test]
fn a_healthy_creature_with_an_attached_aura_is_unaffected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(
        events.is_empty(),
        "nothing should happen to a healthy, legally-attached Aura, got {:?}",
        events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
}

#[test]
fn a_non_aura_enchantment_is_never_touched_by_the_attach_check() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let relic = ObjectId(0);
    put_on_battlefield(
        &mut state,
        permanent(relic, plain_enchantment_definition(), p0),
    );
    // Not an Aura (no Aura subtype), so CR 704.5m never applies.

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(
        events.is_empty(),
        "a plain enchantment with no attachment concept must never be swept up by this check"
    );
}

// Protection from the Aura's own color gained after attaching drops the Aura (CR 702.16) and removes its id from the target's attachments.
#[test]
fn an_auras_target_gaining_protection_from_it_after_attaching_makes_it_fall_off() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    // The creature gains protection from green after the Aura attached (CR 702.16).
    state.continuous_effects.push(protection_from_green(bear));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura)
    );
    assert!(
        find_battlefield_object_named(&state, "Test Aura").is_none(),
        "the Aura should have fallen off"
    );
    assert!(
        !state.object(bear).unwrap().attachments.contains(&aura),
        "retire_object should have cleaned the dead Aura's id out of the target's own attachments list"
    );
}

// CR 303.4d
#[test]
fn a_self_enchanted_aura_dies_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(0);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    // Hand-built, since no attach call site produces this; CR 303.4d states the SBA unconditionally.
    aura_obj.attached_to = Some(aura);
    put_on_battlefield(&mut state, aura_obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura),
        "a self-enchanted Aura should die (CR 303.4d), got {:?}",
        events[0].kind
    );
}

#[test]
fn an_aura_that_becomes_a_creature_unattaches_then_dies_in_the_same_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    // A layer 4 AddCardType effect makes the Aura a creature (CR 303.4d).
    state.continuous_effects.push(ContinuousEffect {
        source: aura,
        layer: Layer::TypeChanging,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddCardType(CardTypeSet::CREATURE)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(
        events.iter().any(|e| matches!(e.kind, GameEventKind::Unattached { object, was_attached_to } if object == aura && was_attached_to == bear)),
        "the Aura-creature should become unattached this same pass, got {:?}",
        events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|e| matches!(e.kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura)),
        "the Aura-creature should also die this same pass (CR 303.4d: 'becomes unattached, then is put into its owner's graveyard'), got {:?}",
        events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    assert!(
        !state.object(bear).unwrap().attachments.contains(&aura),
        "the target's own attachments list should no longer include the dead Aura"
    );
}

// CR 303.4e
#[test]
fn only_the_auras_controller_activates_its_own_ability_not_the_enchanted_creatures_controller() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p1);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: aura,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(
            Color::Green,
        )])),
        intervening_if: None,
    });

    // p1 controls the enchanted creature, which is irrelevant to who may activate the Aura's ability (CR 303.4e).
    state.priority_player = p1;
    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::ActivateAbility {
                source: aura,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap_err();
    assert!(
        matches!(
            err,
            stackfish_engine::engine::ActionError::Rejected(
                stackfish_engine::commands::Rejected::IllegalAction(_)
            )
        ),
        "the enchanted creature's controller shouldn't be able to activate the Aura's own ability"
    );

    // p0, the Aura's actual controller, can.
    engine.state_mut().priority_player = p0;
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: aura,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap();
    assert!(
        !matches!(drain(&mut engine), StepStatus::Error(_)),
        "the Aura's own controller should be able to activate its own ability"
    );
}
