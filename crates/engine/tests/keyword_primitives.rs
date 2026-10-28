//! CR 702 keywords that change an engine checkpoint, stored as `KeywordSet` bits: Flying, Reach and Menace (blocking), Vigilance, Haste and Defender (attacking), Flash (casting timing), Deathtouch and Indestructible (SBA), Lifelink, Wither and Infect (damage).
//! `PTValue::net_with_counters` nets the +1/+1 and -1/-1 counters that Wither and Infect rely on.
// TODO: keyword-name to primitive-or-ability mapping layer (see script_api.rs)

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state, zones};

fn creature_definition(name: &str, power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
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
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(
    id: ObjectId,
    definition: CardDefinition,
    owner: PlayerId,
    controlled_since_turn: u32,
) -> GameObject {
    GameObject { controlled_since_turn, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let mut state = combat_state();
    state.step = Step::CombatDamage;
    state.combat = Some(combat);
    state
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn try_declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        ?;
    Ok(drain(engine))
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

fn try_declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        ?;
    Ok(drain(engine))
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

#[test]
fn flying_attacker_cannot_be_blocked_by_non_flying_non_reach() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Drake", 2, 2, KeywordSet::FLYING), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 2, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let err = try_declare_blockers(&mut engine, p1, vec![(blocker, attacker)]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

#[test]
fn flying_attacker_can_be_blocked_by_reach_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Drake", 2, 2, KeywordSet::FLYING), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Spider", 2, 2, KeywordSet::REACH), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "reach legally blocks flying, got {status:?}");
}

#[test]
fn menace_attacker_rejects_a_single_blocker() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Menacer", 2, 2, KeywordSet::MENACE), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 2, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let err = try_declare_blockers(&mut engine, p1, vec![(blocker, attacker)]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

#[test]
fn menace_attacker_accepts_two_blockers() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_a = ObjectId(1);
    let blocker_b = ObjectId(2);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Menacer", 2, 2, KeywordSet::MENACE), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Bear1", 2, 2, KeywordSet::empty()), p1, 0));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Bear2", 2, 2, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker_a, attacker), (blocker_b, attacker)]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "two blockers legally block menace, got {status:?}");
}

#[test]
fn vigilance_attacker_does_not_tap() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Watcher", 2, 2, KeywordSet::VIGILANCE), p0, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(!engine.state().object(attacker).unwrap().tapped, "702.20a: vigilance means attacking doesn't tap it");
    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Tapped { object } if object == attacker)),
        "no Tapped event for a vigilance attacker"
    );
}

#[test]
fn haste_creature_can_attack_the_turn_it_entered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    // controlled_since_turn equals the current turn (1), so it is sick unless Haste overrides it.
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Sprinter", 2, 2, KeywordSet::HASTE), p0, 1));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "haste bypasses summoning sickness, got {status:?}");
}

#[test]
fn creature_without_haste_cannot_attack_the_turn_it_entered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rookie", 2, 2, KeywordSet::empty()), p0, 1));

    let mut engine = Engine::new(state);
    let err = try_declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

#[test]
fn defender_creature_cannot_attack() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Wall", 0, 4, KeywordSet::DEFENDER), p0, 0));

    let mut engine = Engine::new(state);
    let err = try_declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

fn self_only_keyword_effect(source: ObjectId, effect: EffectPrimitive) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

#[test]
fn haste_granted_by_a_continuous_effect_bypasses_summoning_sickness() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    // controlled_since_turn equals the current turn (1) and no Haste is printed, so only a granted one helps.
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Sprinter", 2, 2, KeywordSet::empty()), p0, 1));
    state.continuous_effects.push(self_only_keyword_effect(attacker, EffectPrimitive::AddKeyword(KeywordSet::HASTE)));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "a continuous effect granting Haste must bypass summoning sickness too, got {status:?}"
    );
}

#[test]
fn defender_granted_by_a_continuous_effect_prevents_attacking() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Cursed Bear", 2, 2, KeywordSet::empty()), p0, 0));
    state.continuous_effects.push(self_only_keyword_effect(attacker, EffectPrimitive::AddKeyword(KeywordSet::DEFENDER)));

    let mut engine = Engine::new(state);
    let err = try_declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "a continuous effect granting Defender must actually prevent attacking, got {err:?}");
}

#[test]
fn vigilance_granted_by_a_continuous_effect_prevents_tapping() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Watcher", 2, 2, KeywordSet::empty()), p0, 0));
    state.continuous_effects.push(self_only_keyword_effect(attacker, EffectPrimitive::AddKeyword(KeywordSet::VIGILANCE)));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(!engine.state().object(attacker).unwrap().tapped, "a continuous effect granting Vigilance must skip the tap too");
}

