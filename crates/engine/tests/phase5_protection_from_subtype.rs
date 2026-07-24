//! Phase 5 vertical slice: CR 702.16's subtype quality ("protection from
//! Zombies," "protection from Arcane"), extending `phase5/protection-from-color`/
//! `phase5/protection-from-card-type`'s three enforcement sites (damage/
//! targeting/blocking) to a third, independent quality axis -
//! `EffectiveCharacteristics.protection_subtypes`, folded in the same
//! layer-6 pass from a sibling `EffectPrimitive::AddProtectionSubtype`. Each
//! enforcement site now checks "protected if the color, card-type, *or*
//! subtype quality intersects the source's own characteristics" - this file
//! only re-proves the subtype half; the color/card-type halves (and the
//! OR-composition mechanics) are already covered by
//! `phase5_protection_from_color.rs`/`phase5_protection_from_card_type.rs`.
//!
//! Unlike `ColorSet`/`CardTypeSet`, `Subtype` is an open-ended string, not a
//! bitflag `Set` - `AddProtectionSubtype` carries one `Subtype` per
//! primitive (CR 702.16g's "and" shorthand is already modeled as two
//! separate registered static abilities).
//!
//! Same scoping as the earlier two slices: protection from a specific
//! player or "from everything" stays unmodeled, and Enchant/Equip (CR
//! 702.16b) is out of scope (no attach/enchant machinery exists).

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object as permanent, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const STATIC_PROTECTION_FROM_ZOMBIES: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionSubtype("Zombie"))
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
    subtypes: Vec<Subtype>,
    script: ScriptReference,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes,
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

fn vanilla_creature(id: u64, name: &str, power: i32, toughness: i32, subtypes: Vec<Subtype>) -> CardDefinition {
    creature_definition(id, name, power, toughness, subtypes, ScriptReference::ByKey(name.to_string()))
}

fn zombie_creature(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    vanilla_creature(id, name, power, toughness, vec![Subtype("Zombie".to_string())])
}

fn plain_creature(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    vanilla_creature(id, name, power, toughness, Vec::new())
}

/// "Test Arcane Bolt: deals 2 damage to target creature," subtyped Arcane
/// (a real CR 205.4g instant/sorcery subtype) so the targeting tests have a
/// real spell-side subtype to check protection against.
fn arcane_bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Arcane Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: vec![Subtype("Arcane".to_string())],
        oracle_text: "Test Arcane Bolt: deals 2 damage to target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_arcane_bolt".to_string()),
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

/// A non-Arcane counterpart to `arcane_bolt_definition`, for the
/// "protection doesn't block an unrelated subtype" control test.
fn plain_bolt_definition() -> CardDefinition {
    CardDefinition { definition_id: 21, name: "Test Plain Bolt".to_string(), subtypes: Vec::new(), ..arcane_bolt_definition() }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Directly-constructed protection, standing in for the ETB-registered
/// version - the Lua wiring itself is proven separately, same convention
/// every other static-ability-adjacent test file in this repo uses.
fn protection_from_zombies(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionSubtype(Subtype("Zombie".to_string()))),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn protection_from_arcane(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionSubtype(Subtype("Arcane".to_string()))),
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
fn game_effects_add_protection_subtype_self_registers_and_applies() {
    let mut state = base_state();
    let bear = ObjectId(0);
    let definition = creature_definition(
        1,
        "Bear",
        2,
        2,
        Vec::new(),
        ScriptReference::Inline(STATIC_PROTECTION_FROM_ZOMBIES.to_string()),
    );
    put_on_battlefield(&mut state, permanent(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(
        &state.continuous_effects[0].effect,
        EffectPayload::Primitive(EffectPrimitive::AddProtectionSubtype(s)) if s.0 == "Zombie"
    ));

    let effective = effective_characteristics(&state, &runtime, bear);
    assert_eq!(effective.protection_subtypes, vec![Subtype("Zombie".to_string())]);
}

// ---------------------------------------------------------------------
// CR 702.16a: damage
// ---------------------------------------------------------------------

#[test]
fn combat_damage_from_a_matching_subtype_is_prevented_by_protection() {
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
    put_on_battlefield(&mut state, permanent(attacker, zombie_creature(1, "Ghoul", 3, 3), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, plain_creature(2, "Ward", 0, 5), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_zombies(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 0, "protection from Zombies prevents the zombie attacker's combat damage");
}

#[test]
fn combat_damage_from_a_non_matching_subtype_still_lands() {
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
    put_on_battlefield(&mut state, permanent(attacker, plain_creature(1, "Bear", 3, 3), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, plain_creature(2, "Ward", 0, 5), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_zombies(blocker));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let ward = engine.state().object(blocker).unwrap();
    assert_eq!(ward.damage_marked, 3, "protection from Zombies doesn't stop a plain creature's damage");
}

// ---------------------------------------------------------------------
// CR 702.16c: targeting
// ---------------------------------------------------------------------

#[test]
fn casting_a_matching_subtype_spell_at_a_protected_target_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, plain_creature(1, "Bear", 2, 2), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_arcane(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, arcane_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "an Arcane spell can't target a creature with protection from Arcane, got {status:?}"
    );
}

#[test]
fn casting_a_non_matching_subtype_spell_at_a_protected_target_still_works() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, plain_creature(1, "Bear", 2, 5), PlayerId(1), ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_arcane(target));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, plain_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let p1 = PlayerId(1);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "the non-Arcane spell's damage still lands");
}

#[test]
fn spell_fizzles_when_target_gains_protection_before_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, plain_creature(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, arcane_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);

    // CR 702.16c is rechecked by the same CR 608.2b mechanism as any other
    // target legality change - granting protection mid-stack, after the
    // spell already targeted this creature, makes the target illegal by
    // resolution time.
    engine.state_mut().continuous_effects.push(protection_from_arcane(target));

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
fn an_attacker_with_protection_cannot_be_blocked_by_a_matching_subtype_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, plain_creature(1, "Bear", 2, 2), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, zombie_creature(2, "Zombie Blocker", 2, 2), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_zombies(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::Error(_)),
        "a Zombie creature can't block an attacker with protection from Zombies, got {status:?}"
    );
}

#[test]
fn an_attacker_with_protection_can_still_be_blocked_by_a_non_matching_subtype_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, plain_creature(1, "Bear", 2, 2), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, permanent(blocker, plain_creature(2, "Plain Blocker", 2, 2), p1, ZoneKind::Battlefield));
    state.continuous_effects.push(protection_from_zombies(attacker));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "a non-Zombie creature can still block an attacker with only protection from Zombies, got {status:?}"
    );
}

// ---------------------------------------------------------------------
// RemoveAllAbilities consistency (CR 613.1f covers protection_subtypes too,
// same as protection_colors/protection_card_types - see
// phase5_lose_all_abilities.rs's own analogous test for the color half).
// ---------------------------------------------------------------------

#[test]
fn remove_all_abilities_also_strips_a_granted_protection_subtype() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, plain_creature(1, "Bear", 2, 2), p0, ZoneKind::Battlefield));
    state.continuous_effects.push(ContinuousEffect {
        source: bear,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionSubtype(Subtype("Zombie".to_string()))),
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

    assert_eq!(effective.protection_subtypes, Vec::<Subtype>::new(), "the earlier-granted protection should be stripped too");
}
