//! Phase 5 vertical slice: backlog item 11e — commander damage production
//! (CR 903.10a). `PlayerState.commander_damage_taken` and the 21-damage SBA
//! loss check are both pre-existing (item 10's own slice) - this file only
//! exercises the write side: `commands::apply_damage_event` now
//! accumulates into it when `GameEventKind::DamageDealt.is_combat_damage`
//! is `true` and the source is `is_commander`. Non-combat damage from a
//! commander (an activated/triggered ability's own damage effect) and
//! combat damage from a non-commander never write to it.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, zones, two_player_state as base_two_player_state};

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

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, is_commander: bool) -> GameObject {
    GameObject { tapped: true, is_commander, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
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
        players: vec![PlayerState { life: 40, ..player_state(p0) }, PlayerState { life: 40, ..player_state(p1) }],
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
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
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
    });
    put_on_battlefield(&mut state, permanent(commander, creature_definition("General", 5, 5, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from("General"), 5);
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
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
    });
    put_on_battlefield(&mut state, permanent(general, creature_definition("General", 4, 4, KeywordSet::empty()), p0, true));
    put_on_battlefield(&mut state, permanent(admiral, creature_definition("Admiral", 6, 6, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from("General"), 4);
    assert_eq!(p1_state.commander_damage_from("Admiral"), 6);
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
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
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
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
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
    assert_eq!(p1_state.commander_damage_from("Plague General"), 4);
}

#[test]
fn a_commanders_non_combat_damage_does_not_count() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = combat_damage_state(CombatState { attackers: Vec::new(), blocks: Vec::new(), declared_defenders: vec![p1] , bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false });
    // No real combat this test - just a commander on the battlefield with
    // an activated damage ability, same shape
    // phase5_script_ops_damage.rs's own lifelink-ability test uses.
    let commander = ObjectId(0);
    let mut commander_obj = permanent(commander, creature_definition("General", 4, 4, KeywordSet::empty()), p0, true);
    commander_obj.tapped = false; // needs to pay {T} for the activated ability below
    put_on_battlefield(&mut state, commander_obj);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: commander,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(TargetRequirement { filter: TargetFilter::AnyTarget, count: 1, up_to: false, at_least: false, exclude_source: false })),
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
                x_value: None,
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
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
    });
    put_on_battlefield(&mut state, permanent(commander, creature_definition("General", 21, 21, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());

    assert!(matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p0), "got {status:?}");
    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from("General"), 21);
}

// ---------------------------------------------------------------------
// CR 903.9a recast identity: commander_damage_taken is keyed by name, not
// ObjectId (see PlayerState::commander_damage_taken's own doc comment) -
// a commander that deals damage, dies, returns to the command zone, and is
// recast under a fresh CR 400.7 id must still accumulate against the same
// total, not reset to 0 under a new key. Neither hit below (15, then
// another 15 once recast - the same card, same power, unblocked both
// times) is individually lethal at the CR 903.10a threshold - only the
// combined 30 crosses 21, so this test can only pass if the two hits are
// genuinely combined under one key despite three different ObjectIds
// across the test (commander_v1 -> commander_v2 in the command zone ->
// commander_v3 back on the battlefield).
// ---------------------------------------------------------------------

#[test]
fn commander_damage_survives_a_death_return_and_recast() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander_v1 = ObjectId(0);
    let mut state = GameState {
        players: vec![PlayerState { life: 40, ..player_state(p0) }, PlayerState { life: 40, ..player_state(p1) }],
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(CombatState {
            attackers: vec![(commander_v1, Target::Player(p1))],
            blocks: Vec::new(),
            declared_defenders: vec![p1],
            bands: Vec::new(),
            effect_blocked: Vec::new(),
            damage_assigned_this_step: false,
        }),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Command, ZoneKind::Battlefield, ZoneKind::Exile],
        ))
    };
    put_on_battlefield(&mut state, permanent(commander_v1, creature_definition("General", 15, 15, KeywordSet::empty()), p0, true));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());
    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from("General"), 15);
    assert_eq!(p1_state.life, 25);

    // Simulate the commander dying (not exercising real destruction here -
    // phase5_sba_basics.rs/phase5_combat_damage.rs already cover that; this
    // test is about the identity tracking once it's in the graveyard) and
    // clear combat/return to a normal main phase so a fresh cast is legal.
    let mut dead = engine.state_mut().objects.remove(&commander_v1).unwrap();
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(commander_v1);
    dead.zone = ZoneKind::Graveyard;
    engine.state_mut().zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(commander_v1);
    engine.state_mut().objects.insert(commander_v1, dead);
    engine.state_mut().combat = None;
    engine.state_mut().phase = Phase::Main;
    engine.state_mut().step = Step::PreCombatMain;
    engine.state_mut().priority_player = p0;

    // CR 903.9a: a commander freshly in the graveyard pauses the very next
    // SBA pass for the owner's return choice.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingCommanderReturnChoice { player, commander } if player == p0 && commander == commander_v1),
        "got {status:?}"
    );
    engine.submit_commander_return_choice(true).unwrap();
    drain(&mut engine);
    let commander_v2 = engine.state().zone(ZoneKind::Command, Some(p0)).unwrap().contents[0];
    assert_ne!(commander_v2, commander_v1, "CR 400.7 - the return itself already minted a new id");
    // The PassPriority above genuinely went through once the pause cleared,
    // handing priority to p1 - force it back to p0 rather than threading a
    // second real pass through (this test isn't about priority handoff).
    engine.state_mut().priority_player = p0;

    // Recast from the command zone - commander_casts["General"] is still 0
    // (commander_v1 was placed directly on the battlefield above, never
    // actually cast), so this recast's own cost stays {0}, no commander tax.
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: commander_v2,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, commander is castable from the command zone");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let commander_v3 = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .copied()
        .find(|&id| engine.state().object(id).is_some_and(|o| o.is_commander))
        .expect("the recast commander resolved onto the battlefield under yet another new id");
    assert_ne!(commander_v3, commander_v2, "CR 400.7 - casting itself already minted a new id");
    assert_ne!(commander_v3, commander_v1);

    // Second combat, second hit - the same 15-power card, unblocked again,
    // deliberately non-lethal on its own (15 < 21) at the CR 903.10a
    // threshold.
    engine.state_mut().phase = Phase::Combat;
    engine.state_mut().step = Step::CombatDamage;
    engine.state_mut().combat = Some(CombatState {
        attackers: vec![(commander_v3, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
    });
    let status = assign_combat_damage(&mut engine, p0, Vec::new());

    // 15 + 15 = 30 >= 21 - only reachable if both hits accumulated under
    // the same key despite three different ObjectIds across this test.
    assert!(matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p0), "got {status:?}");
    let p1_state = engine.state().players.iter().find(|p| p.player_id == p1).unwrap();
    assert_eq!(p1_state.commander_damage_from("General"), 30);
}
