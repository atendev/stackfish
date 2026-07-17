//! Phase 5 vertical slice: backlog item 11e — commander damage production
//! (CR 903.10a). `PlayerState.commander_damage_taken` and the 21-damage SBA
//! loss check are both pre-existing (item 10's own slice) - this file only
//! exercises the write side: `commands::apply_damage_event` now
//! accumulates into it when `GameEventKind::DamageDealt.is_combat_damage`
//! is `true` and the source is `is_commander`. Non-combat damage from a
//! commander (an activated/triggered ability's own damage effect) and
//! combat damage from a non-commander never write to it.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

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
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, is_commander: bool) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
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
        is_commander,
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
        life: 40,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: HashMap::new(),
    }
}

/// Two players, `p0` active/attacking, `p1` defending, `Phase::Combat`/
/// `Step::CombatDamage` with `combat` already populated - same shape
/// `phase5_combat_damage.rs` establishes, `AssignCombatDamage` is
/// immediately legal. 40 starting life (not 20) so a single 21-power hit
/// doesn't also trigger the ordinary 0-life SBA loss in the same pass -
/// keeps the commander-damage-specific loss test isolated to that check.
fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
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
fn unblocked_commander_attacker_accumulates_commander_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(commander, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(commander, creature_definition("General", 5, 5, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from(commander), 5);
    assert_eq!(p1_state.life, 35, "life loss still happens normally alongside the commander-damage count");
}

#[test]
fn two_commanders_attacking_the_same_player_track_independently() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let general = ObjectId(0);
    let admiral = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(general, Target::Player(p1)), (admiral, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(general, creature_definition("General", 4, 4, KeywordSet::empty()), p0, true));
    put_on_battlefield(&mut state, permanent(admiral, creature_definition("Admiral", 6, 6, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from(general), 4);
    assert_eq!(p1_state.commander_damage_from(admiral), 6);
    assert_eq!(p1_state.life, 30);
}

#[test]
fn non_commander_creatures_combat_damage_never_counts() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(bear, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Bear", 3, 3, KeywordSet::empty()), p0, false));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert!(p1_state.commander_damage_taken.is_empty());
    assert_eq!(p1_state.life, 37, "life loss still happens - only the commander-damage count is skipped");
}

#[test]
fn commander_combat_damage_still_accumulates_under_infect() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(commander, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
    });
    put_on_battlefield(
        &mut state,
        permanent(commander, creature_definition("Plague General", 4, 4, KeywordSet::INFECT), p0, true),
    );

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    // CR 702.90b: life untouched, poison counters instead - but CR 903.10a's
    // commander-damage count is a parallel tracking system, not a redirect
    // of the same number, so it still accumulates the full amount.
    assert_eq!(p1_state.life, 40, "infect redirects to poison, no life loss");
    assert_eq!(p1_state.counters.count(&CounterKind::Poison), 4);
    assert_eq!(p1_state.commander_damage_from(commander), 4);
}

#[test]
fn a_commanders_non_combat_damage_does_not_count() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = combat_damage_state(CombatState { attackers: Vec::new(), blocks: Vec::new(), declared_defenders: vec![p1] });
    // No real combat this test - just a commander on the battlefield with
    // an activated damage ability, same shape
    // phase5_script_ops_damage.rs's own lifelink-ability test uses.
    let commander = ObjectId(0);
    let mut commander_obj = permanent(commander, creature_definition("General", 4, 4, KeywordSet::empty()), p0, true);
    commander_obj.tapped = false; // needs to pay {T} for the activated ability below
    put_on_battlefield(&mut state, commander_obj);
    state.abilities.push(RegisteredAbility {
        source: commander,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(TargetRequirement { filter: TargetFilter::AnyTarget, count: 1 })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Damage(3)),
        intervening_if: None,
    });
    // Ability activation needs priority, not the turn-based-action shape
    // combat damage uses - switch to a normal main-phase state.
    state.phase = Phase::Main;
    state.step = Step::PreCombatMain;
    state.combat = None;

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: commander,
                ability_index: 0,
                targets: vec![Target::Player(p1)],
                chosen_colors: Vec::new(),
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(commander)] }).expect("Tap pays TapSelf");
    drain(&mut engine);
    // The ability resolves off the stack once both players pass, same shape
    // any other stack object uses.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert!(
        p1_state.commander_damage_taken.is_empty(),
        "an activated ability's damage effect isn't combat damage, even from a commander source"
    );
    assert_eq!(p1_state.life, 37, "the damage itself still lands - only the commander-damage count is skipped");
}

#[test]
fn commander_damage_reaching_21_ends_the_game() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(commander, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(commander, creature_definition("General", 21, 21, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());

    assert!(matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p0), "got {status:?}");
    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from(commander), 21);
}
