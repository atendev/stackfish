//! CR 611.2a: Timed timing and zone permissions from a resolving effect, and their expiry in the
//! turn drains. Static permissions are in timing_permissions.rs and zone_permissions.rs.

use std::collections::VecDeque;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, cast_spell, drain, minimal_card_definition, pass_both_and_drain,
    pay_free_and_drain, player_state, two_player_main_phase_state,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const SPELL: ObjectId = ObjectId(20);
const GONE: ObjectId = ObjectId(90);

fn timing_template(duration: EffectDuration) -> TimingPermission {
    TimingPermission {
        source: ObjectId(0),
        controller: P1,
        affected: FilterController::You,
        spells: FilterSpec::ANY,
        duration,
    }
}

fn zone_template(duration: EffectDuration) -> ZonePermission {
    ZonePermission {
        source: ObjectId(0),
        controller: P1,
        affected: FilterController::You,
        zone: ZoneKind::Graveyard,
        owners: FilterController::You,
        kind: ZonePermissionKind::PlayLand,
        cards: FilterSpec::ANY,
        library_top: false,
        linked: false,
        reveals_top: false,
        duration,
        rider: None,
    }
}

fn grant_timing(permission: TimingPermission) -> AbilityEffect {
    AbilityEffect::GrantTimingPermission(Box::new(permission))
}

fn grant_zone(permission: ZonePermission) -> AbilityEffect {
    AbilityEffect::GrantZonePermission(Box::new(permission))
}

