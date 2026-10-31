//! The CR 613 layer engine feeds combat-damage power (`AssignCombatDamage`) and Flying, Reach and Menace blocking legality (`DeclareBlockers`), both read from `&GameState`.
//! Earlier TODO comments said these needed a `&ScriptRuntime`, which does not apply to the primitive-only path.

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
    GameObject {
        controlled_since_turn,
        ..card_object(id, definition, owner, ZoneKind::Battlefield)
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let mut state = combat_state();
    state.step = Step::CombatDamage;
    state.combat = Some(combat);
    state
}

fn declare_attackers(
    engine: &mut Engine,
    player: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::DeclareAttackers { attackers },
        })
        .expect("player holds priority");
    drain(engine)
}

fn declare_blockers(
    engine: &mut Engine,
    player: PlayerId,
    blocks: Vec<(ObjectId, ObjectId)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::DeclareBlockers { blocks },
        })
        .expect("player holds priority");
    drain(engine)
}

fn try_declare_blockers(
    engine: &mut Engine,
    player: PlayerId,
    blocks: Vec<(ObjectId, ObjectId)>,
) -> Result<StepStatus, ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::DeclareBlockers { blocks },
    })?;
    Ok(drain(engine))
}

fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction {
                player: active,
                kind: PlayerActionKind::PassPriority,
            })
            .expect("priority window");
        drain(engine);
    }
    panic!(
        "never reached DeclareBlockers - state.step = {:?}",
        engine.state().step
    );
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments },
        })
        .expect("turn-based action, no priority required");
    drain(engine)
}

fn no_filter() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        self_only: false,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
    }
}

#[test]
fn a_modify_pt_boost_increases_combat_damage_dealt() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Bear", 3, 3), p0, 0),
    );
    state.continuous_effects.push(ContinuousEffect {
        source: attacker,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {
            self_only: true,
            ..no_filter()
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT {
            power: 2,
            toughness: 2,
        }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_life = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
    assert_eq!(
        p1_life, 15,
        "boosted 5 power (3 base + 2 from the continuous effect) should hit, not the printed 3"
    );
}

#[test]
fn without_the_boost_the_same_attacker_only_deals_its_printed_power() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Bear", 3, 3), p0, 0),
    );

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_life = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
    assert_eq!(p1_life, 17);
}

#[test]
fn granted_flying_via_continuous_effect_still_enforces_blocking_legality() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Drake", 2, 2), p0, 0),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker, creature_definition("Bear", 2, 2), p1, 0),
    );
    // No printed Flying on Drake, which a continuous effect grants instead (an aura's "enchanted creature has flying").
    state.continuous_effects.push(ContinuousEffect {
        source: attacker,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {
            self_only: true,
            ..no_filter()
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddKeyword(KeywordSet::FLYING)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let err = try_declare_blockers(&mut engine, p1, vec![(blocker, attacker)]).unwrap_err();

    assert!(
        matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "a continuous-effect-granted Flying should block just as legally as printed Flying, got {err:?}"
    );
}

#[test]
fn without_the_grant_the_same_attacker_is_blockable_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Drake", 2, 2), p0, 0),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker, creature_definition("Bear", 2, 2), p1, 0),
    );

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "no Flying granted - a vanilla bear should block legally, got {status:?}"
    );
}
