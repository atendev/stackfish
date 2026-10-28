//! CR 115.4: "another target" excludes the source from an otherwise matching target filter, even when it is the only candidate.
//! `TargetRequirement::exclude_source` is checked in `commands::target_is_legal` ahead of every filter variant.
// TODO: exclude_source for `Game.SelectTargets`, whose bare `TargetFilter` carries no flag

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn another_target_creature_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: true,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Damage(1)),
        intervening_if: None,
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        .expect("player holds priority");
    drain(engine)
}

fn try_activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        ?;
    Ok(drain(engine))
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn an_ability_with_another_target_creature_cannot_target_its_own_source() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let olivia = ObjectId(0);
    put_on_battlefield(&mut state, permanent(olivia, creature_definition(1, "Olivia", 3, 3), p0));
    state.abilities.push(another_target_creature_ability(olivia));

    let mut engine = Engine::new(state);
    let err = try_activate(&mut engine, p0, olivia, vec![Target::Object(olivia)]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)), "expected the source to be rejected as its own target, got {err:?}");
}

#[test]
fn an_ability_with_another_target_creature_can_target_a_different_creature() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let olivia = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(olivia, creature_definition(1, "Olivia", 3, 3), p0));
    put_on_battlefield(&mut state, permanent(bear, creature_definition(2, "Bear", 2, 2), p1));
    state.abilities.push(another_target_creature_ability(olivia));

    let mut engine = Engine::new(state);
    let status = activate(&mut engine, p0, olivia, vec![Target::Object(bear)]);
    if matches!(status, StepStatus::AwaitingPayment { .. }) {
        engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
        drain(&mut engine);
    }
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(bear).unwrap().damage_marked, 1, "the legal, different creature took the damage");
    assert_eq!(engine.state().object(olivia).unwrap().damage_marked, 0, "the source itself is untouched");
}

#[test]
fn an_ability_targeting_its_own_source_fizzles_at_resolution_recheck() {
    // Skips cast-time validation to reach the CR 608.2b recheck, where a self-target must fizzle and not deal damage.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let olivia = ObjectId(0);
    put_on_battlefield(&mut state, permanent(olivia, creature_definition(1, "Olivia", 3, 3), p0));
    state.abilities.push(another_target_creature_ability(olivia));

    let stack_id = ObjectId(50);
    state.stack.push(StackObject::new(
        stack_id,
        StackEntryKind::ActivatedAbility,
        olivia,
        p0,
        p0,
        Vec::new(),
        vec![Target::Object(olivia)],
        Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: true,
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

    let mut engine = Engine::new(state);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().stack.is_empty(), "fizzled ability still leaves the stack");
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "the only target was illegal (the source, excluded), so this must fizzle entirely"
    );
    assert_eq!(engine.state().object(olivia).unwrap().damage_marked, 0, "no damage should have been dealt to anything");
}
