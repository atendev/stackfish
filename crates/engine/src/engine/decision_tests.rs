//! Unit tests for the `decision` facade, building pause states directly. Real flows are
//! covered in `tests/decision_answer_facade.rs`.

use std::collections::VecDeque;

use super::tests::{dummy_object, minimal_state, trigger};
use super::*;
use crate::commands::PaymentResolution;
use crate::decision::{
    Answer, Candidate, DecisionId, PartitionRest, Rejection, RejectionCode, Reply, ReplyShape,
};
use crate::replacement::ReplacementCandidate;
use crate::script::ScriptChoiceRequest;
use crate::types::ability::UnlessPaidConsequence;
use crate::types::card::ModeChoice;
use crate::types::command::{PlayerAction, PlayerActionKind};
use crate::types::cost::{CostSpec, PaymentPlan};
use crate::types::ids::{PlayerId, ScriptHandle};
use crate::types::mana::ManaCost;
use crate::types::target::TargetSpec;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn engine_in(process_state: ProcessState) -> Engine {
    let mut engine = Engine::new(minimal_state());
    engine.process_state = process_state;
    engine
}

fn obj(id: u64) -> Candidate {
    Candidate::Target(Target::Object(ObjectId(id)))
}

fn answer(decision: DecisionId, reply: Reply) -> Answer {
    Answer { decision, reply }
}

fn reject(result: Result<(), Rejection>, code: RejectionCode) -> Rejection {
    let rejection = result.expect_err("the Answer should have been rejected");
    assert_eq!(rejection.code, code, "message was: {}", rejection.message);
    rejection
}

fn legend_rule(candidates: &[u64]) -> ProcessState {
    ProcessState::AwaitingLegendRuleChoice {
        player: P0,
        candidates: candidates.iter().map(|&n| ObjectId(n)).collect(),
        then: SbaResumeThen::SbaLoop,
    }
}

fn commander_return() -> ProcessState {
    ProcessState::AwaitingCommanderReturnChoice {
        player: P0,
        commander: ObjectId(7),
        then: SbaResumeThen::SbaLoop,
    }
}

fn trigger_order(count: u64) -> ProcessState {
    ProcessState::AwaitTriggerOrderChoice {
        player: P0,
        group: (1..=count).map(|n| trigger(n, P0)).collect(),
        ordered: Vec::new(),
        pending_groups: VecDeque::new(),
    }
}

fn scry(cards: &[u64]) -> ProcessState {
    ProcessState::AwaitingScryChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        cards: cards.iter().map(|&n| ObjectId(n)).collect(),
    }
}

fn free_cost() -> CostSpec {
    CostSpec::mana_only(ManaCost {
        symbols: Vec::new(),
    })
}

// Building Decisions from pauses

#[test]
fn a_priority_window_is_an_action_decision_for_the_priority_player() {
    let mut engine = Engine::new(minimal_state());
    let decision = engine.open_decision().expect("priority is open");
    assert_eq!(decision.player, P0);
    assert!(matches!(decision.shape, ReplyShape::Action { .. }));
}

#[test]
fn no_decision_is_open_before_pregame_setup_runs() {
    let mut engine = Engine::new(minimal_state());
    engine.pregame_setup_done = false;
    assert!(engine.open_decision().is_none());
}

#[test]
fn running_and_finished_states_have_no_decision() {
    for process_state in [
        ProcessState::ResolveStackTop,
        ProcessState::SbaLoop,
        ProcessState::AdvanceTurnStep,
        ProcessState::CleanupSbaCheck,
        ProcessState::GameOver { winner: None },
    ] {
        assert!(engine_in(process_state).open_decision().is_none());
    }
}

