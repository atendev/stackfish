//! CR 701.9: `AbilityEffect::Discard` makes the target player discard `count` cards by their own choice (Mind Rot).
//! `DiscardRandom` has no choice and `Game.Ops.DiscardChosen` is chosen by the ability's controller.

use std::collections::HashMap;

use stackfish_engine::engine::{DiscardChoiceError, Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
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

fn mind_rot_definition(count: u32) -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Mind Rot".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Target player discards two cards.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_mind_rot".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Player { controller: None },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Discard { count }),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn madness_redirect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Hand),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    }
}

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state
        .zone_mut(ZoneKind::Hand, Some(owner))
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
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
fn the_targeted_player_chooses_which_single_card_to_discard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let keep = ObjectId(1);
    let discard_me = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, mind_rot_definition(1), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(keep, creature_definition(1, "Keep"), p1, ZoneKind::Hand),
    );
    put_in_hand(
        &mut state,
        card_object(
            discard_me,
            creature_definition(2, "Discard Me"),
            p1,
            ZoneKind::Hand,
        ),
    );

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let (player, candidates, remaining_count) = match status {
        StepStatus::AwaitingDiscardChoice {
            player,
            candidates,
            remaining_count,
        } => (player, candidates, remaining_count),
        other => panic!("expected AwaitingDiscardChoice, got {other:?}"),
    };
    assert_eq!(
        player, p1,
        "the targeted player is asked, not Mind Rot's caster"
    );
    assert_eq!(
        remaining_count, 0,
        "count: 1 means no more picks after this one"
    );
    assert_eq!(candidates.len(), 2);
    assert!(candidates.contains(&keep) && candidates.contains(&discard_me));

    engine
        .submit_discard_choice(p1, discard_me)
        .expect("discard_me is a real candidate");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().object(discard_me).is_none(),
        "the discarded card's id retired"
    );
    assert!(
        engine.state().object(keep).is_some(),
        "the un-chosen card stays in hand"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p1))
            .unwrap()
            .contents
            .len(),
        1
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p1))
            .unwrap()
            .contents
            .len(),
        1
    );
}

#[test]
fn count_two_asks_twice_and_cannot_repeat_a_candidate() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let first = ObjectId(1);
    let second = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, mind_rot_definition(2), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(first, creature_definition(1, "First"), p1, ZoneKind::Hand),
    );
    put_in_hand(
        &mut state,
        card_object(second, creature_definition(2, "Second"), p1, ZoneKind::Hand),
    );

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingDiscardChoice {
        candidates,
        remaining_count,
        ..
    } = status
    else {
        panic!("expected AwaitingDiscardChoice, got {status:?}");
    };
    assert_eq!(
        remaining_count, 1,
        "count: 2 means one more pick owed after this one"
    );
    assert_eq!(candidates.len(), 2);

    engine
        .submit_discard_choice(p1, first)
        .expect("first is a real candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingDiscardChoice {
        candidates,
        remaining_count,
        ..
    } = status
    else {
        panic!("expected a second AwaitingDiscardChoice, got {status:?}");
    };
    assert_eq!(remaining_count, 0, "this is the last pick");
    assert_eq!(
        candidates,
        vec![second],
        "first can't be offered again - it already left the hand"
    );

    // Re-submitting the already-discarded card must fail, so the candidate list is binding.
    let err = engine.submit_discard_choice(p1, first).unwrap_err();
    assert!(matches!(err, DiscardChoiceError::InvalidCandidate));

    engine
        .submit_discard_choice(p1, second)
        .expect("second is a real candidate");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p1))
            .unwrap()
            .contents
            .is_empty()
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p1))
            .unwrap()
            .contents
            .len(),
        2
    );
}

#[test]
fn an_empty_hand_resolves_with_no_pause() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, mind_rot_definition(2), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    // p1's hand is empty - nothing to discard.

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert!(engine.state().stack.is_empty());
}

#[test]
fn only_the_targeted_player_may_answer_the_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let hand_card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(card, mind_rot_definition(1), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(
            hand_card,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Hand,
        ),
    );

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_discard_choice(p0, hand_card).unwrap_err();
    assert!(
        matches!(err, DiscardChoiceError::NotCurrentPlayer),
        "Mind Rot's own caster can't answer for the target"
    );
    assert!(
        engine.state().object(hand_card).is_some(),
        "unanswered - nothing happened"
    );
}

#[test]
fn a_madness_redirect_on_the_discarded_card_still_applies() {
    // continue_targeted_discard reuses the replacement-checked pipeline and the CR 702.35a Madness marker that every discard producer uses.
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let madness_card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(card, mind_rot_definition(1), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(
            madness_card,
            creature_definition(1, "Madness Creature"),
            p1,
            ZoneKind::Hand,
        ),
    );
    state
        .replacement_effects
        .push(madness_redirect(madness_card));

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingDiscardChoice { .. }),
        "got {status:?}"
    );

    engine
        .submit_discard_choice(p1, madness_card)
        .expect("madness_card is a real candidate");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().object(madness_card).is_none(),
        "original id retired"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p1))
            .unwrap()
            .contents
            .is_empty(),
        "the redirect should have sent it to exile, not the graveyard"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .len(),
        1
    );
    let marked = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::MadnessExiled { .. }));
    assert!(
        marked,
        "expected a MadnessExiled marker, proving the shared discard pipeline was really used"
    );
}

#[test]
fn game_ops_discard_decodes_to_the_effect() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.Discard(2)
        end
        return s
    "#;
    let (effect, _handle) = runtime
        .build_operation(source)
        .expect("script should load")
        .expect("declares an operation");
    match effect {
        Some(AbilityEffect::Discard { count }) => assert_eq!(count, 2),
        other => panic!("expected Discard, got {other:?}"),
    }
}
