//! Phase 5 vertical slice: CR 115.4's "another target" (e.g. Olivia
//! Voldaren's "{1}{R}: Olivia Voldaren deals 1 damage to another target
//! creature") - a spell/ability's own source is excluded from an otherwise-
//! matching target filter. A separate mechanism from CR 601.2c/115.3's
//! same-instance dedup (already enforced unconditionally in
//! `commands::validate_targets`, untouched by this slice) - that rejects
//! choosing the *same target twice*; this excludes one specific object (the
//! source) from ever being a legal candidate at all, even alone.
//!
//! `TargetRequirement::exclude_source: bool`, checked once in
//! `commands::target_is_legal` ahead of every filter variant. Threaded
//! through every `target_is_legal` call site (cast/activate-time validation,
//! the CR 608.2b resolution recheck, Aura/Equipment SBA rechecks) except
//! `Game.SelectTargets`'s mid-resolution script choice, which carries a bare
//! `TargetFilter` with no `TargetRequirement` to read a flag from - named
//! TODO, not silently narrowed.
//!
//! Found while implementing: the CR 608.2b resolution recheck
//! (`engine.rs`'s `ResolveStackTop`) was passing `top.stack_id` as
//! `target_is_legal`'s `source` parameter instead of `top.source` - harmless
//! for a spell (where the two coincide) but wrong for an activated/
//! triggered ability, where `stack_id` is a bare id never inserted into
//! `state.objects` at all. That silently broke protection-from-source-
//! quality rechecks for every ability (an ever-false `state.object(source)`
//! lookup), and would have broken this slice's own exclude_source the same
//! way. Fixed alongside this feature, not filed separately - the recheck
//! call site is exactly what needed touching. See
//! `an_ability_targeting_its_own_source_fizzles_at_resolution_recheck`
//! below for the regression.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

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
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// "another target creature" - excludes whatever ability source this ends
/// up registered on.
fn another_target_creature_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility { id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: true,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Damage(1)),
        intervening_if: None,
    }
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

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
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
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
    drain(engine)
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
    let status = activate(&mut engine, p0, olivia, vec![Target::Object(olivia)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::InvalidTarget)), "expected the source to be rejected as its own target, got {status:?}");
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
    // Bypasses ActivateAbility's own cast-time validation (already proven
    // illegal above) to exercise the CR 608.2b resolution recheck directly -
    // this is also the regression for the top.source/top.stack_id mixup
    // named in this file's own header: with the bug, target_is_legal's
    // exclude_source check compared against the wrong id and never caught
    // this, so the illegal self-target would have incorrectly gone on to
    // deal damage instead of fizzling.
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
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: true,
        })),
        None,
        false,
        ScriptHandle(0),
        Some(AbilityEffect::Damage(1)),
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
