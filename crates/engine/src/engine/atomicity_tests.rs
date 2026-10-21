//! 024/025: a bad answer to a pause changes nothing. Each case builds one
//! pause directly (the way `decision_tests.rs` does), sends an answer the
//! engine must refuse, and compares `Engine::snapshot()` before and after.
//!
//! A hand-built pause is as good as a played one for this: a rejection
//! happens before the code reads anything beyond the pause's own fields. The
//! pauses a scripted game reaches (payment, discard, Madness, Cascade, Scry,
//! Search) also have tests that play to them: `tests/phase5_atomicity_submit.rs`
//! and the proof-game test in `crates/host`.
//!
//! Not covered here, because every answer is legal: `submit_commander_return_choice`
//! (either boolean) and the Madness and Cascade declines.

use std::collections::VecDeque;

use super::tests::{minimal_state, trigger};
use super::*;
use crate::commands::PaymentResolution;
use crate::replacement::ReplacementCandidate;
use crate::script::ScriptChoiceRequest;
use crate::types::ability::{AbilityEffect, CopyExceptions, CreatureTokenSpec, SearchDestination, TokenSpec, UnlessPaidConsequence};
use crate::types::card::{
    CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SpellMode, SupertypeSet,
};
use crate::types::command::{PlayerAction, PlayerActionKind};
use crate::types::cost::{CostSpec, PaymentPlan};
use crate::types::counter::CounterSet;
use crate::types::ids::{ObjectLineage, PlayerId, ScriptHandle, SequenceNo};
use crate::types::mana::{AtomicManaSymbol, ColorSet, ManaCost, ManaSymbol};
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::target::{TargetFilter, TargetSpec};
use crate::types::zone::ZoneKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// The refusal's `Debug` text, or `None` if the engine accepted the answer.
type Answer = Box<dyn Fn(&mut Engine) -> Option<String>>;

/// One bad answer: its name, the pause it is sent to, and a call that returns
/// the engine's refusal.
struct Case {
    name: &'static str,
    pause: Box<dyn Fn() -> ProcessState>,
    answer: Answer,
}

macro_rules! case {
    ($name:expr, $pause:expr, |$e:ident| $call:expr) => {
        Case { name: $name, pause: Box::new(move || $pause), answer: Box::new(move |$e: &mut Engine| $call.err().map(|error| format!("{error:?}"))) }
    };
}

fn ids(list: &[u64]) -> Vec<ObjectId> {
    list.iter().map(|&n| ObjectId(n)).collect()
}

fn definition(modes: Vec<SpellMode>, mode_choice: ModeChoice) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Charm".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("charm".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes,
        mode_choice,
    }
}

fn object(id: ObjectId, definition: CardDefinition) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner: P0,
        controller: P0,
        zone: ZoneKind::Stack,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        attached_to_player: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        copy_generation: 0,
        entered_via_escape: false,
    }
}

/// Mode 0 resolves anywhere. Mode 1 (Cascade) cannot be combined with another mode.
fn two_modes() -> Vec<SpellMode> {
    vec![
        SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(1)), additional_cost: None, pawprint_weight: 0 },
        SpellMode { target_requirement: None, effect: Some(AbilityEffect::Cascade), additional_cost: None, pawprint_weight: 0 },
    ]
}

fn free() -> CostSpec {
    CostSpec::mana_only(ManaCost { symbols: Vec::new() })
}

fn one_generic() -> CostSpec {
    CostSpec::mana_only(ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] })
}

fn action(player: PlayerId, kind: PlayerActionKind) -> PlayerAction {
    PlayerAction { player, kind }
}

fn script_pause(request: ScriptChoiceRequest) -> ProcessState {
    ProcessState::AwaitingScriptChoice {
        thread: ScriptHandle(0),
        request,
        stack_id: ObjectId(50),
        source: ObjectId(1),
        controller: P0,
        events_so_far: Vec::new(),
        each_player: None,
    }
}

