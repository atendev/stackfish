//! Phase 5 vertical slice: CR 702.16j's "protection from everything," the
//! last quality axis of `phase5/protection-from-color`/
//! `phase5/protection-from-card-type`/`phase5/protection-from-subtype`/
//! `phase5/protection-from-name`'s three enforcement sites (damage/
//! targeting/blocking). Unlike the other four qualities, "everything" is a
//! bare flag - `EffectiveCharacteristics.protection_everything: bool` -
//! rather than a set/list to intersect against the source's own
//! characteristics: CR 702.16j applies "regardless of that object's
//! characteristic values," so every source matches unconditionally. This
//! file only re-proves the "everything" half; the other four qualities (and
//! the OR-composition mechanics) are already covered by their own test
//! files.
//!
//! Same scoping as the earlier four slices: protection from a specific
//! player stays unmodeled (the last remaining CR 702.16 quality), and
//! Enchant/Equip (CR 702.16b) is out of scope (no attach/enchant machinery
//! exists).

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object as permanent, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const STATIC_PROTECTION_FROM_EVERYTHING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionEverything)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn creature_definition(definition_id: u64, name: &str, power: i32, toughness: i32, script: ScriptReference) -> CardDefinition {
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
        script_ref: script,
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn vanilla_creature(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    creature_definition(id, name, power, toughness, ScriptReference::ByKey(name.to_string()))
}

/// "Test Bolt: deals 2 damage to target creature." Plain, unremarkable
/// spell - the point of "protection from everything" is that *any* source
/// matches, so the targeting tests don't need a specially-qualified spell
/// the way the color/card-type/subtype/name slices did.
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
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
        mode_choice: ModeChoice::Exactly(1),
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
fn protection_from_everything(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionEverything),
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
fn game_effects_add_protection_everything_self_registers_and_applies() {
    let mut state = base_state();
    let bear = ObjectId(0);
    let definition = creature_definition(1, "Bear", 2, 2, ScriptReference::Inline(STATIC_PROTECTION_FROM_EVERYTHING.to_string()));
    put_on_battlefield(&mut state, permanent(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(
        &state.continuous_effects[0].effect,
        EffectPayload::Primitive(EffectPrimitive::AddProtectionEverything)
    ));

    let effective = effective_characteristics(&state, &runtime, bear);
    assert!(effective.protection_everything);
}

// ---------------------------------------------------------------------
// CR 702.16a: damage
// ---------------------------------------------------------------------

#[test]
fn combat_damage_from_any_source_is_prevented_by_protection_from_everything() {
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
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 3, 3), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Ward", 0, 5), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_everything(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 0, "protection from everything prevents combat damage from any source, no exceptions");
}

// ---------------------------------------------------------------------
// CR 702.16c: targeting
// ---------------------------------------------------------------------

#[test]
fn casting_any_spell_at_a_protected_target_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 2), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_everything(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "no spell can target a creature with protection from everything, got {status:?}"
    );
}

#[test]
fn spell_fizzles_when_target_gains_protection_from_everything_before_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla_creature(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
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
    engine.state_mut().continuous_effects.push(protection_from_everything(target));

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
fn an_attacker_with_protection_from_everything_cannot_be_blocked_by_any_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(1, "Bear", 2, 2), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, vanilla_creature(2, "Any Blocker", 2, 2), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_everything(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::Error(_)),
        "no creature can block an attacker with protection from everything, got {status:?}"
    );
}

// ---------------------------------------------------------------------
// RemoveAllAbilities consistency (CR 613.1f covers protection_everything
// too, same as the other four protection fields - see
// phase5_lose_all_abilities.rs's own analogous test for the color half).
// ---------------------------------------------------------------------

#[test]
fn remove_all_abilities_also_strips_a_granted_protection_from_everything() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, vanilla_creature(1, "Bear", 2, 2), p0, ZoneKind::Battlefield));
    state.continuous_effects.push(ContinuousEffect {
        source: bear,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionEverything),
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

    assert!(!effective.protection_everything, "the earlier-granted protection should be stripped too");
}
