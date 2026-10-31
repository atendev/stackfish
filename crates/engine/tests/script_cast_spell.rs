//! CR 601.2, 702.85a: `Game.Ops.CastSpell` is a script-callable cast ("you may cast it without paying its mana cost") built as `OperationOutcome::PendingCast`. It casts through the same synthetic `CastSpell` shape as `submit_cascade_cast` and pauses for its own {0} payment acknowledgment (CR 118.5a).

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const CONDITION_ENTERS_BATTLEFIELD: &str = r#"
return function(source, controller, event)
    return event ~= nil
       and event.kind == "zone_change"
       and event.object == source
       and event.to == "battlefield"
end
"#;

const OPERATION_CAST_EXILED_CANDIDATE: &str = r#"
return function(source, controller)
    return Game.Ops.CastSpell(1, { free = true })
end
"#;

fn caster_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        oracle_text: "When this creature enters the battlefield, cast the exiled card without \
                      paying its mana cost."
            .to_string(),
        script_ref: ScriptReference::Inline(String::new()),
        ..minimal_card_definition(
            0,
            "Test Caster",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn candidate_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(3)),
        toughness: Some(PTValue::Fixed(3)),
        ..minimal_card_definition(
            1,
            "Test Candidate",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(zones(
        PlayerId(0),
        PlayerId(1),
        &[
            ZoneKind::Hand,
            ZoneKind::Battlefield,
            ZoneKind::Graveyard,
            ZoneKind::Exile,
        ],
    ))
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for a {{0}} cost acknowledgment, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
}

#[test]
fn etb_trigger_casts_an_exiled_card_for_free_via_game_ops_cast_spell() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let caster = ObjectId(0);
    let candidate = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        caster,
        card_object(caster, caster_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(caster);
    state.objects.insert(
        candidate,
        card_object(candidate, candidate_definition(), p0, ZoneKind::Exile),
    );
    state
        .zone_mut(ZoneKind::Exile, None)
        .unwrap()
        .push_bottom(candidate);

    let mut engine = Engine::new(state);
    let condition = engine
        .load_script(CONDITION_ENTERS_BATTLEFIELD)
        .expect("condition script loads");
    let operation = engine
        .load_script(OPERATION_CAST_EXILED_CANDIDATE)
        .expect("operation script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: caster,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
        intervening_if: None,
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: caster,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority, caster is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    // Both players pass, which resolves the caster spell onto the battlefield and pushes its triggered ability.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert_eq!(
        engine.state().stack.len(),
        1,
        "the ETB ability itself is now on the stack"
    );

    // Both players pass again, which resolves the trigger and calls Game.Ops.CastSpell on the exiled candidate, pausing for its {0} payment.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    pay_free_and_drain(&mut engine);

    // The candidate is a fresh object on the stack (CR 400.7), not the exiled one.
    assert!(
        engine.state().object(candidate).is_none(),
        "the old exiled-zone id no longer resolves"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty(),
        "the candidate left exile"
    );
    assert_eq!(
        engine.state().stack.len(),
        1,
        "the cast candidate is now on the stack"
    );
    let cast_object = engine.state().stack.last().unwrap();
    assert_eq!(cast_object.controller, p0);

    // A third pass resolves the cast candidate onto the battlefield.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "got {status:?}"
    );

    assert!(engine.state().stack.is_empty(), "everything has resolved");
    let battlefield = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(
        battlefield.contents.len(),
        2,
        "both the caster and the cast candidate ended up on the battlefield"
    );
    let candidate_on_battlefield = battlefield
        .contents
        .iter()
        .filter_map(|id| engine.state().object(*id))
        .find(|o| o.definition.name == "Test Candidate");
    assert!(
        candidate_on_battlefield.is_some(),
        "the free-cast candidate resolved onto the battlefield"
    );
}

// An illegal script-requested cast (a nonexistent object id) does not panic or stick the pipeline, and the ability finishes resolving. This is the script error swallowed, per the TODO on that gap.
#[test]
fn casting_a_nonexistent_object_does_not_panic_and_the_triggering_ability_still_resolves() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let caster = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        caster,
        card_object(caster, caster_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(caster);

    let mut engine = Engine::new(state);
    let condition = engine
        .load_script(CONDITION_ENTERS_BATTLEFIELD)
        .expect("condition script loads");
    let operation = engine
        .load_script(
            r#"
            return function(source, controller)
                return Game.Ops.CastSpell(9999, { free = true })
            end
            "#,
        )
        .expect("operation script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: caster,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
        intervening_if: None,
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: caster,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority, caster is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert_eq!(
        engine.state().stack.len(),
        1,
        "the ETB ability itself is now on the stack"
    );

    // The operation requests an illegal cast: no panic, no stuck pipeline, and no payment pause.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "got {status:?}"
    );
    assert!(
        engine.state().stack.is_empty(),
        "the ability resolved with no effect - nothing left on the stack"
    );
}
