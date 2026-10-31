//! CR 700.2e: a player other than the controller chooses the mode. `CastSpell` is single-actor, so `ModeChoice::ExactlyChosenByOpponent` and `UpToChosenByOpponent` push the spell with `pending_mode_choice` set.
//! `ResolveStackTop` then pauses on `AwaitingModeChoice`, and `submit_mode_choice` (answered by the other player) patches the choice in.
// TODO: opponent-chosen modes with targets or additional costs, and games with more than two players (CR 700.2e)

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, ModeChoiceError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, player_state, two_player_state as base_two_player_state, zones,
};

fn chosen_by_opponent_definition(mode_choice: ModeChoice) -> CardDefinition {
    CardDefinition {
        definition_id: 60,
        name: "Test Chosen By Opponent".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose one - your opponent chooses - \
            * You gain 3 life. \
            * You shuffle your library."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_chosen_by_opponent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: vec![
            SpellMode {
                target_requirement: None,
                effect: Some(AbilityEffect::GainLife(3)),
                additional_cost: None,
                pawprint_weight: 0,
            },
            SpellMode {
                target_requirement: None,
                effect: Some(AbilityEffect::Shuffle),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        mode_choice,
    }
}

fn any_target_requirement() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn chosen_by_opponent_with_a_target_definition() -> CardDefinition {
    CardDefinition {
        modes: vec![
            SpellMode {
                target_requirement: None,
                effect: Some(AbilityEffect::GainLife(3)),
                additional_cost: None,
                pawprint_weight: 0,
            },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        ..chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(1))
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        // The library zone must exist for mode 1's Shuffle effect, even when empty.
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Library,
            ],
        ))
    }
}

fn cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes,
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes,
            targets,
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    })
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

#[test]
fn casting_with_modes_supplied_by_the_caster_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(1)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // The caster cannot supply the mode, which is deferred to the other player.
    let err = try_cast_spell(&mut engine, p0, card, vec![0], Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn a_mode_with_a_target_requirement_is_rejected_at_cast_time() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_with_a_target_definition(),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, Vec::new(), Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn a_three_player_game_is_rejected_at_cast_time() {
    let mut state = two_player_state();
    let p2 = PlayerId(2);
    state.players.push(player_state(p2));
    state.turn_order.push(p2);
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(1)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, Vec::new(), Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn casting_pauses_at_resolution_for_the_other_players_mode_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(1)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    match status {
        StepStatus::AwaitingModeChoice {
            player, min, max, ..
        } => {
            assert_eq!(player, p1, "CR 700.2e: p0's opponent chooses, not p0");
            assert_eq!((min, max), (1, 1));
        }
        other => panic!("expected AwaitingModeChoice, got {other:?}"),
    }
    // Not yet resolved - the spell is still paused, no life/hand change yet.
    assert_eq!(engine.state().player(p0).unwrap().life, 20);
}

#[test]
fn the_other_players_chosen_mode_resolves_for_the_caster() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(1)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    // p1 chooses mode 0 (GainLife(3)) though p0 is the controller and benefits.
    engine
        .submit_mode_choice(vec![0])
        .expect("p1 chooses mode 0, within range, no duplicates");
    drain(&mut engine);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        23,
        "p0 (the caster) still gets the effect - only the choice moved"
    );
}

#[test]
fn choice_count_outside_the_announced_range_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(1)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_mode_choice(Vec::new()).unwrap_err();
    assert!(matches!(err, ModeChoiceError::InvalidCount));
    let err = engine.submit_mode_choice(vec![0, 1]).unwrap_err();
    assert!(
        matches!(err, ModeChoiceError::InvalidCount),
        "ExactlyChosenByOpponent(1) allows only one"
    );
}

#[test]
fn duplicate_mode_choice_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::UpToChosenByOpponent(2)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_mode_choice(vec![0, 0]).unwrap_err();
    assert!(
        matches!(err, ModeChoiceError::DuplicateMode),
        "CR 700.2e doesn't grant 700.2d's own repeat exception"
    );
}

#[test]
fn choosing_two_modes_resolves_both() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            chosen_by_opponent_definition(ModeChoice::ExactlyChosenByOpponent(2)),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine
        .submit_mode_choice(vec![1, 0])
        .expect("order doesn't matter - resolved in card-print order");
    drain(&mut engine);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        23,
        "mode 0's GainLife(3) resolved"
    );
    assert!(
        engine.state().stack.is_empty(),
        "both modes resolved and the spell left the stack"
    );
}
