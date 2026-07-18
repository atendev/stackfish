//! Phase 5 vertical slice: "dies"/leaves-battlefield trigger conditions via
//! last-known-information (CR 603.10, backlog item 7's third sub-slice).
//!
//! `commands::retire_object` purges a departing object's own `Triggered`
//! abilities from `state.abilities` synchronously, as part of the very
//! zone-move (`sba::commit_death`) that would produce the "dies"/"leaves the
//! battlefield" event a trigger like this needs to match - by the time
//! `collect_triggers` runs, both the object *and* its ability are already
//! gone from live state. This slice threads the departing object's own
//! `Triggered` abilities forward (via `Engine`'s transient
//! `departed_triggered_abilities` stash) so `ConditionPrimitive::Dies`/
//! `LeavesBattlefield` can still match them against the event's own
//! `ZoneChange::last_known_info`.
//!
//! Scoped to the SBA-driven death path only (`sba::commit_death`) - sacrifice,
//! discard-as-cost, and a resolving spell's other zone departures still
//! discard `retire_object`'s return value this slice (see that function's own
//! doc comment). Only source-identity conditions (Dies/LeavesBattlefield),
//! not "whenever a creature you control dies"-style filtered conditions.

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

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition("Test Creature", 2, toughness),
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked,
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

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: Some(p1), contents: VecDeque::new() },
        ],
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

/// Mirrors `phase5_damage_dealt_triggers::combat_damage_state` - a game
/// sitting mid-combat-damage-step, for the regression test that needs a real
/// `AssignCombatDamage` to produce both a `DamageDealt` and a lethal-damage
/// death in the same pipeline run.
fn combat_damage_state(combat: CombatState) -> GameState {
    let mut state = base_state();
    state.phase = Phase::Combat;
    state.step = Step::CombatDamage;
    state.combat = Some(combat);
    state
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn dies_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Dies)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn leaves_battlefield_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
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

fn would_die_exile_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

// ---------------------------------------------------------------------
// Dies: fires and resolves after a real SBA-driven lethal-damage death.
// ---------------------------------------------------------------------

#[test]
fn dies_trigger_fires_and_resolves_after_real_lethal_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Lethal damage already marked - PassPriority cascades straight into
    // SbaLoop, which finds this creature dying via CR 704.5g.
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.abilities.push(dies_ability(bear));

    let mut engine = Engine::new(state);
    // A bare PassPriority always hands priority to the next player in
    // turn_order (CR 117.3c), regardless of what the SBA/trigger pipeline
    // finds along the way - so priority lands on p1 here even though the
    // Dies trigger (from p0's own creature) was what actually fired.
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "the Dies trigger must have been collected and pushed to the stack: {status:?}"
    );
    assert_eq!(engine.state().stack.len(), 1, "the Dies trigger was collected and pushed");
    assert!(engine.state().object(bear).is_none(), "the bear itself is gone - died to the graveyard");

    // Both players have now passed with the trigger on the stack - it
    // resolves, and priority returns to the active player (CR 117.3b).
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied to the bear's own controller");
}

#[test]
fn ability_on_a_creature_that_does_not_die_never_fires() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Toughness 2, no damage marked - survives, never produces a ZoneChange.
    put_on_battlefield(&mut state, creature(bear, p0, 2, 0));
    state.abilities.push(dies_ability(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));
    assert!(engine.state().stack.is_empty(), "no death occurred, so the Dies trigger never matched");
    assert!(engine.state().object(bear).is_some(), "the bear is still alive");
}

// ---------------------------------------------------------------------
// LeavesBattlefield: fires on a non-death departure (CR 614.13 "exile
// instead" redirect), still through the same wired sba::commit_death path.
// ---------------------------------------------------------------------

#[test]
fn leaves_battlefield_trigger_fires_when_a_replacement_exiles_instead_of_killing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.abilities.push(leaves_battlefield_ability(bear));
    state.replacement_effects.push(would_die_exile_effect(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));
    assert_eq!(engine.state().stack.len(), 1, "LeavesBattlefield must fire on the exile, not just a death");
    assert!(
        engine.state().zone(ZoneKind::Exile, Some(p0)).unwrap().contents.len() == 1,
        "the bear actually went to exile, not the graveyard"
    );

    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied");
}

#[test]
fn dies_does_not_fire_when_a_replacement_exiles_instead_of_killing() {
    // The mirror image of the test above - Dies is specifically
    // battlefield->graveyard, so a redirected-to-exile departure must not
    // satisfy it even though the object did leave the battlefield.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.abilities.push(dies_ability(bear));
    state.replacement_effects.push(would_die_exile_effect(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));
    assert!(engine.state().stack.is_empty(), "Dies must not match a redirected-to-exile departure");
}

// ---------------------------------------------------------------------
// Regression: a live DealsDamage trigger (damage-dealt-triggers slice) and
// a departed-abilities Dies trigger both fire correctly out of the same
// CollectTriggers pass, from real combat.
// ---------------------------------------------------------------------

#[test]
fn deals_damage_and_dies_triggers_both_fire_from_the_same_combat_damage_step() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, creature(attacker, p0, 5, 0));
    put_on_battlefield(&mut state, creature(blocker, p1, 1, 0)); // dies to 3 damage
    state.abilities.push(RegisteredAbility {
        source: attacker,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DealsDamage)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
    state.abilities.push(dies_ability(blocker));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::AssignCombatDamage { assignments: Vec::new() },
        })
        .expect("turn-based action, no priority required");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 2, "both the DealsDamage and Dies triggers were collected");
    assert!(engine.state().object(blocker).is_none(), "the blocker died");

    // Two triggers on the stack - each resolution needs both players to
    // pass in a row (CR 117.3b), and priority returns to the active player
    // after each resolution before the next round starts.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "{status:?}");
    assert!(engine.state().stack.is_empty(), "both triggers resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "DealsDamage: attacker's controller (p0) gained 1 life");
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "Dies: blocker's controller (p1) gained 1 life");
}
