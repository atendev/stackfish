//! Phase 5 vertical slice: wires the CR 613 layer engine (built in
//! `phase5/layer-engine-basics`, only real consumer was `sba.rs`'s
//! lethal-toughness check) into the two consumers flagged as follow-up
//! there — combat-damage power (`AssignCombatDamage`) and Flying/Reach/
//! Menace blocking legality (`DeclareBlockers`). Both already had a
//! `&GameState` to read, no `&ScriptRuntime` needed for the primitive-only
//! path — the prior slice's TODO comments overstated a blocker that didn't
//! actually apply here (see docs/implementation.md).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
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

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, controlled_since_turn: u32) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
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
        controlled_since_turn,
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

fn player_state(id: PlayerId) -> PlayerState {
    PlayerState {
        player_id: id,
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
    }
}

/// Two players, `p0` active/attacking, `p1` defending, `Phase::Combat`/
/// `Step::DeclareAttackers` - same shape as `phase5_keyword_primitives.rs`'s
/// `combat_state`.
fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        objects: HashMap::new(),
        zones: vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
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

/// Same as `combat_state`, with `combat` already populated as if
/// `DeclareAttackers`/`DeclareBlockers` ran, sitting at `Step::CombatDamage`.
fn combat_damage_state(combat: CombatState) -> GameState {
    let mut state = combat_state();
    state.step = Step::CombatDamage;
    state.combat = Some(combat);
    state
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

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached DeclareBlockers - state.step = {:?}", engine.state().step);
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

fn no_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, self_only: false }
}

// ---------------------------------------------------------------------
// Combat-damage power (AssignCombatDamage)
// ---------------------------------------------------------------------

#[test]
fn a_modify_pt_boost_increases_combat_damage_dealt() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state =
        combat_damage_state(CombatState { attackers: vec![(attacker, Target::Player(p1))], blocks: Vec::new(), declared_defenders: vec![p1] });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0, 0));
    state.continuous_effects.push(ContinuousEffect {
        source: attacker,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter { self_only: true, ..no_filter() }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 2, toughness: 2 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
    });

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 15, "boosted 5 power (3 base + 2 from the continuous effect) should hit, not the printed 3");
}

#[test]
fn without_the_boost_the_same_attacker_only_deals_its_printed_power() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state =
        combat_damage_state(CombatState { attackers: vec![(attacker, Target::Player(p1))], blocks: Vec::new(), declared_defenders: vec![p1] });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 17);
}

// ---------------------------------------------------------------------
// Blocking legality (DeclareBlockers) - Flying granted, not printed
// ---------------------------------------------------------------------

#[test]
fn granted_flying_via_continuous_effect_still_enforces_blocking_legality() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Drake", 2, 2), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 2), p1, 0));
    // No printed Flying on "Drake" this time - granted by a continuous
    // effect instead (e.g. an aura's "enchanted creature has flying").
    state.continuous_effects.push(ContinuousEffect {
        source: attacker,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { self_only: true, ..no_filter() }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddKeyword(KeywordSet::FLYING)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
    });

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))),
        "a continuous-effect-granted Flying should block just as legally as printed Flying, got {status:?}"
    );
}

#[test]
fn without_the_grant_the_same_attacker_is_blockable_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Drake", 2, 2), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 2), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "no Flying granted - a vanilla bear should block legally, got {status:?}");
}