// RemoveKeyword(Defender) lets a Defender attack because the keyword is gone. That is not the Assault Formation shape, since a card checking "has defender" would see it missing.
#[test]
fn remove_keyword_lets_a_printed_defender_creature_attack() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Wall", 0, 4, KeywordSet::DEFENDER), p0, 0));
    state.continuous_effects.push(self_only_keyword_effect(attacker, EffectPrimitive::RemoveKeyword(KeywordSet::DEFENDER)));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "RemoveKeyword(Defender) must let this wall attack as though it didn't have defender, got {status:?}"
    );
}

#[test]
fn remove_keyword_only_strips_the_named_keyword() {
    let bear = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Drake", 2, 2, KeywordSet::FLYING | KeywordSet::VIGILANCE), PlayerId(0), 0));
    state.continuous_effects.push(self_only_keyword_effect(bear, EffectPrimitive::RemoveKeyword(KeywordSet::FLYING)));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(!effective.keywords.contains(KeywordSet::FLYING), "Flying was named, must be stripped");
    assert!(effective.keywords.contains(KeywordSet::VIGILANCE), "Vigilance wasn't named, unlike RemoveAllAbilities this must survive");
}

#[test]
fn a_later_add_keyword_still_applies_after_an_earlier_remove_keyword() {
    let bear = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Drake", 2, 2, KeywordSet::FLYING), PlayerId(0), 0));
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(0),
        ..self_only_keyword_effect(bear, EffectPrimitive::RemoveKeyword(KeywordSet::FLYING))
    });
    // A later timestamp, such as a second effect resolving after the removal, still sticks on top.
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(1),
        ..self_only_keyword_effect(bear, EffectPrimitive::AddKeyword(KeywordSet::FLYING))
    });

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(effective.keywords.contains(KeywordSet::FLYING), "a later-timestamp grant still applies on top of an earlier removal");
}

#[test]
fn a_later_remove_keyword_strips_an_earlier_add_keyword() {
    let bear = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Drake", 2, 2, KeywordSet::empty()), PlayerId(0), 0));
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(0),
        ..self_only_keyword_effect(bear, EffectPrimitive::AddKeyword(KeywordSet::FLYING))
    });
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(1),
        ..self_only_keyword_effect(bear, EffectPrimitive::RemoveKeyword(KeywordSet::FLYING))
    });

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(!effective.keywords.contains(KeywordSet::FLYING), "a later-timestamp removal strips an earlier grant");
}

// The Assault Formation shape: a printed Defender with an IgnoreKeyword(Defender) static can attack and still has Defender.
#[test]
fn ignore_keyword_lets_a_printed_defender_creature_attack() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Wall", 0, 4, KeywordSet::DEFENDER), p0, 0));
    state.continuous_effects.push(self_only_keyword_effect(attacker, EffectPrimitive::IgnoreKeyword(KeywordSet::DEFENDER)));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), attacker);
    assert!(effective.keywords.contains(KeywordSet::DEFENDER), "unlike RemoveKeyword, this wall must still have defender");

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "IgnoreKeyword(Defender) must let this wall attack as though it didn't have defender, got {status:?}"
    );
}

// Without a matching IgnoreKeyword grant a printed Defender cannot attack, since the exemption must name the keyword.
#[test]
fn defender_creature_still_cannot_attack_without_a_matching_ignore_grant() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Wall", 0, 4, KeywordSet::DEFENDER), p0, 0));
    // An exemption for a different keyword must not lift the Defender restriction.
    state.continuous_effects.push(self_only_keyword_effect(attacker, EffectPrimitive::IgnoreKeyword(KeywordSet::VIGILANCE)));

    let mut engine = Engine::new(state);
    let err = try_declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "an ignore grant for an unrelated keyword must not lift the Defender restriction, got {err:?}");
}

#[test]
fn a_later_remove_all_abilities_clears_an_earlier_ignore_keyword() {
    let bear = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Wall", 0, 4, KeywordSet::empty()), PlayerId(0), 0));
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(0),
        ..self_only_keyword_effect(bear, EffectPrimitive::AddKeyword(KeywordSet::DEFENDER))
    });
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(1),
        ..self_only_keyword_effect(bear, EffectPrimitive::IgnoreKeyword(KeywordSet::DEFENDER))
    });
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(2),
        ..self_only_keyword_effect(bear, EffectPrimitive::RemoveAllAbilities)
    });
    // Defender is re-granted after the strip but the ignore grant is not, so the restriction applies again.
    state.continuous_effects.push(ContinuousEffect {
        timestamp: SequenceNo(3),
        ..self_only_keyword_effect(bear, EffectPrimitive::AddKeyword(KeywordSet::DEFENDER))
    });

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(effective.keywords.contains(KeywordSet::DEFENDER), "defender was re-granted after the strip");
    assert!(
        !effective.ignored_keywords.contains(KeywordSet::DEFENDER),
        "RemoveAllAbilities must clear ignored_keywords same as it clears keywords - it wasn't re-granted"
    );
}

