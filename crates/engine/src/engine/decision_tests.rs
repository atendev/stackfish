//! Unit tests for the `decision` facade, building pause states directly. Real flows are
//! covered in `tests/decision_answer_facade.rs`.

use std::collections::VecDeque;

use super::tests::{minimal_state, trigger};
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

fn reject(result: Result<(), Rejection>, code: RejectionCode) {
    let rejection = result.expect_err("the Answer should have been rejected");
    assert_eq!(rejection.code, code, "message was: {}", rejection.message);
}

fn legend_rule(candidates: &[u64]) -> ProcessState {
    ProcessState::AwaitingLegendRuleChoice {
        player: P0,
        candidates: candidates.iter().map(|&n| ObjectId(n)).collect(),
        then: SbaResumeThen::SbaLoop,
    }
}

fn commander_return() -> ProcessState {
    ProcessState::AwaitingCommanderReturnChoice { player: P0, commander: ObjectId(7), then: SbaResumeThen::SbaLoop }
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
    CostSpec::mana_only(ManaCost { symbols: Vec::new() })
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
    let ReplyShape::Pick { candidates, min, max } = decision.shape else { panic!("expected Pick") };
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
    assert_eq!((min, max), (0, 2), "Devour takes any subset, including none");

    let search = ProcessState::AwaitingSearchChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        candidates: vec![ObjectId(8), ObjectId(9), ObjectId(10)],
        count: 2,
        destination: crate::types::ability::SearchDestination::Hand,
        shuffle_after: true,
    };
    let ReplyShape::Pick { min, max, .. } = engine_in(search).open_decision().unwrap().shape else {
        panic!("expected Pick")
    };
    assert_eq!((min, max), (0, 2), "CR 701.23b: fewer than count, even none, is legal");
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
    };
    let ReplyShape::Pick { candidates, .. } = engine_in(search).open_decision().unwrap().shape else {
        panic!("expected Pick")
    };
    assert_eq!(candidates, vec![obj(8), obj(9), obj(10)], "CR 401.2: the list reveals no library order");
}

