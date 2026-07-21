//! Phase 5 vertical slice: `AbilityEffect::GainControl` — `Game.Ops.
//! GainControl(duration)`, the "gain control of target creature until
//! <duration>" shape (CR 108.4/613.1 layer 2, Threaten-style). Closes the
//! gap `phase5/layer-engine-control-consumers`'s own UNRESOLVED note named:
//! every real consumer of `layers::effective_controller` (combat,
//! targeting, cost filters, `has_summoning_sickness`, `FilterSpec::matches`)
//! already reads the *live* controller correctly, but nothing could create a
//! `GainControl` effect - every existing test built one via a bare struct
//! literal. Mirrors `AbilityEffect::Pump`'s own per-target `ContinuousEffect`
//! construction exactly, just layer 2/`GainControl` instead of layer 7c/
//! `ModifyPT`.
//!
//! Same harness as `phase5_pump_effect.rs`: `CardDefinition.effect` set
//! directly exercises the Rust-native fast path `ResolveStackTop` already
//! takes for a recognized primitive, without needing the actual `Game.Ops.
//! GainControl` Lua binding invoked (same precedent every other
//! `AbilityEffect` primitive's own test file already follows).
//!
//! Deliberately narrow, same as the primitive's own doc comment: only this
//! resolving-spell/ability creation path exists. A static ability's own
//! "you control enchanted creature" (Control Magic-style aura) needs real
//! attach/enchant machinery this engine doesn't have yet - a separate,
//! larger gap, not attempted here.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_controller;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn creature_definition(definition_id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id,
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

/// "Test Threaten: Gain control of target creature until end of turn."
fn threaten_definition(duration: EffectDuration) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Threaten".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Test Threaten: Gain control of target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_threaten".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::GainControl(duration)),
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
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

fn two_player_state() -> GameState {
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
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
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

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
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

/// Passes priority for whichever player the engine is waiting on until the
/// active player rotates - i.e. until Cleanup has actually run, regardless
/// of exactly how many steps (combat, end step, ...) sit in between.
fn advance_to_next_turn(engine: &mut Engine) {
    let starting_turn = engine.state().turn_number;
    loop {
        match drain(engine) {
            StepStatus::AwaitingAction { player } => {
                engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
            }
            other => panic!("unexpected pause while advancing turns: {other:?}"),
        }
        if engine.state().turn_number != starting_turn {
            break;
        }
    }
}

fn pass_to_declare_attackers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareAttackers {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached DeclareAttackers - state.step = {:?}", engine.state().step);
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

#[test]
fn threaten_spell_makes_the_caster_the_real_effective_controller() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, threaten_definition(EffectDuration::UntilEndOfTurn), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(effective_controller(engine.state(), &ScriptRuntime::new(), target), p0, "p0 (the caster) is now the real controller");
    assert_eq!(engine.state().continuous_effects.len(), 1);
    assert!(matches!(engine.state().continuous_effects[0].duration, EffectDuration::UntilEndOfTurn));

    // Concrete proof, not just the query: p0 can now declare p1's original
    // creature as an attacker - real combat legality, same shape
    // phase5_control_change_consumers.rs's own end-to-end test uses for a
    // hand-built ContinuousEffect.
    pass_to_declare_attackers(&mut engine);
    let status = declare_attackers(&mut engine, p0, vec![(target, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "the new controller should legally declare it as an attacker, got {status:?}");
}

#[test]
fn gain_control_effect_is_pinned_to_its_target_not_the_spells_own_source() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let bystander = ObjectId(1);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.objects.insert(bystander, card_object(bystander, creature_definition(2, "Bystander", 1, 1), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bystander);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, threaten_definition(EffectDuration::UntilEndOfTurn), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), bystander),
        p1,
        "the untargeted creature's controller is untouched"
    );
}

#[test]
fn until_end_of_turn_gain_control_reverts_by_the_start_of_the_next_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, threaten_definition(EffectDuration::UntilEndOfTurn), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(effective_controller(engine.state(), &ScriptRuntime::new(), target), p0, "sanity: control was gained");

    advance_to_next_turn(&mut engine);

    assert_eq!(engine.state().continuous_effects.len(), 0, "UntilEndOfTurn drains at cleanup");
    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), target),
        p1,
        "control reverts to the original owner once the effect is gone"
    );
}

#[test]
fn permanent_gain_control_survives_into_the_next_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, threaten_definition(EffectDuration::Permanent), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    advance_to_next_turn(&mut engine);

    assert_eq!(engine.state().continuous_effects.len(), 1, "Permanent duration is unaffected by cleanup");
    assert_eq!(effective_controller(engine.state(), &ScriptRuntime::new(), target), p0, "control is still held after the turn passes");
}