#[test]
fn pick_pauses_list_explicit_candidates_with_bounds() {
    let decision = engine_in(legend_rule(&[3, 4])).open_decision().unwrap();
    let ReplyShape::Pick {
        candidates,
        min,
        max,
    } = decision.shape
    else {
        panic!("expected Pick")
    };
    assert_eq!(candidates, vec![obj(3), obj(4)]);
    assert_eq!((min, max), (1, 1));

    let devour = ProcessState::AwaitingDevourChoice {
        object: ObjectId(1),
        controller: P0,
        per_creature: 1,
        candidates: vec![ObjectId(8), ObjectId(9)],
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let ReplyShape::Pick { min, max, .. } = engine_in(devour).open_decision().unwrap().shape else {
        panic!("expected Pick")
    };
    assert_eq!(
        (min, max),
        (0, 2),
        "Devour takes any subset, including none"
    );

    let search = ProcessState::AwaitingSearchChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        candidates: vec![ObjectId(8), ObjectId(9), ObjectId(10)],
        count: 2,
        destination: crate::types::ability::SearchDestination::Hand,
        shuffle_after: true,
        reveal: false,
    };
    let ReplyShape::Pick { min, max, .. } = engine_in(search).open_decision().unwrap().shape else {
        panic!("expected Pick")
    };
    assert_eq!(
        (min, max),
        (0, 2),
        "CR 701.23b: fewer than count, even none, is legal"
    );
}

#[test]
fn a_search_lists_its_candidates_by_id_and_not_in_library_order() {
    // The state holds the candidates in library order, top card first.
    let search = ProcessState::AwaitingSearchChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        candidates: vec![ObjectId(10), ObjectId(8), ObjectId(9)],
        count: 1,
        destination: crate::types::ability::SearchDestination::Hand,
        shuffle_after: true,
        reveal: false,
    };
    let ReplyShape::Pick { candidates, .. } = engine_in(search).open_decision().unwrap().shape
    else {
        panic!("expected Pick")
    };
    assert_eq!(
        candidates,
        vec![obj(8), obj(9), obj(10)],
        "CR 401.2: the list reveals no library order"
    );
}

#[test]
fn a_replacement_choice_offers_replacement_candidates() {
    let process_state = ProcessState::AwaitingReplacementChoice {
        affected_player: P0,
        pending_event: GameEventKind::ScriptError {
            message: String::new(),
        },
        candidates: vec![
            ReplacementCandidate::Registered(ObjectId(3)),
            ReplacementCandidate::Protection(ObjectId(3)),
        ],
        excluded: Vec::new(),
        resume: ReplacementResumeTarget::ReplacementCheck {
            remaining: VecDeque::new(),
            checked: Vec::new(),
        },
    };
    let ReplyShape::Pick { candidates, .. } =
        engine_in(process_state).open_decision().unwrap().shape
    else {
        panic!("expected Pick")
    };
    assert_eq!(
        candidates,
        vec![
            Candidate::Replacement(ReplacementCandidate::Registered(ObjectId(3))),
            Candidate::Replacement(ReplacementCandidate::Protection(ObjectId(3))),
        ],
        "a registered effect and protection can share an object id and stay distinct"
    );
}

#[test]
fn yes_no_pauses() {
    let mulligan = ProcessState::AwaitingMulliganDecision { player: P0 };
    for process_state in [commander_return(), mulligan] {
        assert!(matches!(
            engine_in(process_state).open_decision().unwrap().shape,
            ReplyShape::YesNo
        ));
    }
    let may = ProcessState::AwaitingScriptChoice {
        thread: ScriptHandle(0),
        request: ScriptChoiceRequest::MayChoose,
        stack_id: ObjectId(50),
        source: ObjectId(1),
        controller: P0,
        events_so_far: Vec::new(),
        each_player: None,
    };
    assert!(matches!(
        engine_in(may).open_decision().unwrap().shape,
        ReplyShape::YesNo
    ));
}