fn hand_state_outside_main_phase(keywords: KeywordSet) -> (GameState, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = GameState {
        // Not Main - only Flash (or an instant) can be cast here.
        phase: Phase::Combat,
        step: Step::BeginningOfCombat,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    let object = permanent(card, creature_definition("Ambusher", 2, 2, keywords), p0, 0);
    state.objects.insert(card, object);
    state.object_mut(card).unwrap().zone = ZoneKind::Hand;
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    (state, card)
}

#[test]
fn flash_creature_castable_outside_main_phase() {
    let (state, card) = hand_state_outside_main_phase(KeywordSet::FLASH);
    let p0 = PlayerId(0);
    let mut engine = Engine::new(state);
    let status = engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false} })
        .map(|_| drain(&mut engine));
    assert!(
        matches!(status, Ok(StepStatus::AwaitingPayment { .. })),
        "702.8b: flash lets a non-instant cast any time its controller could cast an instant, got {status:?}"
    );
}

#[test]
fn non_flash_creature_rejected_outside_main_phase() {
    let (state, card) = hand_state_outside_main_phase(KeywordSet::empty());
    let p0 = PlayerId(0);
    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false} })
        .unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "without flash, a creature needs the sorcery-speed window, got {err:?}");
}

#[test]
fn deathtouch_source_kills_a_bigger_creature_with_one_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Deathtoucher", 1, 1, KeywordSet::DEATHTOUCH), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Giant", 8, 8, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(engine.state().object(blocker).is_none(), "702.2b/c: any nonzero deathtouch damage is lethal");
}

#[test]
fn indestructible_creature_survives_lethal_marked_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 5, KeywordSet::empty()), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Golem", 1, 3, KeywordSet::INDESTRUCTIBLE), p1, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(engine.state().object(blocker).is_some(), "702.12b: indestructible ignores 704.5g's lethal-damage check");
    assert_eq!(engine.state().object(blocker).unwrap().damage_marked, 5);
}

#[test]
fn indestructible_creature_still_dies_to_zero_toughness() {
    let mut state = combat_damage_state(CombatState { attackers: Vec::new(), blocks: Vec::new(), declared_defenders: Vec::new() , bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new() });
    let creature = ObjectId(0);
    let mut object = permanent(creature, creature_definition("Golem", 1, 1, KeywordSet::INDESTRUCTIBLE), PlayerId(0), 0);
    object.counters.add(CounterKind::MinusOneMinusOne, 1, SequenceNo(0));
    put_on_battlefield(&mut state, object);

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, PlayerId(0), Vec::new());
    assert!(
        engine.state().object(creature).is_none(),
        "702.12b only exempts 704.5g (damage-based) - 704.5f (0-or-less toughness) still applies"
    );
}

#[test]
fn lifelink_source_gains_its_controller_life_on_combat_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Lifelinker", 3, 3, KeywordSet::LIFELINK), p0, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    assert_eq!(engine.state().player(p0).unwrap().life, 23, "702.15b: lifelink gains its controller life equal to damage dealt");
    assert_eq!(engine.state().player(p1).unwrap().life, 17);
}

#[test]
fn wither_damage_becomes_counters_not_marked_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Wither Beast", 3, 3, KeywordSet::WITHER), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 5, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    let blocker_obj = engine.state().object(blocker).unwrap();
    assert_eq!(blocker_obj.damage_marked, 0, "702.80a: wither damage isn't marked");
    assert_eq!(blocker_obj.counters.count(&CounterKind::MinusOneMinusOne), 3, "702.80a: wither damage becomes -1/-1 counters instead");
}

#[test]
fn infect_damage_to_a_player_becomes_poison_counters() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Infector", 3, 3, KeywordSet::INFECT), p0, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    assert_eq!(engine.state().player(p1).unwrap().life, 20, "702.90b: infect damage to a player doesn't cause life loss");
    assert_eq!(engine.state().player(p1).unwrap().counters.count(&CounterKind::Poison), 3);
}

#[test]
fn infect_damage_to_a_creature_becomes_minus_counters() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Infector", 3, 3, KeywordSet::INFECT), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 5, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    let blocker_obj = engine.state().object(blocker).unwrap();
    assert_eq!(blocker_obj.damage_marked, 0, "702.90c: infect damage to a creature isn't marked either");
    assert_eq!(blocker_obj.counters.count(&CounterKind::MinusOneMinusOne), 3);
}

#[test]
fn net_with_counters_folds_plus_and_minus_counters() {
    let toughness = PTValue::Fixed(4);
    let mut counters = CounterSet::new();
    assert_eq!(toughness.net_with_counters(&counters), Some(4));
    counters.add(CounterKind::MinusOneMinusOne, 3, SequenceNo(0));
    assert_eq!(toughness.net_with_counters(&counters), Some(1));
    counters.add(CounterKind::PlusOnePlusOne, 2, SequenceNo(1));
    assert_eq!(toughness.net_with_counters(&counters), Some(3));
}

#[test]
fn net_with_counters_is_none_for_a_characteristic_defining_value() {
    let toughness = PTValue::CharacteristicDefining { display: "*".to_string(), script: None };
    assert_eq!(toughness.net_with_counters(&CounterSet::new()), None);
}
