//! `layers::effective_controller` drives real gameplay consumers: each migrated site honors a control-changed permanent's current controller and not its owner.
//! Hand, graveyard, command and library controller checks, and `ContinuousEffect::applies_to`, are not migrated.

use std::collections::HashMap;

use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
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

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, controlled_since_turn: u32) -> GameObject {
    GameObject { controlled_since_turn, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn gain_control(source: ObjectId, new_controller: PlayerId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::ControlChangingEffects,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}


fn control_changed_combat_state(p0_last_active_turn: u32, creature_controlled_since_turn: u32) -> (GameState, PlayerId, PlayerId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let mut state = GameState {
        // p1 is a veteran (last active turn 10), so the sickness check keying off p0, the effective controller, gives a different answer than p1, the owner.
        turn_number: 10,
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, p0_last_active_turn), (p1, 10)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    put_on_battlefield(&mut state, permanent(creature, creature_definition("Bear"), p1, creature_controlled_since_turn));
    state.continuous_effects.push(gain_control(creature, p0));
    (state, p0, p1, creature)
}

#[test]
fn only_the_new_controller_can_declare_a_gain_controlled_creature_as_an_attacker() {
    let (state, p0, _p1, creature) = control_changed_combat_state(1, 0);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(1)))] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "p0 (the GainControl'd effective controller) should be allowed to attack with it, got {status:?}"
    );
    assert!(engine.state().object(creature).unwrap().tapped, "attacking taps it");
}

#[test]
fn the_original_owner_can_no_longer_attack_with_a_gain_controlled_creature() {
    let (state, _p0, p1, creature) = control_changed_combat_state(1, 0);
    let mut state = state;
    // p1 is not the active player, so active_player is flipped to p1 to isolate control from turn order.
    state.active_player = p1;
    state.priority_player = p1;
    let mut engine = Engine::new(state);

    let result = engine.submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(0)))] },
        });
    assert!(
        matches!(result, Err(ActionError::Rejected(_))),
        "p1 (the original owner, no longer the effective controller) must not be able to attack with it"
    );
    assert!(!engine.state().object(creature).unwrap().tapped, "rejected declaration must not tap it");
}

#[test]
fn only_the_new_controller_can_declare_it_as_a_blocker() {
    let (mut state, p0, p1, creature) = control_changed_combat_state(1, 0);
    // An unrelated p0 creature attacks so p1 has something to block with. Blocking legality is not under test.
    let attacker = ObjectId(1);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Attacker"), p0, 0));
    state.combat = Some(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    state.step = Step::DeclareBlockers;
    let mut engine = Engine::new(state);

    // GainControl gave the creature to p0, so p1 cannot block with it even though p1 owns it.
    let result = engine.submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::DeclareBlockers { blocks: vec![(creature, attacker)] },
        });
    assert!(
        matches!(result, Err(ActionError::Rejected(_))),
        "p1 no longer effectively controls the creature and must not be able to block with it"
    );
}

// controlled_since_turn 5 is after p0's turn began (1) and before p1's (10): sick under the effective controller (5 >= 1), not under the raw owner (5 >= 10 is false).
#[test]
fn summoning_sickness_is_checked_against_the_new_controllers_last_active_turn() {
    let (state, p0, _p1, creature) = control_changed_combat_state(1, 5);
    let mut engine = Engine::new(state);

    let result = engine.submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(1)))] },
        });
    assert!(
        matches!(result, Err(ActionError::Rejected(_))),
        "the creature should read as summoning sick under its new controller p0 (5 >= p0's own \
         last active turn 1), got {result:?}"
    );
}

#[test]
fn summoning_sickness_is_bypassed_when_controlled_before_the_new_controllers_own_turn() {
    // Controlled since turn 0, before p0's most recent turn (1) began, so it is not sick and may attack.
    let (state, p0, _p1, creature) = control_changed_combat_state(1, 0);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(1)))] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
}

fn control_changed_activation_state() -> (GameState, PlayerId, PlayerId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(CostSpec { mana: None, additional: Vec::new(), alternative: Vec::new() }),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    put_on_battlefield(&mut state, permanent(source, creature_definition("Source"), p1, 0));
    state.continuous_effects.push(gain_control(source, p0));
    (state, p0, p1, source)
}

#[test]
fn the_new_controller_can_activate_a_gain_controlled_sources_ability() {
    let (state, p0, _p1, source) = control_changed_activation_state();
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("p0 (the effective controller) should be able to activate it");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }) || matches!(status, StepStatus::AwaitingPayment { .. }));
}

#[test]
fn the_original_owner_can_no_longer_activate_a_gain_controlled_sources_ability() {
    let (state, _p0, p1, source) = control_changed_activation_state();
    let mut engine = Engine::new(state);

    let result = engine.submit_action(PlayerAction {
        player: p1,
        kind: PlayerActionKind::ActivateAbility {
            source,
            ability_index: 0,
            targets: Vec::new(),
            chosen_colors: Vec::new(),
            x_value: None,
        },
    });
    match result {
        Ok(_) => {
            let status = drain(&mut engine);
            assert!(
                matches!(status, StepStatus::Error(_)),
                "p1 no longer effectively controls the source and must not be able to activate it, got {status:?}"
            );
        }
        Err(_) => {} // rejected up front - also an acceptable "not allowed" outcome
    }
}

#[test]
fn sacrifice_cost_accepts_a_gain_controlled_creature_from_its_new_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let fodder = ObjectId(1);
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec {
                card_types: Some(CardTypeSet::CREATURE),
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
        }],
        alternative: Vec::new(),
    };
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(1),
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(cost),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    // The ability's source is p0's ordinary permanent, and only fodder (owned by p1, GainControl'd to p0) is under test.
    put_on_battlefield(&mut state, permanent(source, creature_definition("Source"), p0, 0));
    put_on_battlefield(&mut state, permanent(fodder, creature_definition("Fodder"), p1, 0));
    state.continuous_effects.push(gain_control(fodder, p0));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .expect(
            "p0 (fodder's effective controller via GainControl, even though p1 owns it) should be \
             able to sacrifice it to pay a 'sacrifice a creature you control' cost",
        );
}