#[test]
fn trigger_pauses_list_the_group_and_the_mode_bounds() {
    let ReplyShape::Order { items } = engine_in(trigger_order(3)).open_decision().unwrap().shape
    else {
        panic!("expected Order")
    };
    assert_eq!(items.len(), 3);

    let modal = ProcessState::AwaitingTriggerModeChoice {
        trigger: trigger(1, P0),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(2),
        remaining: VecDeque::new(),
        chooser: P1,
    };
    let decision = engine_in(modal).open_decision().unwrap();
    assert_eq!(
        decision.player, P1,
        "CR 700.2e: the chooser answers, not the controller"
    );
    assert!(matches!(
        decision.shape,
        ReplyShape::Modes {
            min: 2,
            max: 2,
            repeatable: false,
            ..
        }
    ));

    let targets = ProcessState::AwaitingTriggerTargets {
        trigger: trigger(1, P0),
        spec: TargetSpec::Multiple(Vec::new()),
        candidates: vec![vec![Target::Object(ObjectId(5)), Target::Player(P1)]],
        remaining: VecDeque::new(),
        modal_effect: None,
    };
    let ReplyShape::Targets { candidates, .. } = engine_in(targets).open_decision().unwrap().shape
    else {
        panic!("expected Targets")
    };
    assert_eq!(
        candidates,
        vec![vec![Target::Object(ObjectId(5)), Target::Player(P1)]]
    );
}

#[test]
fn scry_and_surveil_are_partitions_with_different_destinations() {
    let ReplyShape::Partition { cards, rest } =
        engine_in(scry(&[1, 2])).open_decision().unwrap().shape
    else {
        panic!("expected Partition")
    };
    assert_eq!(
        (cards, rest),
        (vec![ObjectId(1), ObjectId(2)], PartitionRest::LibraryBottom)
    );

    let surveil = ProcessState::AwaitingSurveilChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        cards: vec![ObjectId(1)],
    };
    let ReplyShape::Partition { rest, .. } = engine_in(surveil).open_decision().unwrap().shape
    else {
        panic!("expected Partition")
    };
    assert_eq!(rest, PartitionRest::Graveyard);
}

