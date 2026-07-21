//! Phase 5 vertical slice: CR 702.16's card-type quality ("protection from
//! artifacts"), extending `phase5/protection-from-color`'s three enforcement
//! sites (damage/targeting/blocking) to a second, independent quality axis
//! alongside color - `EffectiveCharacteristics.protection_card_types`,
//! folded in the same layer-6 pass from a sibling `EffectPrimitive::
//! AddProtectionCardType`. Each enforcement site now checks "protected if
//! *either* the color or the card-type quality intersects the source's own
//! characteristics" - this file only re-proves the card-type half; the
//! color half (and the OR-composition mechanics) are already covered by
//! `phase5_protection_from_color.rs`.
//!
//! Same scoping as the color slice: protection from a creature subtype, a
//! specific player, or "from everything" stays unmodeled, and Enchant/Equip
//! (CR 702.16b) is out of scope (no attach/enchant machinery exists).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

const STATIC_PROTECTION_FROM_ARTIFACTS: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionCardType("Artifact"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn creature_definition(
    definition_id: u64,
    name: &str,
    power: i32,
    toughness: i32,
    extra_types: CardTypeSet,
    script: ScriptReference,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE | extra_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: script,
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn vanilla_creature(id: u64, name: &str, power: i32, toughness: i32, extra_types: CardTypeSet) -> CardDefinition {
    creature_definition(id, name, power, toughness, extra_types, ScriptReference::ByKey(name.to_string()))
}

/// "Test Artifact Bolt: deals 2 damage to target creature." An artifact
/// instant, so the targeting tests have a real card type to check
/// protection against.
fn artifact_bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Artifact Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT | CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: "Test Artifact Bolt: deals 2 damage to target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_artifact_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
    }
}

/// A non-artifact counterpart to `artifact_bolt_definition`, for the
/// "protection doesn't block an unrelated card type" control test.
fn plain_bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Plain Bolt".to_string(),
        card_types: CardTypeSet::INSTANT,
        ..artifact_bolt_definition()
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Directly-constructed protection, standing in for the ETB-registered
/// version - the Lua wiring itself is proven separately, same convention
/// every other static-ability-adjacent test file in this repo uses.
fn protection_from_artifacts(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionCardType(CardTypeSet::ARTIFACT)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
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

fn combat_damage_state(combat: CombatState) -> GameState {
    let mut state = base_state();
    state.phase = Phase::Combat;
    state.step = Step::CombatDamage;
    state.combat = Some(combat);
    state
}

fn combat_state() -> GameState {
    let mut state = base_state();
    state.phase = Phase::Combat;
    state.step = Step::DeclareAttackers;
    state
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

// ---------------------------------------------------------------------
// Lua wiring
// ---------------------------------------------------------------------

#[test]
fn game_effects_add_protection_card_type_self_registers_and_applies() {
    let mut state = base_state();
    let bear = ObjectId(0);
    let definition = creature_definition(
        1,
        "Bear",
        2,
        2,
        CardTypeSet::empty(),
        ScriptReference::Inline(STATIC_PROTECTION_FROM_ARTIFACTS.to_string()),
    );
    put_on_battlefield(&mut state, permanent(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(
        state.continuous_effects[0].effect,
        EffectPayload::Primitive(EffectPrimitive::AddProtectionCardType(t)) if t == CardTypeSet::ARTIFACT
    ));

    let effective = effective_characteristics(&state, &runtime, bear);
    assert_eq!(effective.protection_card_types, CardTypeSet::ARTIFACT);
}

// ---------------------------------------------------------------------
// CR 702.16a: damage
// ---------------------------------------------------------------------

#[test]
fn combat_damage_from_a_matching_card_type_is_prevented_by_protection() {
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
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Golem", 3, 3, CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Ward", 0, 5, CardTypeSet::empty()), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_artifacts(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 0, "protection from artifacts prevents the artifact attacker's combat damage");
}

#[test]
fn combat_damage_from_a_non_matching_card_type_still_lands() {
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
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 3, 3, CardTypeSet::empty()), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Ward", 0, 5, CardTypeSet::empty()), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_artifacts(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 3, "protection from artifacts doesn't stop a plain creature's damage");
}

// ---------------------------------------------------------------------
// CR 702.16c: targeting
// ---------------------------------------------------------------------

#[test]
fn casting_a_matching_card_type_spell_at_a_protected_target_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 2, CardTypeSet::empty()), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_artifacts(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, artifact_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "an artifact spell can't target a creature with protection from artifacts, got {status:?}"
    );
}

#[test]
fn casting_a_non_matching_card_type_spell_at_a_protected_target_still_works() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 5, CardTypeSet::empty()), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_artifacts(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, plain_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let p1 = PlayerId(1);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "the non-artifact spell's damage still lands");
}

#[test]
fn spell_fizzles_when_target_gains_protection_before_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 2, CardTypeSet::empty()), p1, ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, artifact_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);

    // CR 702.16c is rechecked by the same CR 608.2b mechanism as any other
    // target legality change - granting protection mid-stack, after the
    // spell already targeted this creature, makes the target illegal by
    // resolution time.
    engine.state_mut().continuous_effects.push(protection_from_artifacts(target));

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: the spell's only target became illegal, so it must fizzle rather than resolve"
    );
    assert_eq!(engine.state().object(target).unwrap().damage_marked, 0, "a fizzled spell deals no damage");
}

// ---------------------------------------------------------------------
// CR 702.16d: blocking
// ---------------------------------------------------------------------

#[test]
fn an_attacker_with_protection_cannot_be_blocked_by_a_matching_card_type_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 2, 2, CardTypeSet::empty()), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Golem Blocker", 2, 2, CardTypeSet::ARTIFACT), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_artifacts(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::Error(_)),
        "an artifact creature can't block an attacker with protection from artifacts, got {status:?}"
    );
}

#[test]
fn an_attacker_with_protection_can_still_be_blocked_by_a_non_matching_card_type_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 2, 2, CardTypeSet::empty()), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Plain Blocker", 2, 2, CardTypeSet::empty()), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_artifacts(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "a non-artifact creature can still block an attacker with only protection from artifacts, got {status:?}"
    );
}

// ---------------------------------------------------------------------
// RemoveAllAbilities consistency (CR 613.1f covers protection_card_types
// too, same as protection_colors - see phase5_lose_all_abilities.rs's own
// analogous test for the color half).
// ---------------------------------------------------------------------

#[test]
fn remove_all_abilities_also_strips_a_granted_protection_card_type() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, vanilla_creature(1, "Bear", 2, 2, CardTypeSet::empty()), p0, ZoneKind::Battlefield));
    state.continuous_effects.push(ContinuousEffect {
        source: bear,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionCardType(CardTypeSet::ARTIFACT)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });
    state.continuous_effects.push(ContinuousEffect {
        source: bear,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::RemoveAllAbilities),
        timestamp: SequenceNo(1),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.protection_card_types, CardTypeSet::empty(), "the earlier-granted protection should be stripped too");
}
