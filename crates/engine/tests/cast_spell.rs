//! CR 601, 608.2m: `CastSpell` with a free cost, then resolution. A permanent spell becomes a battlefield permanent, and an instant or sorcery goes to its owner's graveyard.
//! The TODO on this arm in commands.rs tracks the cost flow, which cast_spell_payment.rs covers.

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state as base_two_player_state,
};

fn land_definition() -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..minimal_card_definition(1, "Forest", "forest", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn instant_definition() -> CardDefinition {
    minimal_card_definition(2, "Shock", "shock", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
}

fn sorcery_definition() -> CardDefinition {
    minimal_card_definition(3, "Rampant Growth", "rampant_growth", ManaCost { symbols: Vec::new() }, CardTypeSet::SORCERY)
}

fn creature_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(4, "Grizzly Bears", "grizzly_bears", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

// Asserts the engine paused on `AwaitingPayment` before paying the empty plan CR 118.5a requires.
fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

// Each move mints a new ObjectId (CR 400.7), so tests read the last ZoneChange and not the original id.
fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

#[test]
fn cast_spell_moves_card_hand_to_stack() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("p0 holds priority, spell is in hand");
    let status = pay_free_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Hand, ZoneKind::Stack));
    assert!(engine.state().object(card).is_none(), "old id retired per CR 400.7");
    assert_eq!(engine.state().object(new_id).unwrap().zone, ZoneKind::Stack);
    assert!(!engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.contains(&card));
    assert_eq!(engine.state().stack.len(), 1);
    assert_eq!(engine.state().stack[0].stack_id, new_id);
}

#[test]
fn cast_spell_keeps_priority_with_caster() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap();
    drain(&mut engine);

    // CR 601.2i: the controller held priority before casting, so they get it back.
    assert_eq!(engine.state().priority_player, p0);
}

#[test]
fn cast_spell_breaks_pass_chain() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // p0 passes first, handing priority to p1, who then casts the instant.
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 1);

    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 0, "CastSpell must reset the pass chain");
}

#[test]
fn cast_spell_rejects_land() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn cast_spell_rejects_card_not_in_hand() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

#[test]
fn cast_sorcery_rejects_outside_main_phase() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.phase = Phase::Combat;
    state.objects.insert(card, card_object(card, sorcery_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn cast_instant_allowed_outside_main_phase_and_with_nonempty_stack() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    state.phase = Phase::Combat;
    let already_on_stack = ObjectId(0);
    state.objects.insert(
        already_on_stack,
        card_object(already_on_stack, sorcery_definition(), p0, ZoneKind::Stack),
    );
    state.stack.push(StackObject {
        stack_id: already_on_stack,
        kind: StackEntryKind::Spell,
        source: already_on_stack,
        source_lki: None,
        owner: p0,
        controller: p0,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("p0 holds priority - instant can be cast despite combat phase + nonempty stack");
    let status = pay_free_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 2);
}

// CR 117.4, 608.2m
// Two passes in a row resolve the top object, and the instant ends in its owner's graveyard.
#[test]
fn cast_instant_resolves_to_owners_graveyard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap();
    pay_free_and_drain(&mut engine);
    let (stack_id, ..) = last_zone_change(&engine);
    assert_eq!(engine.state().stack.len(), 1);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "spell resolved off the stack");
    let (resolved_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Graveyard));
    assert!(engine.state().object(stack_id).is_none(), "stack-side id retired per CR 400.7");
    let resolved = engine.state().object(resolved_id).unwrap();
    assert_eq!(resolved.zone, ZoneKind::Graveyard);
    assert!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.contains(&resolved_id));
}

// CR 608.2m
// A permanent spell goes to the battlefield.
#[test]
fn cast_creature_resolves_to_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, creature_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap();
    pay_free_and_drain(&mut engine);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert!(engine.state().stack.is_empty());
    let (resolved_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Battlefield));
    let resolved = engine.state().object(resolved_id).unwrap();
    assert_eq!(resolved.zone, ZoneKind::Battlefield);
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&resolved_id));
}
