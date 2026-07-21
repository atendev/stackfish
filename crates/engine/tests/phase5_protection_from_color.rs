//! Phase 5 vertical slice: CR 702.16 protection, narrowed to "protection
//! from color" (702.16e's shape - by far the most common real form) and
//! three of the four "DEBT" restrictions - damage (702.16a), targeting
//! (702.16c), and blocking (702.16d). Enchant/Equip (702.16b) stays
//! unmodeled - no attach/enchant machinery exists in this engine yet.
//!
//! Design finding: protection isn't a `state.replacement_effects` entry a
//! card registers - it's a static, always-on characteristic of the object
//! itself (`EffectiveCharacteristics.protection_colors`, a new layer-6
//! primitive alongside `AddKeyword`). Enforcement is three hardcoded checks
//! at the sites that already gate the equivalent real-gameplay actions:
//! `replacement::apply_protection_from_color` (ahead of the registered-
//! effects match, same "checked up front" precedent CR 702.12b
//! indestructible already set for `Destroy`), `commands::target_is_legal`
//! (now threading the spell/ability's own source id through, same
//! "pre-resolve and pass in" shape `FilterSpec::matches` already took), and
//! `DeclareBlockers`'s existing Flying/Reach/Menace check block.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object as permanent, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const STATIC_PROTECTION_FROM_RED: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionColor("R"))
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
    color_indicator: Option<ColorSet>,
    script: ScriptReference,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
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

fn vanilla_creature(id: u64, name: &str, power: i32, toughness: i32, color: Option<ColorSet>) -> CardDefinition {
    creature_definition(id, name, power, toughness, color, ScriptReference::ByKey(name.to_string()))
}

/// "Test Bolt: deals 2 damage to target creature." Red, so the targeting
/// tests have a real color to check protection against.
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: Some(ColorSet::RED),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Bolt: deals 2 damage to target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
    }
}

/// A colorless counterpart to `bolt_definition`, for the "protection
/// doesn't block an unrelated color" control test.
fn colorless_bolt_definition() -> CardDefinition {
    CardDefinition { definition_id: 21, name: "Test Colorless Bolt".to_string(), color_indicator: None, ..bolt_definition() }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Directly-constructed protection, standing in for the ETB-registered
/// version - the Lua wiring itself is proven separately, same convention
/// every other static-ability-adjacent test file in this repo uses.
fn protection_from_red(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionColor(ColorSet::RED)),
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
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
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
fn game_effects_add_protection_color_self_registers_and_applies() {
    let mut state = base_state();
    let bear = ObjectId(0);
    let definition = creature_definition(1, "Bear", 2, 2, None, ScriptReference::Inline(STATIC_PROTECTION_FROM_RED.to_string()));
    put_on_battlefield(&mut state, permanent(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(
        state.continuous_effects[0].effect,
        EffectPayload::Primitive(EffectPrimitive::AddProtectionColor(c)) if c == ColorSet::RED
    ));

    let effective = effective_characteristics(&state, &runtime, bear);
    assert_eq!(effective.protection_colors, ColorSet::RED);
}

// ---------------------------------------------------------------------
// CR 702.16a: damage
// ---------------------------------------------------------------------

#[test]
fn combat_damage_from_a_matching_color_is_prevented_by_protection() {
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
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 3, 3, Some(ColorSet::RED)), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Ward", 0, 5, None), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_red(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 0, "protection from red prevents the red attacker's combat damage");
}

#[test]
fn combat_damage_from_a_different_color_still_lands() {
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
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 3, 3, Some(ColorSet::GREEN)), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Ward", 0, 5, None), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_red(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 3, "protection from red doesn't stop a green attacker's damage");
}

// ---------------------------------------------------------------------
// CR 702.16c: targeting
// ---------------------------------------------------------------------

#[test]
fn casting_a_matching_color_spell_at_a_protected_target_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 2, None), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_red(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "a red spell can't target a creature with protection from red, got {status:?}"
    );
}

#[test]
fn casting_a_colorless_spell_at_a_protected_target_still_works() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 5, None), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_red(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, colorless_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let p1 = PlayerId(1);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "the colorless spell's damage still lands");
}

#[test]
fn spell_fizzles_when_target_gains_protection_before_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 2, None), p1, ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);

    // CR 702.16c is rechecked by the same CR 608.2b mechanism as any other
    // target legality change - granting protection mid-stack, after the
    // spell already targeted this creature, makes the target illegal by
    // resolution time.
    engine.state_mut().continuous_effects.push(protection_from_red(target));

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
fn an_attacker_with_protection_cannot_be_blocked_by_a_matching_color_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 2, 2, None), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Red Blocker", 2, 2, Some(ColorSet::RED)), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_red(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::Error(_)),
        "a red creature can't block an attacker with protection from red, got {status:?}"
    );
}

#[test]
fn an_attacker_with_protection_can_still_be_blocked_by_a_different_color_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 2, 2, None), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Green Blocker", 2, 2, Some(ColorSet::GREEN)), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_red(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "a green creature can still block an attacker with only protection from red, got {status:?}"
    );
}