fn cases() -> Vec<Case> {
    let scry = || ProcessState::AwaitingScryChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        cards: ids(&[1, 2]),
    };
    let surveil = || ProcessState::AwaitingSurveilChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        cards: ids(&[1, 2]),
    };
    let search = || ProcessState::AwaitingSearchChoice {
        stack_id: ObjectId(50),
        controller: P0,
        events_so_far: Vec::new(),
        candidates: ids(&[8, 9]),
        count: 1,
        destination: SearchDestination::Hand,
        shuffle_after: false,
    };
    let entry_attach = || ProcessState::AwaitingEntryAttachChoice {
        attaching: ObjectId(4),
        controller: P0,
        candidates: vec![Target::Object(ObjectId(5)), Target::Player(P1)],
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
        remaining: VecDeque::new(),
        destination: SearchDestination::Hand,
        shuffle_after: false,
    };
    let token_attach = || ProcessState::AwaitingTokenAttachChoice {
        attaching: ObjectId(4),
        controller: P0,
        candidates: vec![Target::Object(ObjectId(5))],
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
        remaining_count: 0,
        spec: TokenSpec::Creature(CreatureTokenSpec {
            subtypes: vec!["Soldier".to_string()],
            colors: ColorSet::empty(),
            power: 1,
            toughness: 1,
            keywords: KeywordSet::empty(),
            additional_types: CardTypeSet::empty(),
            additional_supertypes: SupertypeSet::empty(),
            name: None,
        }),
        copy_snapshot: None,
    };
    let attach = || ProcessState::AwaitingAttachChoice { aura: ObjectId(4), controller: P0, candidates: ids(&[5]) };
    let devour = || ProcessState::AwaitingDevourChoice {
        object: ObjectId(1),
        controller: P0,
        per_creature: 1,
        candidates: ids(&[8, 9]),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let amplify = || ProcessState::AwaitingAmplifyChoice {
        object: ObjectId(1),
        controller: P0,
        per_card: 1,
        candidates: ids(&[8, 9]),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let search_devour = || ProcessState::AwaitingSearchDevourChoice {
        object: ObjectId(1),
        controller: P0,
        per_creature: 1,
        candidates: ids(&[8, 9]),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
        remaining: VecDeque::new(),
        destination: SearchDestination::Hand,
        shuffle_after: false,
    };
    let search_amplify = || ProcessState::AwaitingSearchAmplifyChoice {
        object: ObjectId(1),
        controller: P0,
        per_card: 1,
        candidates: ids(&[8, 9]),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
        remaining: VecDeque::new(),
        destination: SearchDestination::Hand,
        shuffle_after: false,
    };
    let trigger_order = || ProcessState::AwaitTriggerOrderChoice {
        player: P0,
        group: vec![trigger(1, P0), trigger(2, P0)],
        ordered: Vec::new(),
        pending_groups: VecDeque::new(),
    };
    let trigger_targets = || ProcessState::AwaitingTriggerTargets {
        trigger: trigger(1, P0),
        spec: TargetSpec::Multiple(Vec::new()),
        candidates: vec![vec![Target::Object(ObjectId(5))]],
        remaining: VecDeque::new(),
        modal_effect: None,
    };
    let trigger_modes = |mode_choice: ModeChoice| ProcessState::AwaitingTriggerModeChoice {
        trigger: trigger(1, P0),
        modes: two_modes(),
        mode_choice,
        remaining: VecDeque::new(),
        chooser: P0,
    };
    let replacement = || ProcessState::AwaitingReplacementChoice {
        affected_player: P0,
        pending_event: GameEventKind::ScriptError { message: String::new() },
        candidates: vec![ReplacementCandidate::Registered(ObjectId(3))],
        excluded: Vec::new(),
        resume: ReplacementResumeTarget::ReplacementCheck { remaining: VecDeque::new(), checked: Vec::new() },
    };
    let legend = || ProcessState::AwaitingLegendRuleChoice {
        player: P0,
        candidates: ids(&[3, 4]),
        then: SbaResumeThen::SbaLoop,
    };
    let unless_paid = || ProcessState::AwaitingUnlessPaidChoice {
        payer_queue: VecDeque::from([P1]),
        cost: one_generic(),
        otherwise: UnlessPaidConsequence::SacrificeSelf,
        target_id: None,
        source: ObjectId(1),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let discard_choice = || ProcessState::AwaitingDiscardChoice {
        player: P0,
        candidates: ids(&[5, 6]),
        remaining_count: 1,
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
    };
    let copy = || ProcessState::AwaitingCopyChoice {
        object: ObjectId(1),
        controller: P0,
        exceptions: CopyExceptions {
            set_power_toughness: None,
            add_keywords: KeywordSet::empty(),
            retain_name: false,
            retain_color: false,
            grant_recurring_copy_trigger: false,
            enters_with_additional_counters: None,
        },
        candidates: ids(&[5]),
        stack_id: ObjectId(50),
        events_so_far: Vec::new(),
        colors_spent: ColorSet::empty(),
        carry_forward_if_declined: None,
        x_value: None,
    };
    let spell_mode = || ProcessState::AwaitingModeChoice { stack_id: ObjectId(50), chooser: P0, min: 1, max: 2 };
    let mulligan = || ProcessState::AwaitingMulliganDecision { player: P0 };
    let bottom = || ProcessState::AwaitingBottomCardChoice { player: P0, count: 2, remaining: VecDeque::new() };
    let select_target = || ScriptChoiceRequest::SelectTarget { filter: TargetFilter::AnyTarget, exclude_ids: ids(&[5]) };

    vec![
        // Scry and surveil: every offered card goes to exactly one pile.
        case!("scry: a card left out", scry(), |e| e.submit_scry_choice(ids(&[1]), Vec::new())),
        case!("scry: a card in both piles", scry(), |e| e.submit_scry_choice(ids(&[1, 2]), ids(&[2]))),
        case!("scry: a card that was not offered", scry(), |e| e.submit_scry_choice(ids(&[1, 2]), ids(&[3]))),
        case!("surveil: a card left out", surveil(), |e| e.submit_surveil_choice(Vec::new(), ids(&[2]))),
        case!("surveil: a card twice", surveil(), |e| e.submit_surveil_choice(ids(&[1, 1]), ids(&[2]))),
        case!("surveil: a card that was not offered", surveil(), |e| e.submit_surveil_choice(ids(&[1]), ids(&[2, 7]))),
        // Search.
        case!("search: more than the count", search(), |e| e.submit_search_choice(ids(&[8, 9]))),
        case!("search: not a candidate", search(), |e| e.submit_search_choice(ids(&[7]))),
        case!("search: a candidate twice", ProcessState::AwaitingSearchChoice {
            stack_id: ObjectId(50),
            controller: P0,
            events_so_far: Vec::new(),
            candidates: ids(&[8, 9]),
            count: 2,
            destination: SearchDestination::Hand,
            shuffle_after: false,
        }, |e| e.submit_search_choice(ids(&[8, 8]))),
        // Attach choices.
        case!("attach choice: not a candidate", attach(), |e| e.submit_attach_choice(ObjectId(9))),
        case!("entry attach: not a candidate", entry_attach(), |e| e.submit_entry_attach_choice(Target::Object(ObjectId(9)))),
        case!("entry attach: a player who is not offered", entry_attach(), |e| e.submit_entry_attach_choice(Target::Player(PlayerId(7)))),
        case!("token attach: not a candidate", token_attach(), |e| e.submit_token_attach_choice(Target::Player(P1))),
        // Devour and amplify, before and after a search.
        case!("devour: not a candidate", devour(), |e| e.submit_devour_choice(ids(&[7]))),
        case!("devour: a candidate twice", devour(), |e| e.submit_devour_choice(ids(&[8, 8]))),
        case!("amplify: not a candidate", amplify(), |e| e.submit_amplify_choice(ids(&[7]))),
        case!("amplify: a candidate twice", amplify(), |e| e.submit_amplify_choice(ids(&[9, 9]))),
        case!("search devour: not a candidate", search_devour(), |e| e.submit_search_devour_choice(ids(&[7]))),
        case!("search devour: a candidate twice", search_devour(), |e| e.submit_search_devour_choice(ids(&[8, 8]))),
        case!("search amplify: not a candidate", search_amplify(), |e| e.submit_search_amplify_choice(ids(&[7]))),
        case!("search amplify: a candidate twice", search_amplify(), |e| e.submit_search_amplify_choice(ids(&[9, 9]))),
        // Trigger ordering, targets and modes.
        case!("trigger order: too short", trigger_order(), |e| e.submit_trigger_order(vec![trigger(1, P0)])),
        case!("trigger order: a trigger from nowhere", trigger_order(), |e| e.submit_trigger_order(vec![trigger(1, P0), trigger(9, P0)])),
        case!("trigger order: one trigger twice", trigger_order(), |e| e.submit_trigger_order(vec![trigger(1, P0), trigger(1, P0)])),
        case!("trigger targets: a target no word offers", trigger_targets(), |e| e.submit_trigger_targets(vec![Target::Object(ObjectId(777))])),
        case!("trigger modes: none chosen", trigger_modes(ModeChoice::Exactly(1)), |e| e.submit_trigger_mode_choice(Vec::new())),
        case!("trigger modes: too many", trigger_modes(ModeChoice::Exactly(1)), |e| e.submit_trigger_mode_choice(vec![0, 1])),
        case!("trigger modes: one twice", trigger_modes(ModeChoice::Exactly(2)), |e| e.submit_trigger_mode_choice(vec![0, 0])),
        case!("trigger modes: a mode that does not exist", trigger_modes(ModeChoice::Exactly(1)), |e| e.submit_trigger_mode_choice(vec![9])),
        case!("trigger modes: a pair that cannot combine", trigger_modes(ModeChoice::Exactly(2)), |e| e.submit_trigger_mode_choice(vec![0, 1])),
        // Replacement, legend rule, copy, discard.
        case!("replacement: not a candidate", replacement(), |e| e.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(9)))),
        case!("replacement: protection nobody offered", replacement(), |e| e.submit_replacement_choice(ReplacementCandidate::Protection(ObjectId(3)))),
        case!("legend rule: not a candidate", legend(), |e| e.submit_legend_rule_choice(ObjectId(9))),
        case!("copy: not a candidate", copy(), |e| e.submit_copy_choice(Some(ObjectId(9)))),
        case!("discard choice: the wrong player", discard_choice(), |e| e.submit_discard_choice(P1, ObjectId(5))),
        case!("discard choice: not a candidate", discard_choice(), |e| e.submit_discard_choice(P0, ObjectId(9))),
        // Unless paid.
        case!("unless paid: the wrong payer pays", unless_paid(), |e| e.submit_unless_paid_payment(P0, PaymentPlan { payments: Vec::new() })),
        case!("unless paid: a plan that does not cover the cost", unless_paid(), |e| e.submit_unless_paid_payment(P1, PaymentPlan { payments: Vec::new() })),
        case!("unless paid: the wrong payer declines", unless_paid(), |e| e.decline_unless_paid(P0)),
        // Script choices.
        case!("script target: an excluded object", script_pause(select_target()), |e| e.submit_script_choice(Target::Object(ObjectId(5)))),
        case!("script target: an object that does not exist", script_pause(select_target()), |e| e.submit_script_choice(Target::Object(ObjectId(777)))),
        case!("script target: answered as a yes or no", script_pause(select_target()), |e| e.submit_script_may_choice(true)),
        case!("script may: answered with a target", script_pause(ScriptChoiceRequest::MayChoose), |e| e.submit_script_choice(Target::Player(P1))),
        // Pregame.
        case!("mulligan: the wrong player", mulligan(), |e| e.submit_mulligan_decision(action(P1, PlayerActionKind::KeepHand))),
        case!("mulligan: an action that is not keep or mulligan", mulligan(), |e| e.submit_mulligan_decision(action(P0, PlayerActionKind::PassPriority))),
        case!("mulligan: past the limit", mulligan(), |e| {
            e.settings.max_mulligans = Some(0);
            e.submit_mulligan_decision(action(P0, PlayerActionKind::TakeMulligan))
        }),
        case!("bottom: the wrong player", bottom(), |e| e.submit_bottom_cards(action(P1, PlayerActionKind::BottomCards { cards: ids(&[1, 2]) }))),
        case!("bottom: too few", bottom(), |e| e.submit_bottom_cards(action(P0, PlayerActionKind::BottomCards { cards: ids(&[1]) }))),
        case!("bottom: one card twice", bottom(), |e| e.submit_bottom_cards(action(P0, PlayerActionKind::BottomCards { cards: ids(&[1, 1]) }))),
        case!("bottom: cards that are not in the hand", bottom(), |e| e.submit_bottom_cards(action(P0, PlayerActionKind::BottomCards { cards: ids(&[1, 2]) }))),
        case!("bottom: the wrong kind of action", bottom(), |e| e.submit_bottom_cards(action(P0, PlayerActionKind::KeepHand))),
        // A spell's mode, chosen at resolution.
        case!("spell mode: none chosen", spell_mode(), |e| e.submit_mode_choice(Vec::new())),
        case!("spell mode: too many", spell_mode(), |e| e.submit_mode_choice(vec![0, 1, 2])),
        case!("spell mode: one twice", spell_mode(), |e| e.submit_mode_choice(vec![0, 0])),
        case!("spell mode: a mode that does not exist", spell_mode(), |e| e.submit_mode_choice(vec![9])),
        case!("spell mode: a pair that cannot combine", spell_mode(), |e| e.submit_mode_choice(vec![0, 1])),
        // A payment, to prove an unrelated pause is not touched by a payer's mistake.
        case!("payment: a plan for a free cost with a stray payment", ProcessState::AwaitingPayment {
            stack_id: ObjectId(50),
            payer: P0,
            cost: free(),
            source: ObjectId(1),
            on_paid: PaymentResolution::StackObject,
        }, |e| e.submit_payment(PaymentPlan { payments: vec![crate::types::cost::CostPayment::Life(3)] })),
    ]
}

fn engine_for(case: &Case) -> Engine {
    let mut state = minimal_state();
    // The spell whose mode is chosen at resolution.
    state.objects.insert(ObjectId(50), object(ObjectId(50), definition(two_modes(), ModeChoice::Exactly(1))));
    let mut engine = Engine::new(state);
    engine.process_state = (case.pause)();
    engine
}

#[test]
fn a_bad_answer_to_any_pause_is_refused_and_changes_nothing() {
    let table = cases();
    assert!(table.len() >= 50, "the table should cover every pause with a bad answer");
    for case in table {
        let mut engine = engine_for(&case);
        let before = engine.snapshot();
        let refusal = (case.answer)(&mut engine).unwrap_or_else(|| panic!("{}: the engine accepted a bad answer", case.name));
        // A wrong-pause refusal would pass the comparison below for the wrong reason.
        assert!(!refusal.starts_with("NotAwaiting"), "{}: the pause was not the one under test ({refusal})", case.name);
        println!("{:<60} {refusal}", case.name);
        let after = engine.snapshot();
        assert!(
            before == after,
            "{}: a refused answer changed the engine\n  before: {}\n  after:  {}",
            case.name,
            before.transient,
            after.transient
        );
    }
}
