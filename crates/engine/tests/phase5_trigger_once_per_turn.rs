//! Phase 5: CR 603.2h ("This ability triggers only once each turn"),
//! backlog item 7's once-per-turn sub-piece — `ConditionSpec::
//! PrimitiveOncePerTurn`/`Game.Conditions.OncePerTurn(tag)`, enforced in
//! `triggers::collect_triggers` against `GameState::once_per_turn_triggered`.
//!
//! The subtle case this slice actually has to get right: two qualifying
//! events in the very same batch (e.g. an attacker splitting combat damage
//! across two blockers, CR 510.1c - one `AssignCombatDamage` command, one
//! `CollectTriggers` pass, but two `DamageDealt` events). A naive "check
//! once per `collect_triggers` call" implementation would still double-
//! trigger here; the fix records a match into `GameState::
//! once_per_turn_triggered` as soon as one event in the batch produces it,
//! before the next event in that same batch is checked (see `Engine`'s
//! `CollectTriggers` arm).

use std::collections::{HashMap, HashSet, VecDeque};

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

/// Mirrors `phase5_damage_dealt_triggers::combat_damage_state`.
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
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
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

/// Drives real priority passes (whoever `AwaitingAction` names next) through
/// however many steps it takes for `turn_number` to roll over - the actual
/// turn structure (Combat -> End -> Cleanup -> next Untap/Upkeep), not a
/// shortcut. A safety cap keeps a genuine engine bug (stuck priority) a
/// clear test failure instead of a hang.
fn pass_until_turn_advances(engine: &mut Engine, starting_turn: u32) -> StepStatus {
    let mut status = drain(engine);
    for _ in 0..64 {
        if engine.state().turn_number != starting_turn {
            break;
        }
        let StepStatus::AwaitingAction { player } = status else {
            break;
        };
        pass(engine, player);
        status = drain(engine);
    }
    status
}

fn dealsdamage_ability(source: ObjectId, once_per_turn: bool) -> RegisteredAbility {
    RegisteredAbility {
        source,
        kind: AbilityKind::Triggered,
        condition: Some(if once_per_turn {
            ConditionSpec::PrimitiveOncePerTurn(ConditionPrimitive::DealsDamage)
        } else {
            ConditionSpec::Primitive(ConditionPrimitive::DealsDamage)
        }),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

/// CR 510.1c: an attacker with 2+ blockers splits its combat damage - one
/// `AssignCombatDamage` command, one `CollectTriggers` pass, but two
/// `DamageDealt` events (source = the attacker, both in the same batch).
#[test]
fn once_per_turn_dealsdamage_trigger_fires_only_once_from_two_events_in_the_same_batch() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker1 = ObjectId(1);
    let blocker2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker1, attacker), (blocker2, attacker)],
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Ogre", 3, 3), p0));
    put_on_battlefield(&mut state, permanent(blocker1, creature_definition("Wall", 0, 5), p1));
    put_on_battlefield(&mut state, permanent(blocker2, creature_definition("Wall", 0, 5), p1));
    state.abilities.push(dealsdamage_ability(attacker, true));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker1, 2), (blocker2, 1)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    // Both DamageDealt events really happened (not a setup mistake)...
    let damage_events: Vec<u32> = engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::DamageDealt { source, amount, .. } if source == attacker => Some(amount),
            _ => None,
        })
        .collect();
    assert_eq!(damage_events, vec![2, 1], "the attacker's split damage produced two DamageDealt events");

    // ...but only one trigger was placed for them, not two.
    assert_eq!(engine.state().stack.len(), 1, "once-per-turn capped the trigger to a single instance");

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied exactly once");
}

/// Same setup, but without the once-per-turn cap - the same two events
/// produce two separate triggers, confirming the capped test above isn't
/// passing for some unrelated reason (e.g. only one DamageDealt event ever
/// being produced).
#[test]
fn without_the_cap_the_same_two_events_fire_the_trigger_twice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker1 = ObjectId(1);
    let blocker2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker1, attacker), (blocker2, attacker)],
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Ogre", 3, 3), p0));
    put_on_battlefield(&mut state, permanent(blocker1, creature_definition("Wall", 0, 5), p1));
    put_on_battlefield(&mut state, permanent(blocker2, creature_definition("Wall", 0, 5), p1));
    state.abilities.push(dealsdamage_ability(attacker, false));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker1, 2), (blocker2, 1)])]);

    // Both triggers share a controller (p0), so CR 603.3b pauses for p0 to
    // choose their relative stack order rather than pushing both outright -
    // the group size itself is the proof two separate triggers matched.
    match status {
        StepStatus::AwaitingTriggerOrderChoice { player, group } => {
            assert_eq!(player, p0);
            assert_eq!(group.len(), 2, "uncapped: both DamageDealt events triggered their own instance");
        }
        other => panic!("expected AwaitingTriggerOrderChoice for two same-controller triggers, got {other:?}"),
    }
}

/// CR 603.2h's cap resets with the turn - a permanent that already
/// triggered once this turn can trigger again next turn.
#[test]
fn once_per_turn_trigger_resets_after_a_new_turn_begins() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker1 = ObjectId(1);
    let blocker2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker1, attacker), (blocker2, attacker)],
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Ogre", 3, 3), p0));
    put_on_battlefield(&mut state, permanent(blocker1, creature_definition("Wall", 0, 5), p1));
    put_on_battlefield(&mut state, permanent(blocker2, creature_definition("Wall", 0, 5), p1));
    state.abilities.push(dealsdamage_ability(attacker, true));
    // Simulate "already triggered earlier this same turn" directly, rather
    // than re-deriving it via a first combat - this test only cares whether
    // a fresh turn clears the record.
    state.once_per_turn_triggered.insert((attacker, 0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker1, 2), (blocker2, 1)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "already-triggered-this-turn correctly suppressed it");

    // Walk the real turn structure (Combat -> End -> Cleanup -> next
    // Untap/Upkeep) until the turn actually rolls over.
    let status = pass_until_turn_advances(&mut engine, 1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "turn rotated, got {status:?}");
    assert_eq!(engine.state().turn_number, 2);
    assert!(!engine.state().once_per_turn_triggered.contains(&(attacker, 0)), "the cap reset with the new turn");
}
