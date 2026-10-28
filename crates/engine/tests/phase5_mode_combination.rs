//! `mode_can_join_others` is the one predicate for which modes can join other chosen modes. The modal entries, facade, cast, modal triggered ability and opponent's mode choice all use it, and a mode that fails it is solo only.

use std::collections::HashMap;

use stackfish_engine::decision::{Answer, CastEntry, Entry, Rejection, RejectionCode, Reply, ReplyShape};
use stackfish_engine::engine::{Engine, StepStatus, TriggerModeChoiceError};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const SOURCE: ObjectId = ObjectId(10);

fn mode(effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement: None, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn gain() -> SpellMode {
    mode(AbilityEffect::GainLife(1))
}

/// `Cascade` is not resolvable beside another mode, so this mode is solo only.
fn cascade() -> SpellMode {
    mode(AbilityEffect::Cascade)
}

fn weighted(effect: AbilityEffect, pawprint_weight: u32) -> SpellMode {
    SpellMode { pawprint_weight, ..mode(effect) }
}

fn modal_spell(modes: Vec<SpellMode>, mode_choice: ModeChoice) -> CardDefinition {
    CardDefinition {
        modes,
        mode_choice,
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn main_phase_engine(spell: CardDefinition) -> Engine {
    let mut state = GameState {
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 0)]),
        ..two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    state.objects.insert(SPELL, card_object(SPELL, spell, P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    Engine::new(state)
}

fn cast_entry(engine: &Engine) -> Option<CastEntry> {
    engine.legal_actions().into_iter().find_map(|entry| match entry {
        Entry::Cast(cast) if cast.card == SPELL => Some(cast),
        _ => None,
    })
}

fn cast_action(modes: &[u32]) -> PlayerAction {
    PlayerAction {
        player: P0,
        kind: PlayerActionKind::CastSpell {
            card: SPELL,
            modes: modes.to_vec(),
            targets: Vec::new(),
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    }
}

fn answer_cast(engine: &mut Engine, modes: &[u32]) -> Result<(), Rejection> {
    let decision = engine.open_decision().expect("a priority Decision");
    engine.answer(P0, Answer { decision: decision.id, reply: Reply::Action(cast_action(modes)) })
}

/// The facade refuses `modes` as `InvalidChoice`, changes nothing and keeps
/// the same Decision open.
fn assert_refused(engine: &mut Engine, modes: &[u32]) {
    let decision = engine.open_decision().expect("a priority Decision").id;
    let before = engine.snapshot();
    let refusal = answer_cast(engine, modes).unwrap_err();
    assert_eq!(refusal.code, RejectionCode::InvalidChoice, "{modes:?}: {}", refusal.message);
    assert_eq!(engine.snapshot(), before, "{modes:?}: a refusal changes nothing");
    assert_eq!(engine.open_decision().unwrap().id, decision, "{modes:?}: the Decision stays open");
}

#[test]
fn a_choose_two_card_with_one_combinable_mode_is_not_listed() {
    let engine = main_phase_engine(modal_spell(vec![gain(), cascade()], ModeChoice::Exactly(2)));
    assert!(cast_entry(&engine).is_none(), "no pair of modes is legal");
}

#[test]
fn a_choose_two_card_with_a_legal_pair_is_listed_with_its_solo_only_mode() {
    let spell = modal_spell(vec![gain(), cascade(), gain()], ModeChoice::Exactly(2));
    let mut engine = main_phase_engine(spell);
    let range = cast_entry(&engine).expect("modes 0 and 2 combine").modes.expect("a modal entry");
    assert_eq!(range.solo_only, vec![1]);

    assert_refused(&mut engine, &[0, 1]);
    assert_refused(&mut engine, &[1, 2]);
    answer_cast(&mut engine, &[0, 2]).expect("the combinable pair is accepted");
}

#[test]
fn a_solo_only_mode_is_accepted_alone_and_refused_beside_another_in_choose_one_or_both() {
    let mut engine = main_phase_engine(modal_spell(vec![gain(), cascade()], ModeChoice::UpTo(2)));
    let range = cast_entry(&engine).expect("a single mode is a legal selection").modes.unwrap();
    assert_eq!(range.solo_only, vec![1]);

    assert_refused(&mut engine, &[0, 1]);
    answer_cast(&mut engine, &[1]).expect("a solo-only mode alone is fine");
}

#[test]
fn an_empty_multi_target_mode_is_solo_only() {
    let empty_multiple = SpellMode { target_requirement: Some(TargetSpec::Multiple(Vec::new())), ..gain() };
    let engine = main_phase_engine(modal_spell(vec![empty_multiple, gain(), gain()], ModeChoice::UpTo(2)));
    let range = cast_entry(&engine).unwrap().modes.unwrap();
    assert_eq!(range.solo_only, vec![0]);
}

#[test]
fn a_card_with_no_solo_only_mode_lists_an_empty_set() {
    let engine = main_phase_engine(modal_spell(vec![gain(), gain()], ModeChoice::Exactly(2)));
    assert!(cast_entry(&engine).unwrap().modes.unwrap().solo_only.is_empty());
}

#[test]
fn a_repeatable_choose_two_needs_one_combinable_usable_mode() {
    let only_solo = main_phase_engine(modal_spell(vec![cascade()], ModeChoice::ExactlyRepeatable(2)));
    assert!(cast_entry(&only_solo).is_none(), "choosing the solo-only mode twice is still two modes");

    let mut with_one = main_phase_engine(modal_spell(vec![cascade(), gain()], ModeChoice::ExactlyRepeatable(2)));
    assert_eq!(cast_entry(&with_one).expect("the combinable mode can be chosen twice").modes.unwrap().solo_only, vec![0]);
    assert_refused(&mut with_one, &[0, 0]);
    answer_cast(&mut with_one, &[1, 1]).expect("the same combinable mode twice");
}

#[test]
fn a_pawprint_budget_card_takes_a_solo_only_mode_alone_but_not_paired() {
    let spell = modal_spell(
        vec![weighted(AbilityEffect::GainLife(1), 1), weighted(AbilityEffect::Cascade, 2)],
        ModeChoice::PawprintBudget(3),
    );
    let mut engine = main_phase_engine(spell);
    let range = cast_entry(&engine).unwrap().modes.unwrap();
    assert_eq!(range.solo_only, vec![1]);

    assert_refused(&mut engine, &[0, 1]);
    answer_cast(&mut engine, &[1]).expect("the heavy solo-only mode alone is within budget");
}

/// Every selection the entries' range allows is accepted by the facade
/// exactly when the engine accepts it as a cast.
#[test]
fn the_facade_and_the_engine_accept_the_same_selections() {
    let cards = [
        modal_spell(vec![gain(), cascade(), gain()], ModeChoice::Exactly(2)),
        modal_spell(vec![gain(), cascade(), gain()], ModeChoice::UpTo(3)),
        modal_spell(vec![cascade(), gain()], ModeChoice::UpToRepeatable(2)),
        modal_spell(vec![gain(), cascade()], ModeChoice::PawprintBudget(4)),
    ];
    for card in cards {
        let range = cast_entry(&main_phase_engine(card.clone())).expect("each card has a legal selection").modes.unwrap();
        let mut selections: Vec<Vec<u32>> = vec![Vec::new()];
        for _ in 0..3 {
            let longer: Vec<Vec<u32>> = selections
                .iter()
                .flat_map(|chosen| (0..range.mode_count as u32).map(move |index| [chosen.clone(), vec![index]].concat()))
                .collect();
            selections.extend(longer);
        }
        selections.sort();
        selections.dedup();
        for modes in selections {
            let facade = answer_cast(&mut main_phase_engine(card.clone()), &modes).is_ok();
            let engine = main_phase_engine(card.clone()).submit_action(cast_action(&modes)).is_ok();
            assert_eq!(facade, engine, "{:?} {modes:?}", card.mode_choice);
        }
    }
}

fn modal_upkeep(modes: Vec<SpellMode>) -> RegisteredAbility {
    RegisteredAbility {
        modes,
        mode_choice: ModeChoice::Exactly(2),
        id: AbilityId(0),
        source: SOURCE,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    }
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn a_modal_trigger_names_its_solo_only_modes_and_refuses_a_pair_with_one() {
    let mut state = GameState {
        phase: Phase::PreGame,
        ..two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    let source = minimal_card_definition(10, "Reaper", "reaper", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE);
    state.objects.insert(SOURCE, card_object(SOURCE, source, P0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(SOURCE);
    state.abilities.push(modal_upkeep(vec![gain(), cascade(), gain()]));
    let mut engine = Engine::new(state);

    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }), "{status:?}");
    let decision = engine.open_decision().unwrap();
    let ReplyShape::Modes { mode_count: 3, solo_only, .. } = decision.shape else { panic!("{:?}", decision.shape) };
    assert_eq!(solo_only, vec![1]);

    let before = engine.snapshot();
    let refusal = engine.answer(decision.player, Answer { decision: decision.id, reply: Reply::Modes(vec![0, 1]) });
    assert_eq!(refusal.unwrap_err().code, RejectionCode::InvalidChoice);
    assert_eq!(engine.snapshot(), before);
    assert_eq!(engine.open_decision().unwrap().id, decision.id, "the Decision stays open");
    let err = engine.submit_trigger_mode_choice(vec![0, 1]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::UnsupportedModeCombination), "{err:?}");
}

#[test]
fn an_opponent_chosen_spell_names_its_solo_only_modes_and_refuses_a_pair_with_one() {
    let spell = modal_spell(vec![gain(), cascade(), gain()], ModeChoice::ExactlyChosenByOpponent(2));
    let mut engine = main_phase_engine(spell);
    engine.submit_action(cast_action(&[])).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(&mut engine);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingModeChoice { .. }), "{status:?}");

    let decision = engine.open_decision().unwrap();
    let ReplyShape::Modes { mode_count: 3, solo_only, .. } = decision.shape else { panic!("{:?}", decision.shape) };
    assert_eq!(solo_only, vec![1]);

    let before = engine.snapshot();
    let refusal = engine.answer(decision.player, Answer { decision: decision.id, reply: Reply::Modes(vec![0, 1]) });
    assert_eq!(refusal.unwrap_err().code, RejectionCode::InvalidChoice);
    assert_eq!(engine.snapshot(), before);
    assert_eq!(engine.open_decision().unwrap().id, decision.id);
    engine.answer(decision.player, Answer { decision: decision.id, reply: Reply::Modes(vec![0, 2]) }).unwrap();
}
