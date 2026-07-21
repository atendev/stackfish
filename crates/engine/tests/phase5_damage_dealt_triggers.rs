//! Phase 5 vertical slice: damage-dealt trigger conditions (CR 603.2, first
//! sub-slice of backlog item 7, "trigger system completeness"). Adds
//! `ConditionPrimitive::DealsDamage`/`IsDealtDamage`, matched against a real
//! `GameEventKind::DamageDealt` event - reachable without any pipeline
//! changes because combat damage (`AssignCombatDamage`) already routes its
//! `DamageDealt` events through the real `CommitEvents`->`SbaLoop`->
//! `CollectTriggers` pipeline (see the `fix-replacement-bypasses` slice).
//!
//! Deliberately NOT covered here (see docs/implementation.md's backlog):
//! phase/step-boundary conditions ("beginning of upkeep") - `AdvanceTurnStep`
//! bypasses `CollectTriggers` entirely via `log_events`, a separate,
//! undocumented-until-now gap; leaves-battlefield/dies conditions - blocked
//! on the pre-existing "source no longer resolves" gap in `collect_triggers`;
//! intervening-if legality (CR 603.4). CR 603.2h's "once each turn" cap is
//! covered too now, but in its own file - see
//! phase5_trigger_once_per_turn.rs.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
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
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: true,
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
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Mirrors `phase5_combat_damage::combat_damage_state`.
fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
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
                commander_casts: std::collections::HashMap::new(),
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::CombatDamage,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        objects: HashMap::new(),
        zones: vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
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

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

// ---------------------------------------------------------------------
// DealsDamage: the attacker's own "whenever this deals damage" ability.
// ---------------------------------------------------------------------

#[test]
fn deals_damage_trigger_fires_when_the_attacker_connects() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));
    state.abilities.push(RegisteredAbility { id: AbilityId(0),
        source: attacker,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DealsDamage)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "the DealsDamage trigger was collected and pushed");

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied");
}

#[test]
fn deals_damage_trigger_does_not_fire_for_a_creature_that_never_dealt_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let bystander = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));
    // Registered on a bystander that isn't attacking/blocking at all -
    // never a `source` of any DamageDealt event this combat.
    put_on_battlefield(&mut state, permanent(bystander, creature_definition("Sprout", 0, 1), p0));
    state.abilities.push(RegisteredAbility { id: AbilityId(1),
        source: bystander,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DealsDamage)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the bystander's ability never matched a DamageDealt event");
}

// ---------------------------------------------------------------------
// IsDealtDamage: the blocker's own "whenever this is dealt damage" ability.
// ---------------------------------------------------------------------

#[test]
fn is_dealt_damage_trigger_fires_on_the_creature_taking_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 5), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 5), p1));
    state.abilities.push(RegisteredAbility { id: AbilityId(2),
        source: blocker,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::IsDealtDamage)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    assert_eq!(engine.state().stack.len(), 1, "the IsDealtDamage trigger was collected and pushed");

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);

    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "blocker's controller (p1) gained 1 life");
}