fn grant_spell(effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        ..minimal_card_definition(
            20,
            "Test Grant",
            "test_grant",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

// P0 casts the grant in their precombat main phase and it resolves.
fn resolve_grant(effect: AbilityEffect) -> Engine {
    let mut state = two_player_main_phase_state();
    state.objects.insert(
        SPELL,
        card_object(SPELL, grant_spell(effect), P0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(SPELL);
    let mut engine = Engine::new(state);
    cast_spell(&mut engine, P0, SPELL, Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    engine
}

// Passes priority until `stop` holds at a priority pause.
fn pass_until(engine: &mut Engine, stop: impl Fn(&GameState) -> bool) {
    loop {
        match drain(engine) {
            StepStatus::AwaitingAction { player } => {
                if stop(engine.state()) {
                    return;
                }
                engine
                    .submit_action(PlayerAction {
                        player,
                        kind: PlayerActionKind::PassPriority,
                    })
                    .unwrap();
            }
            other => panic!("unexpected pause while passing: {other:?}"),
        }
    }
}

fn at(phase: Phase, step: Step) -> impl Fn(&GameState) -> bool {
    move |state| state.phase == phase && state.step == step
}

fn turn_of(player: PlayerId) -> impl Fn(&GameState) -> bool {
    move |state| state.active_player == player && state.step == Step::Upkeep
}

fn with_player_two(mut state: GameState) -> GameState {
    state.players.push(player_state(P2));
    state.turn_order.push(P2);
    state.last_active_turn_number.insert(P2, 1);
    for kind in [ZoneKind::Hand, ZoneKind::Graveyard] {
        state.zones.push(Zone {
            kind,
            owner: Some(P2),
            contents: VecDeque::new(),
        });
    }
    // Each player draws once on the way back to P0, and an empty library loses (CR 704.5b).
    for (i, player) in [P0, P1, P2].into_iter().enumerate() {
        let card = ObjectId(60 + i as u64);
        state.objects.insert(
            card,
            card_object(
                card,
                grant_spell(AbilityEffect::Shuffle),
                player,
                ZoneKind::Library,
            ),
        );
        state.zones.push(Zone {
            kind: ZoneKind::Library,
            owner: Some(player),
            contents: VecDeque::from([card]),
        });
    }
    state
}

// CR 611.2a
#[test]
fn a_resolved_timing_grant_takes_the_spell_and_its_controller() {
    let engine = resolve_grant(grant_timing(timing_template(
        EffectDuration::UntilEndOfTurn,
    )));
    let permissions = &engine.state().timing_permissions;
    assert_eq!(permissions.len(), 1);
    assert_eq!(permissions[0].controller, P0);
    assert_eq!(permissions[0].duration, EffectDuration::UntilEndOfTurn);
    assert!(
        engine.state().object(permissions[0].source).is_none(),
        "the source was the spell on the stack, which has left it"
    );
}

// CR 611.2a, 514.2
#[test]
fn an_until_end_of_turn_timing_grant_survives_combat_and_expires_at_cleanup() {
    let mut engine = resolve_grant(grant_timing(timing_template(
        EffectDuration::UntilEndOfTurn,
    )));
    pass_until(&mut engine, at(Phase::Ending, Step::End));
    assert_eq!(engine.state().timing_permissions.len(), 1);
    pass_until(&mut engine, turn_of(P1));
    assert!(engine.state().timing_permissions.is_empty());
}

// CR 611.2a, 514.2
#[test]
fn an_until_end_of_turn_zone_grant_expires_at_cleanup() {
    let mut engine = resolve_grant(grant_zone(zone_template(EffectDuration::UntilEndOfTurn)));
    assert_eq!(engine.state().zone_permissions.len(), 1);
    assert_eq!(engine.state().zone_permissions[0].controller, P0);
    pass_until(&mut engine, at(Phase::Ending, Step::End));
    assert_eq!(engine.state().zone_permissions.len(), 1);
    pass_until(&mut engine, turn_of(P1));
    assert!(engine.state().zone_permissions.is_empty());
}

// CR 511.3
#[test]
fn an_until_end_of_combat_grant_expires_when_combat_ends() {
    let mut engine = resolve_grant(grant_zone(zone_template(EffectDuration::UntilEndOfCombat)));
    assert_eq!(engine.state().zone_permissions.len(), 1);
    pass_until(&mut engine, at(Phase::Main, Step::PostCombatMain));
    assert!(engine.state().zone_permissions.is_empty());
}

// CR 611.2a
#[test]
fn an_until_your_next_turn_grant_lasts_until_its_controllers_next_turn() {
    let mut state = with_player_two(two_player_main_phase_state());
    state.timing_permissions.push(TimingPermission {
        source: GONE,
        controller: P0,
        ..timing_template(EffectDuration::UntilYourNextTurn)
    });
    let mut engine = Engine::new(state);
    pass_until(&mut engine, turn_of(P1));
    assert_eq!(engine.state().timing_permissions.len(), 1);
    pass_until(&mut engine, turn_of(P2));
    assert_eq!(engine.state().timing_permissions.len(), 1);
    pass_until(&mut engine, turn_of(P0));
    assert!(
        engine.state().timing_permissions.is_empty(),
        "the source never existed, and the grant still expires by its controller"
    );
}

// CR 611.3b
#[test]
fn a_static_permission_is_not_expired_by_cleanup() {
    let mut state = two_player_main_phase_state();
    state.timing_permissions.push(TimingPermission {
        source: GONE,
        controller: P0,
        ..timing_template(EffectDuration::WhileSourceOnBattlefield)
    });
    let mut engine = Engine::new(state);
    pass_until(&mut engine, turn_of(P1));
    assert_eq!(engine.state().timing_permissions.len(), 1);
}

// CR 607.2a
#[test]
fn a_timed_linked_grant_from_a_spell_matches_nothing() {
    let exiled = ObjectId(30);
    let engine = resolve_grant(grant_zone(ZonePermission {
        zone: ZoneKind::Exile,
        kind: ZonePermissionKind::Cast,
        linked: true,
        ..zone_template(EffectDuration::UntilEndOfTurn)
    }));
    let mut state = engine.state().clone();
    state.zones.push(Zone {
        kind: ZoneKind::Exile,
        owner: None,
        contents: VecDeque::from([exiled]),
    });
    let card = card_object(
        exiled,
        grant_spell(AbilityEffect::Shuffle),
        P0,
        ZoneKind::Exile,
    );
    state.objects.insert(exiled, card.clone());
    assert_eq!(state.zone_permissions.len(), 1);
    assert!(!has_zone_permission(
        &state,
        ZonePermissionKind::Cast,
        P0,
        &card
    ));
}

// CR 607.2a
#[test]
fn a_timed_linked_grant_matches_while_its_source_stays() {
    let source = ObjectId(40);
    let exiled = ObjectId(30);
    let mut state = two_player_main_phase_state();
    state.zones.push(Zone {
        kind: ZoneKind::Exile,
        owner: None,
        contents: VecDeque::from([exiled]),
    });
    let card = card_object(
        exiled,
        grant_spell(AbilityEffect::Shuffle),
        P0,
        ZoneKind::Exile,
    );
    state.objects.insert(exiled, card.clone());
    state.exiled_with.push(LinkedExile { source, exiled });
    state.zone_permissions.push(ZonePermission {
        source,
        controller: P0,
        zone: ZoneKind::Exile,
        kind: ZonePermissionKind::Cast,
        linked: true,
        ..zone_template(EffectDuration::UntilEndOfTurn)
    });
    assert!(has_zone_permission(
        &state,
        ZonePermissionKind::Cast,
        P0,
        &card
    ));
}

fn load(source: &str) -> Result<AbilityEffect, String> {
    ScriptRuntime::new()
        .load_operation(source)
        .map(|(effect, _)| effect.expect("a Game.Ops value"))
        .map_err(|e| format!("{e:?}"))
}

// CR 611.2a
#[test]
fn the_lua_grant_ops_build_timed_permissions() {
    let timing = load(
        "local p = Game.CreateTimingPermission(7)\np:SetAffected(Game.Affected.Opponent)\n\
         return Game.Ops.GrantTimingPermission(p, Game.Durations.UntilYourNextTurn)",
    )
    .unwrap();
    assert_eq!(
        timing,
        grant_timing(TimingPermission {
            source: ObjectId(7),
            controller: P0,
            affected: FilterController::Opponent,
            spells: FilterSpec::ANY,
            duration: EffectDuration::UntilYourNextTurn,
        })
    );
    let zone = load(
        "local p = Game.CreateZonePermission(7)\np:SetAffected(Game.Affected.You)\n\
         p:SetZone(\"Graveyard\")\np:SetKind(\"PlayLand\")\n\
         return Game.Ops.GrantZonePermission(p, Game.Durations.UntilEndOfCombat)",
    )
    .unwrap();
    assert_eq!(
        zone,
        grant_zone(ZonePermission {
            source: ObjectId(7),
            controller: P0,
            ..zone_template(EffectDuration::UntilEndOfCombat)
        })
    );
}

// CR 611.2b
// `Game.Durations` binds only `Permanent` beyond the three drained durations.
#[test]
fn the_lua_grant_ops_refuse_a_permanent_duration_and_a_missing_setter() {
    let err = load(
        "local p = Game.CreateTimingPermission(7)\np:SetAffected(Game.Affected.You)\n\
         return Game.Ops.GrantTimingPermission(p, Game.Durations.Permanent)",
    )
    .unwrap_err();
    assert!(
        err.contains("UntilEndOfTurn") && err.contains("611.2b"),
        "{err}"
    );
    let err = load(
        "local p = Game.CreateZonePermission(7)\np:SetAffected(Game.Affected.You)\n\
         return Game.Ops.GrantZonePermission(p, Game.Durations.UntilEndOfTurn)",
    )
    .unwrap_err();
    assert!(err.contains("SetZone"), "{err}");
}

// CR 611.2a
#[test]
fn a_grant_op_does_not_register_its_builder() {
    let mut state = two_player_main_phase_state();
    let definition = CardDefinition {
        script_ref: ScriptReference::Inline(
            "local s = {}\nfunction s.register_abilities(c)\n\
             local p = Game.CreateTimingPermission(c)\np:SetAffected(Game.Affected.You)\n\
             Game.Ops.GrantTimingPermission(p, Game.Durations.UntilEndOfTurn)\nend\nreturn s"
                .to_string(),
        ),
        ..grant_spell(AbilityEffect::Shuffle)
    };
    let source = ObjectId(50);
    state.objects.insert(
        source,
        card_object(source, definition.clone(), P0, ZoneKind::Battlefield),
    );
    ScriptRuntime::new()
        .run_card_script(&mut state, &definition, source)
        .unwrap();
    assert!(state.timing_permissions.is_empty());
}

// CR 611.2a
#[test]
fn a_grant_from_an_activated_ability_takes_the_permanent_as_its_source() {
    let source = ObjectId(40);
    let mut state = two_player_main_phase_state();
    state.objects.insert(
        source,
        card_object(
            source,
            grant_spell(AbilityEffect::Shuffle),
            P0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(source);
    state.abilities.push(RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec {
            mana: None,
            additional: vec![AdditionalCost::TapSelf],
            alternative: Vec::new(),
        }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(grant_timing(timing_template(
            EffectDuration::UntilEndOfCombat,
        ))),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                modes: Vec::new(),
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                division: Vec::new(),
                x_value: None,
            },
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Tap(source)],
        })
        .unwrap();
    drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    let permissions = &engine.state().timing_permissions;
    assert_eq!(permissions.len(), 1);
    assert_eq!(permissions[0].source, source);
    assert_eq!(permissions[0].controller, P0);
    pass_until(&mut engine, at(Phase::Main, Step::PostCombatMain));
    assert!(engine.state().timing_permissions.is_empty());
}

// CR 401.5, 514.2
#[test]
fn a_timed_library_top_grant_stops_revealing_at_cleanup() {
    let top = ObjectId(30);
    let mut engine = resolve_grant(grant_zone(ZonePermission {
        zone: ZoneKind::Library,
        kind: ZonePermissionKind::Cast,
        library_top: true,
        reveals_top: true,
        ..zone_template(EffectDuration::UntilEndOfTurn)
    }));
    let mut state = engine.state().clone();
    let card = card_object(
        top,
        grant_spell(AbilityEffect::Shuffle),
        P0,
        ZoneKind::Library,
    );
    state.objects.insert(top, card.clone());
    state.zones.push(Zone {
        kind: ZoneKind::Library,
        owner: Some(P0),
        contents: VecDeque::from([top]),
    });
    assert!(library_top_visible_to(&state, &card, Some(P1)));
    engine = Engine::new(state);
    pass_until(&mut engine, turn_of(P1));
    let card = engine.state().object(top).unwrap().clone();
    assert!(!library_top_visible_to(engine.state(), &card, Some(P1)));
}