#[test]
fn a_replacement_choice_offers_replacement_candidates() {
    let process_state = ProcessState::AwaitingReplacementChoice {
        affected_player: P0,
        pending_event: GameEventKind::ScriptError { message: String::new() },
        candidates: vec![ReplacementCandidate::Registered(ObjectId(3)), ReplacementCandidate::Protection(ObjectId(3))],
        excluded: Vec::new(),
        resume: ReplacementResumeTarget::ReplacementCheck { remaining: VecDeque::new(), checked: Vec::new() },
    };
    let ReplyShape::Pick { candidates, .. } = engine_in(process_state).open_decision().unwrap().shape else {
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
        assert!(matches!(engine_in(process_state).open_decision().unwrap().shape, ReplyShape::YesNo));
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
    assert!(matches!(engine_in(may).open_decision().unwrap().shape, ReplyShape::YesNo));
}

#[test]
fn trigger_pauses_list_the_group_and_the_mode_bounds() {
    let ReplyShape::Order { items } = engine_in(trigger_order(3)).open_decision().unwrap().shape else {
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
    assert_eq!(decision.player, P1, "CR 700.2e: the chooser answers, not the controller");
    assert!(matches!(decision.shape, ReplyShape::Modes { min: 2, max: 2, repeatable: false, .. }));

    let targets = ProcessState::AwaitingTriggerTargets {
        trigger: trigger(1, P0),
        spec: TargetSpec::Multiple(Vec::new()),
        candidates: vec![vec![Target::Object(ObjectId(5)), Target::Player(P1)]],
        remaining: VecDeque::new(),
        modal_effect: None,
    };
    let ReplyShape::Targets { candidates, .. } = engine_in(targets).open_decision().unwrap().shape else {
        panic!("expected Targets")
    };
    assert_eq!(candidates, vec![vec![Target::Object(ObjectId(5)), Target::Player(P1)]]);
}

#[test]
fn scry_and_surveil_are_partitions_with_different_destinations() {
    let ReplyShape::Partition { cards, rest } = engine_in(scry(&[1, 2])).open_decision().unwrap().shape else {
        panic!("expected Partition")
    };
    assert_eq!((cards, rest), (vec![ObjectId(1), ObjectId(2)], PartitionRest::LibraryBottom));

    let surveil = ProcessState::AwaitingSurveilChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        cards: vec![ObjectId(1)],
    };
    let ReplyShape::Partition { rest, .. } = engine_in(surveil).open_decision().unwrap().shape else {
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
    assert_eq!(decision.player, P1, "the front of the payer queue answers first");
    assert!(matches!(decision.shape, ReplyShape::Payment { .. }));
}

#[test]
fn madness_and_cascade_offer_a_cast() {
    let madness =
        ProcessState::AwaitingMadnessChoice { player: P0, card: ObjectId(4), stack_id: ObjectId(50), events_so_far: Vec::new() };
    assert!(matches!(
        engine_in(madness).open_decision().unwrap().shape,
        ReplyShape::MayCast { card: Some(ObjectId(4)) }
    ));

    let cascade = ProcessState::AwaitingCascadeChoice {
        player: P0,
        candidate: None,
        exiled: Vec::new(),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    assert!(matches!(engine_in(cascade).open_decision().unwrap().shape, ReplyShape::MayCast { card: None }));
}

// Decision ids

#[test]
fn ids_start_at_one_and_repeat_until_the_decision_is_answered() {
    let mut engine = engine_in(commander_return());
    let first = engine.open_decision().unwrap().id;
    assert_eq!(first, DecisionId(1));
    assert_eq!(engine.open_decision().unwrap().id, first, "asking again does not allocate a new id");

    engine.answer(P0, answer(first, Reply::YesNo(false))).expect("a yes/no is accepted");

    // The next pause is the same kind, and still gets a fresh id.
    engine.process_state = commander_return();
    assert_eq!(engine.open_decision().unwrap().id, DecisionId(2));
}

// Rejections change nothing

#[test]
fn answering_with_no_decision_open_is_not_open() {
    let mut engine = engine_in(ProcessState::SbaLoop);
    reject(engine.answer(P0, answer(DecisionId(1), Reply::YesNo(true))), RejectionCode::NotOpen);
}

#[test]
fn a_stale_id_wrong_seat_and_wrong_shape_leave_the_decision_open() {
    let mut engine = engine_in(trigger_order(2));
    let id = engine.open_decision().unwrap().id;
    let ledger_before = engine.state().ledger.len();

    reject(engine.answer(P0, answer(DecisionId(id.0 + 1), Reply::Order(vec![0, 1]))), RejectionCode::StaleDecision);
    reject(engine.answer(P1, answer(id, Reply::Order(vec![0, 1]))), RejectionCode::WrongSeat);
    reject(engine.answer(P0, answer(id, Reply::YesNo(true))), RejectionCode::WrongAnswerShape);

    assert!(matches!(engine.process_state, ProcessState::AwaitTriggerOrderChoice { .. }));
    assert_eq!(engine.state().ledger.len(), ledger_before);
    assert_eq!(engine.open_decision().unwrap().id, id);
}

#[test]
fn pick_replies_outside_the_candidates_or_bounds_are_invalid_choices() {
    let mut engine = engine_in(legend_rule(&[3, 4]));
    let id = engine.open_decision().unwrap().id;

    let cases: Vec<(&str, Vec<Candidate>)> = vec![
        ("not a candidate", vec![obj(99)]),
        ("a player where objects are offered", vec![Candidate::Target(Target::Player(P1))]),
        ("too few", vec![]),
        ("too many", vec![obj(3), obj(4)]),
        ("a replacement candidate", vec![Candidate::Replacement(ReplacementCandidate::Registered(ObjectId(3)))]),
    ];
    for (label, chosen) in cases {
        let result = engine.answer(P0, answer(id, Reply::Pick(chosen)));
        assert_eq!(result.expect_err(label).code, RejectionCode::InvalidChoice, "{label}");
        assert!(matches!(engine.process_state, ProcessState::AwaitingLegendRuleChoice { .. }), "{label}");
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
    reject(engine.answer(P0, answer(id, Reply::Pick(vec![obj(8), obj(8)]))), RejectionCode::InvalidChoice);
    assert!(matches!(engine.process_state, ProcessState::AwaitingDevourChoice { .. }));
}

#[test]
fn order_replies_must_be_a_permutation_of_the_indices() {
    let mut engine = engine_in(trigger_order(2));
    let id = engine.open_decision().unwrap().id;
    for bad in [vec![0, 0], vec![0, 5], vec![0], vec![0, 1, 1]] {
        reject(engine.answer(P0, answer(id, Reply::Order(bad))), RejectionCode::InvalidChoice);
    }
    assert!(matches!(engine.process_state, ProcessState::AwaitTriggerOrderChoice { .. }));
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
    engine.answer(P0, answer(id, Reply::Order(vec![1, 2, 0]))).expect("a permutation is accepted");
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
        engine.answer(P0, answer(id, Reply::Targets(vec![vec![Target::Object(ObjectId(5))]]))),
        RejectionCode::InvalidChoice,
    );
}

#[test]
fn engine_validation_failures_map_to_invalid_choice_and_keep_the_pause() {
    let mut engine = engine_in(scry(&[1, 2, 3]));
    let id = engine.open_decision().unwrap().id;
    // Card 3 is missing from both piles.
    let reply = Reply::Partition { top: vec![ObjectId(1)], rest: vec![ObjectId(2)] };
    reject(engine.answer(P0, answer(id, reply)), RejectionCode::InvalidChoice);
    assert!(matches!(engine.process_state, ProcessState::AwaitingScryChoice { .. }));
    assert_eq!(engine.open_decision().unwrap().id, id);
}

// Routing accepted Answers

#[test]
fn a_yes_no_routes_to_the_commander_return_entry_point() {
    let mut engine = engine_in(commander_return());
    let id = engine.open_decision().unwrap().id;
    engine.answer(P0, answer(id, Reply::YesNo(false))).unwrap();
    assert!(engine.state().commander_return_offered.contains(&ObjectId(7)));
    assert!(!matches!(engine.process_state, ProcessState::AwaitingCommanderReturnChoice { .. }));
}

#[test]
fn a_partition_routes_to_scry() {
    let mut engine = engine_in(scry(&[1, 2, 3]));
    let id = engine.open_decision().unwrap().id;
    let reply = Reply::Partition { top: vec![ObjectId(2)], rest: vec![ObjectId(1), ObjectId(3)] };
    engine.answer(P0, answer(id, reply)).unwrap();
    assert!(!matches!(engine.process_state, ProcessState::AwaitingScryChoice { .. }));
}

#[test]
fn a_priority_action_from_another_player_is_the_wrong_seat() {
    let mut engine = Engine::new(minimal_state());
    let id = engine.open_decision().unwrap().id;
    let other = PlayerAction::new(P1, PlayerActionKind::PassPriority);
    reject(engine.answer(P0, answer(id, Reply::Action(other))), RejectionCode::WrongSeat);
    assert!(matches!(engine.process_state, ProcessState::PriorityWindow));

    let pass = PlayerAction::new(P0, PlayerActionKind::PassPriority);
    engine.answer(P0, answer(id, Reply::Action(pass))).unwrap();
    assert!(matches!(engine.process_state, ProcessState::PendingAction(_)));
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
    assert!(!matches!(engine.process_state, ProcessState::AwaitingPayment { .. }));
}

#[test]
fn a_payment_plan_that_does_not_cover_the_cost_is_invalid_and_keeps_the_pause() {
    let cost = CostSpec::mana_only(ManaCost { symbols: vec![crate::types::mana::ManaSymbol::Atom(crate::types::mana::AtomicManaSymbol::Generic(2))] });
    let cast = ProcessState::AwaitingPayment {
        stack_id: ObjectId(50),
        payer: P0,
        cost,
        source: ObjectId(1),
        on_paid: PaymentResolution::StackObject,
    };
    let mut engine = engine_in(cast);
    let id = engine.open_decision().unwrap().id;
    reject(engine.answer(P0, answer(id, Reply::Payment(Some(PaymentPlan { payments: Vec::new() })))), RejectionCode::InvalidChoice);
    assert!(matches!(engine.process_state, ProcessState::AwaitingPayment { .. }));
}