#[test]
fn payment_pauses_name_the_payer() {
    let cast = ProcessState::AwaitingPayment {
        stack_id: ObjectId(50),
        payer: P1,
        cost: free_cost(),
        source: ObjectId(1),
        on_paid: PaymentResolution::StackObject,
    };
    let decision = engine_in(cast).open_decision().unwrap();
    assert_eq!(decision.player, P1);
    assert!(matches!(decision.shape, ReplyShape::Payment { .. }));

    let unless_paid = ProcessState::AwaitingUnlessPaidChoice {
        payer_queue: VecDeque::from([P1, P0]),
        cost: free_cost(),
        otherwise: UnlessPaidConsequence::SacrificeSelf,
        target_id: None,
        source: ObjectId(1),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let decision = engine_in(unless_paid).open_decision().unwrap();
    assert_eq!(
        decision.player, P1,
        "the front of the payer queue answers first"
    );
    assert!(matches!(decision.shape, ReplyShape::Payment { .. }));
}

use crate::types::card::CardTypeSet;
use crate::types::cost::{AdditionalCost, CostPayment, FilterController, FilterSpec};
use crate::types::player::PlayerState;
use crate::types::zone::ZoneKind;

fn filter(card_types: Option<CardTypeSet>) -> FilterSpec {
    FilterSpec {
        card_types,
        supertypes: None,
        subtypes: None,
        controller: None,
        random: false,
        extra: None,
    }
}

// Objects 1 to 8 cover each zone, controller, tapped state and type a filtered cost checks.
fn cost_state() -> GameState {
    let mut state = minimal_state();
    state.players.push(PlayerState::new(P1, 20));
    let objects = [
        (1, P0, ZoneKind::Battlefield, CardTypeSet::CREATURE, false),
        (2, P0, ZoneKind::Battlefield, CardTypeSet::CREATURE, true),
        (3, P1, ZoneKind::Battlefield, CardTypeSet::CREATURE, false),
        (4, P0, ZoneKind::Battlefield, CardTypeSet::ARTIFACT, false),
        (5, P0, ZoneKind::Hand, CardTypeSet::CREATURE, false),
        (6, P1, ZoneKind::Hand, CardTypeSet::CREATURE, false),
        (7, P0, ZoneKind::Graveyard, CardTypeSet::SORCERY, false),
        (8, P1, ZoneKind::Graveyard, CardTypeSet::SORCERY, false),
    ];
    for (id, controller, zone, card_types, tapped) in objects {
        let mut object = dummy_object(ObjectId(id), controller);
        object.zone = zone;
        object.tapped = tapped;
        object.definition.card_types = card_types;
        state.objects.insert(ObjectId(id), object);
    }
    state
}

fn filtered_costs() -> Vec<AdditionalCost> {
    vec![
        AdditionalCost::Sacrifice {
            filter: filter(Some(CardTypeSet::CREATURE)),
        },
        AdditionalCost::Discard {
            filter: filter(None),
        },
        AdditionalCost::TapPermanents {
            filter: FilterSpec {
                controller: Some(FilterController::You),
                ..filter(Some(CardTypeSet::CREATURE))
            },
            count: 1,
        },
        AdditionalCost::ExileFromGraveyard {
            filter: filter(None),
            count: 1,
        },
    ]
}

fn cost_with(additional: Vec<AdditionalCost>) -> CostSpec {
    CostSpec {
        additional,
        ..free_cost()
    }
}

#[test]
fn payment_lists_the_objects_each_filtered_cost_may_name() {
    let mut additional = filtered_costs();
    additional.push(AdditionalCost::TapSelf);
    additional.push(AdditionalCost::LifePayment { amount: 2 });
    let mut engine = Engine::new(cost_state());
    engine.process_state = ProcessState::AwaitingPayment {
        stack_id: ObjectId(50),
        payer: P0,
        cost: cost_with(additional),
        source: ObjectId(1),
        on_paid: PaymentResolution::StackObject,
    };
    let ReplyShape::Payment { cost, eligible, .. } = engine.open_decision().unwrap().shape else {
        panic!("expected Payment")
    };
    let ids = |list: &[u64]| list.iter().map(|&i| ObjectId(i)).collect::<Vec<_>>();
    assert_eq!(eligible.len(), cost.additional.len());
    assert_eq!(
        eligible,
        vec![
            ids(&[1, 2]),
            ids(&[5]),
            ids(&[1]),
            ids(&[7]),
            Vec::new(),
            Vec::new()
        ]
    );
}

#[test]
fn an_unless_paid_payment_lists_objects_for_the_payer_at_the_front() {
    let mut engine = Engine::new(cost_state());
    engine.process_state = ProcessState::AwaitingUnlessPaidChoice {
        payer_queue: VecDeque::from([P1, P0]),
        cost: cost_with(filtered_costs()),
        otherwise: UnlessPaidConsequence::SacrificeSelf,
        target_id: None,
        source: ObjectId(1),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let ReplyShape::Payment { eligible, .. } = engine.open_decision().unwrap().shape else {
        panic!("expected Payment")
    };
    assert_eq!(
        eligible,
        vec![
            vec![ObjectId(3)],
            vec![ObjectId(6)],
            vec![ObjectId(3)],
            vec![ObjectId(8)]
        ]
    );
}

// A one-object plan for each filtered cost validates exactly when the Decision lists the object.
#[test]
fn every_listed_object_pays_and_no_other_object_does() {
    let engine = Engine::new(cost_state());
    for additional in filtered_costs() {
        let listed = additional.eligible(engine.state(), engine.script_runtime(), P0);
        for id in (1..=8).map(ObjectId) {
            let payment = match additional {
                AdditionalCost::Sacrifice { .. } => CostPayment::Sacrifice(id),
                AdditionalCost::Discard { .. } => CostPayment::Discard(id),
                AdditionalCost::TapPermanents { .. } => CostPayment::Tap(id),
                _ => CostPayment::Exile(id),
            };
            let plan = PaymentPlan {
                payments: vec![payment],
            };
            let paid = plan.validate(
                &cost_with(vec![additional.clone()]),
                engine.state(),
                engine.script_runtime(),
                P0,
                ObjectId(99),
            );
            assert_eq!(
                paid,
                listed.contains(&id),
                "{additional:?} with object {id:?}"
            );
        }
    }
}

// CR 702.35a
#[test]
fn a_madness_decision_lists_the_cast_announcement() {
    let mut state = minimal_state();
    let mut card = dummy_object(ObjectId(4), P0);
    card.zone = ZoneKind::Exile;
    card.definition.card_types = CardTypeSet::SORCERY;
    card.definition.mana_cost = crate::types::mana::ManaCost {
        symbols: vec![crate::types::mana::ManaSymbol::X],
    };
    state.objects.insert(ObjectId(4), card);
    let mut engine = Engine::new(state);
    engine.process_state = ProcessState::AwaitingMadnessChoice {
        player: P0,
        card: ObjectId(4),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let ReplyShape::MayCast {
        options: Some(options),
        ..
    } = engine.open_decision().unwrap().shape
    else {
        panic!("expected MayCast with options")
    };
    assert_eq!(
        options.targets,
        crate::decision::EntryTargets::Fixed(Vec::new())
    );
    assert!(options.modes.is_none());
    assert!(
        !options.x,
        "madness pays an alternative cost, so X is not announced"
    );
}

#[test]
fn madness_and_cascade_offer_a_cast() {
    let madness = ProcessState::AwaitingMadnessChoice {
        player: P0,
        card: ObjectId(4),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    assert!(matches!(
        engine_in(madness).open_decision().unwrap().shape,
        ReplyShape::MayCast {
            card: Some(ObjectId(4)),
            ..
        }
    ));

    let cascade = ProcessState::AwaitingCascadeChoice {
        player: P0,
        candidate: None,
        exiled: Vec::new(),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    assert!(matches!(
        engine_in(cascade).open_decision().unwrap().shape,
        ReplyShape::MayCast {
            card: None,
            options: None
        }
    ));
}

// Decision ids

#[test]
fn ids_start_at_one_and_repeat_until_the_decision_is_answered() {
    let mut engine = engine_in(commander_return());
    let first = engine.open_decision().unwrap().id;
    assert_eq!(first, DecisionId(1));
    assert_eq!(
        engine.open_decision().unwrap().id,
        first,
        "asking again does not allocate a new id"
    );

    engine
        .answer(P0, answer(first, Reply::YesNo(false)))
        .expect("a yes/no is accepted");

    // The next pause is the same kind, and still gets a fresh id.
    engine.process_state = commander_return();
    assert_eq!(engine.open_decision().unwrap().id, DecisionId(2));
}

// Rejections change nothing

#[test]
fn answering_with_no_decision_open_is_not_open() {
    let mut engine = engine_in(ProcessState::SbaLoop);
    reject(
        engine.answer(P0, answer(DecisionId(1), Reply::YesNo(true))),
        RejectionCode::NotOpen,
    );
}

#[test]
fn a_stale_id_wrong_seat_and_wrong_shape_leave_the_decision_open() {
    let mut engine = engine_in(trigger_order(2));
    let id = engine.open_decision().unwrap().id;
    let ledger_before = engine.state().ledger.len();

    reject(
        engine.answer(P0, answer(DecisionId(id.0 + 1), Reply::Order(vec![0, 1]))),
        RejectionCode::StaleDecision,
    );
    reject(
        engine.answer(P1, answer(id, Reply::Order(vec![0, 1]))),
        RejectionCode::WrongSeat,
    );
    reject(
        engine.answer(P0, answer(id, Reply::YesNo(true))),
        RejectionCode::WrongAnswerShape,
    );

    assert!(matches!(
        engine.process_state,
        ProcessState::AwaitTriggerOrderChoice { .. }
    ));
    assert_eq!(engine.state().ledger.len(), ledger_before);
    assert_eq!(engine.open_decision().unwrap().id, id);
}

#[test]
fn pick_replies_outside_the_candidates_or_bounds_are_invalid_choices() {
    let mut engine = engine_in(legend_rule(&[3, 4]));
    let id = engine.open_decision().unwrap().id;

    let cases: Vec<(&str, Vec<Candidate>)> = vec![
        ("not a candidate", vec![obj(99)]),
        (
            "a player where objects are offered",
            vec![Candidate::Target(Target::Player(P1))],
        ),
        ("too few", vec![]),
        ("too many", vec![obj(3), obj(4)]),
        (
            "a replacement candidate",
            vec![Candidate::Replacement(ReplacementCandidate::Registered(
                ObjectId(3),
            ))],
        ),
    ];
    for (label, chosen) in cases {
        let result = engine.answer(P0, answer(id, Reply::Pick(chosen)));
        assert_eq!(
            result.expect_err(label).code,
            RejectionCode::InvalidChoice,
            "{label}"
        );
        assert!(
            matches!(
                engine.process_state,
                ProcessState::AwaitingLegendRuleChoice { .. }
            ),
            "{label}"
        );
    }
}

#[test]
fn a_pick_cannot_name_the_same_candidate_twice() {
    let process_state = ProcessState::AwaitingDevourChoice {
        object: ObjectId(1),
        controller: P0,
        per_creature: 1,
        candidates: vec![ObjectId(8), ObjectId(9)],
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let mut engine = engine_in(process_state);
    let id = engine.open_decision().unwrap().id;
    reject(
        engine.answer(P0, answer(id, Reply::Pick(vec![obj(8), obj(8)]))),
        RejectionCode::InvalidChoice,
    );
    assert!(matches!(
        engine.process_state,
        ProcessState::AwaitingDevourChoice { .. }
    ));
}

#[test]
fn order_replies_must_be_a_permutation_of_the_indices() {
    let mut engine = engine_in(trigger_order(2));
    let id = engine.open_decision().unwrap().id;
    for bad in [vec![0, 0], vec![0, 5], vec![0], vec![0, 1, 1]] {
        reject(
            engine.answer(P0, answer(id, Reply::Order(bad))),
            RejectionCode::InvalidChoice,
        );
    }
    assert!(matches!(
        engine.process_state,
        ProcessState::AwaitTriggerOrderChoice { .. }
    ));
}

#[test]
fn an_order_reply_tells_identical_triggers_apart_by_index() {
    // Two pending triggers with the same source and ability are equal, so an
    // Answer by value could not say which goes first.
    let twin = trigger(1, P0);
    let other = trigger(2, P0);
    let process_state = ProcessState::AwaitTriggerOrderChoice {
        player: P0,
        group: vec![twin, other, twin],
        ordered: Vec::new(),
        pending_groups: VecDeque::new(),
    };
    let mut engine = engine_in(process_state);
    let id = engine.open_decision().unwrap().id;
    engine
        .answer(P0, answer(id, Reply::Order(vec![1, 2, 0])))
        .expect("a permutation is accepted");
    let ProcessState::PutTriggersApnap { ordered, .. } = &engine.process_state else {
        panic!("expected the trigger pipeline to resume")
    };
    assert_eq!(ordered, &vec![other, twin, twin]);
}

#[test]
fn a_trigger_targets_reply_needs_one_list_per_word() {
    let process_state = ProcessState::AwaitingTriggerTargets {
        trigger: trigger(1, P0),
        spec: TargetSpec::Multiple(Vec::new()),
        candidates: vec![vec![Target::Object(ObjectId(5))], vec![Target::Player(P1)]],
        remaining: VecDeque::new(),
        modal_effect: None,
    };
    let mut engine = engine_in(process_state);
    let id = engine.open_decision().unwrap().id;
    reject(
        engine.answer(
            P0,
            answer(id, Reply::Targets(vec![vec![Target::Object(ObjectId(5))]])),
        ),
        RejectionCode::InvalidChoice,
    );
}

#[test]
fn engine_validation_failures_map_to_invalid_choice_and_keep_the_pause() {
    let mut engine = engine_in(scry(&[1, 2, 3]));
    let id = engine.open_decision().unwrap().id;
    // Card 3 is missing from both piles.
    let reply = Reply::Partition {
        top: vec![ObjectId(1)],
        rest: vec![ObjectId(2)],
    };
    reject(
        engine.answer(P0, answer(id, reply)),
        RejectionCode::InvalidChoice,
    );
    assert!(matches!(
        engine.process_state,
        ProcessState::AwaitingScryChoice { .. }
    ));
    assert_eq!(engine.open_decision().unwrap().id, id);
}

// Routing accepted Answers

#[test]
fn a_yes_no_routes_to_the_commander_return_entry_point() {
    let mut engine = engine_in(commander_return());
    let id = engine.open_decision().unwrap().id;
    engine.answer(P0, answer(id, Reply::YesNo(false))).unwrap();
    assert!(
        engine
            .state()
            .commander_return_offered
            .contains(&ObjectId(7))
    );
    assert!(!matches!(
        engine.process_state,
        ProcessState::AwaitingCommanderReturnChoice { .. }
    ));
}

#[test]
fn a_partition_routes_to_scry() {
    let mut engine = engine_in(scry(&[1, 2, 3]));
    let id = engine.open_decision().unwrap().id;
    let reply = Reply::Partition {
        top: vec![ObjectId(2)],
        rest: vec![ObjectId(1), ObjectId(3)],
    };
    engine.answer(P0, answer(id, reply)).unwrap();
    assert!(!matches!(
        engine.process_state,
        ProcessState::AwaitingScryChoice { .. }
    ));
}

#[test]
fn a_priority_action_from_another_player_is_the_wrong_seat() {
    let mut engine = Engine::new(minimal_state());
    let id = engine.open_decision().unwrap().id;
    let other = PlayerAction::new(P1, PlayerActionKind::PassPriority);
    reject(
        engine.answer(P0, answer(id, Reply::Action(other))),
        RejectionCode::WrongSeat,
    );
    assert!(matches!(engine.process_state, ProcessState::PriorityWindow));

    let pass = PlayerAction::new(P0, PlayerActionKind::PassPriority);
    engine.answer(P0, answer(id, Reply::Action(pass))).unwrap();
    assert!(matches!(
        engine.process_state,
        ProcessState::PendingAction(_)
    ));
}

#[test]
fn cannot_pay_routes_to_unwind_payment() {
    let cast = ProcessState::AwaitingPayment {
        stack_id: ObjectId(50),
        payer: P0,
        cost: free_cost(),
        source: ObjectId(1),
        on_paid: PaymentResolution::StackObject,
    };
    let mut engine = engine_in(cast);
    let id = engine.open_decision().unwrap().id;
    // The synthetic state has no stack object, which `unwind_payment` treats as already unwound.
    engine.answer(P0, answer(id, Reply::Payment(None))).unwrap();
    assert!(!matches!(
        engine.process_state,
        ProcessState::AwaitingPayment { .. }
    ));
}

#[test]
fn a_payment_plan_that_does_not_cover_the_cost_is_invalid_and_keeps_the_pause() {
    let cost = CostSpec::mana_only(ManaCost {
        symbols: vec![crate::types::mana::ManaSymbol::Atom(
            crate::types::mana::AtomicManaSymbol::Generic(2),
        )],
    });
    let cast = ProcessState::AwaitingPayment {
        stack_id: ObjectId(50),
        payer: P0,
        cost,
        source: ObjectId(1),
        on_paid: PaymentResolution::StackObject,
    };
    let mut engine = engine_in(cast);
    let id = engine.open_decision().unwrap().id;
    let rejection = reject(
        engine.answer(
            P0,
            answer(
                id,
                Reply::Payment(Some(PaymentPlan {
                    payments: Vec::new(),
                })),
            ),
        ),
        RejectionCode::InvalidChoice,
    );
    assert_eq!(rejection.message, "the payment does not cover the cost");
    assert!(matches!(
        engine.process_state,
        ProcessState::AwaitingPayment { .. }
    ));
}

fn stack_entry(
    stack_id: u64,
    kind: crate::types::stack::StackEntryKind,
) -> crate::types::stack::StackObject {
    crate::types::stack::StackObject::new(
        ObjectId(stack_id),
        kind,
        ObjectId(1),
        P0,
        P0,
        Vec::new(),
        Vec::new(),
        None,
        None,
        None,
        false,
        false,
        ScriptHandle(0),
        None,
        None,
        None,
        None,
    )
}

// CR 113.9
#[test]
fn a_script_pick_lists_abilities_on_the_stack_but_not_spells() {
    use crate::types::stack::StackEntryKind;
    let mut state = minimal_state();
    state
        .objects
        .insert(ObjectId(1), dummy_object(ObjectId(1), P0));
    state
        .stack
        .push(stack_entry(60, StackEntryKind::TriggeredAbility));
    state.stack.push(stack_entry(61, StackEntryKind::Spell));
    let mut engine = Engine::new(state);
    engine.process_state = ProcessState::AwaitingScriptChoice {
        thread: ScriptHandle(0),
        request: ScriptChoiceRequest::SelectTarget {
            filter: crate::types::target::TargetFilter::Ability,
            exclude_ids: Vec::new(),
        },
        stack_id: ObjectId(50),
        source: ObjectId(1),
        controller: P0,
        events_so_far: Vec::new(),
        each_player: None,
    };
    let decision = engine.open_decision().unwrap();
    let ReplyShape::Pick { candidates, .. } = decision.shape else {
        panic!("expected a Pick");
    };
    assert_eq!(
        candidates,
        vec![Candidate::Target(Target::Ability(ObjectId(60)))]
    );

    let before = engine.state().clone();
    for unlisted in [ObjectId(61), ObjectId(62)] {
        reject(
            engine.answer(
                P0,
                answer(
                    decision.id,
                    Reply::Pick(vec![Candidate::Target(Target::Ability(unlisted))]),
                ),
            ),
            RejectionCode::InvalidChoice,
        );
        assert_eq!(engine.state(), &before);
    }
}

// CR 605.3a
#[test]
fn a_payment_lists_mana_abilities_only_while_the_payer_holds_priority() {
    use crate::types::ability::{AbilityEffect, AbilityKind, RegisteredAbility};
    use crate::types::ids::AbilityId;
    use crate::types::mana::{Color, ManaUnit};
    let mut state = minimal_state();
    state.players.push(PlayerState::new(P1, 20));
    let mut land = dummy_object(ObjectId(1), P1);
    land.zone = ZoneKind::Battlefield;
    land.definition.card_types = CardTypeSet::LAND;
    state.objects.insert(ObjectId(1), land);
    state.abilities.push(RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: ObjectId(1),
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Red)])),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);
    engine.process_state = ProcessState::AwaitingPayment {
        stack_id: ObjectId(50),
        payer: P1,
        cost: free_cost(),
        source: ObjectId(1),
        on_paid: PaymentResolution::StackObject,
    };
    let mana_abilities = |engine: &mut Engine| match engine.open_decision().unwrap().shape {
        ReplyShape::Payment { mana_abilities, .. } => mana_abilities,
        other => panic!("expected Payment, got {other:?}"),
    };
    engine.state.priority_player = P0;
    assert!(mana_abilities(&mut engine).is_empty());
    engine.state.priority_player = P1;
    let listed = mana_abilities(&mut engine);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source, ObjectId(1));
}

// CR 605.3a
#[test]
fn an_unless_paid_payment_lists_no_mana_abilities_and_refuses_an_action() {
    let unless_paid = ProcessState::AwaitingUnlessPaidChoice {
        payer_queue: VecDeque::from([P0]),
        cost: free_cost(),
        otherwise: UnlessPaidConsequence::SacrificeSelf,
        target_id: None,
        source: ObjectId(1),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let mut engine = engine_in(unless_paid);
    let decision = engine.open_decision().unwrap();
    let ReplyShape::Payment { mana_abilities, .. } = &decision.shape else {
        panic!("expected Payment");
    };
    assert!(mana_abilities.is_empty());
    let before = engine.state().clone();
    let activate = Reply::Action(PlayerAction::new(
        P0,
        PlayerActionKind::ActivateAbility {
            source: ObjectId(1),
            ability_index: 0,
            targets: Vec::new(),
            chosen_colors: Vec::new(),
            x_value: None,
        },
    ));
    reject(
        engine.answer(P0, answer(decision.id, activate)),
        RejectionCode::InvalidChoice,
    );
    assert_eq!(engine.state(), &before);
    assert_eq!(engine.open_decision().unwrap().id, decision.id);
}
