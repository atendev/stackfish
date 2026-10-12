//! Phase 5 vertical slice: coverage for the kinds `phase5/modal-resolvable-ops`
//! put in `is_modal_resolvable` without testing them as a mode of a
//! multi-mode spell (`Discard`, `Surveil`, `Regenerate`, `Counter`), and for
//! the CR 700.2e chosen-by-opponent path (`Engine::submit_mode_choice`) with
//! modes that pause. Also pins that a resolving instant's own Stack ->
//! Graveyard `ZoneChange` is logged when its effect is a scripted zone move.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const VICTIM: ObjectId = ObjectId(101);
const BEAR: ObjectId = ObjectId(1);

fn card(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
        power: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        toughness: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(id, name, "unused", ManaCost { symbols: Vec::new() }, types)
    }
}

fn object_filter(card_types: Option<CardTypeSet>, zone: Option<ZoneKind>) -> TargetFilter {
    TargetFilter::Object {
        mana_value: None,
        card_types,
        excluded_card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone,
    }
}

fn one_target(filter: TargetFilter) -> Option<TargetSpec> {
    Some(TargetSpec::Primitive(TargetRequirement { filter, count: 1, up_to: false, at_least: false, exclude_source: false }))
}

fn creature() -> Option<TargetSpec> {
    one_target(object_filter(Some(CardTypeSet::CREATURE), None))
}

fn player() -> Option<TargetSpec> {
    one_target(TargetFilter::Player { controller: None })
}

fn spell_on_the_stack() -> Option<TargetSpec> {
    one_target(object_filter(None, Some(ZoneKind::Stack)))
}

fn mode(target_requirement: Option<TargetSpec>, effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn modal_spell(modes: Vec<SpellMode>, mode_choice: ModeChoice) -> CardDefinition {
    CardDefinition {
        modes,
        mode_choice,
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn everything() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

fn creatures() -> EffectFilter {
    EffectFilter { card_types: Some(CardTypeSet::CREATURE), ..everything() }
}

/// A bear on the battlefield (P1's), `library` cards and two `hand` cards in
/// each library and P1 hand, and the modal spell in P0's hand.
fn engine_with(spell: CardDefinition, library: u64, p1_hand: u64) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut place = |id: ObjectId, def: CardDefinition, owner: PlayerId, zone: ZoneKind| {
        state.objects.insert(id, card_object(id, def, owner, zone));
        let scope = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(owner) };
        state.zone_mut(zone, scope).unwrap().push_bottom(id);
    };
    place(BEAR, card(1, "Bear", CardTypeSet::CREATURE), P1, ZoneKind::Battlefield);
    for i in 0..library {
        place(ObjectId(200 + i), card(200 + i, &format!("P0 card {i}"), CardTypeSet::LAND), P0, ZoneKind::Library);
    }
    for i in 0..p1_hand {
        place(ObjectId(300 + i), card(300 + i, &format!("P1 hand {i}"), CardTypeSet::LAND), P1, ZoneKind::Hand);
    }
    place(SPELL, spell, P0, ZoneKind::Hand);
    Engine::new(state)
}

fn cast_card(engine: &mut Engine, card: ObjectId, chosen: Vec<u32>, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card, modes: chosen, targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
}

fn cast(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) {
    cast_card(engine, SPELL, chosen, targets);
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn zones_of_named(engine: &Engine, name: &str) -> Vec<ZoneKind> {
    engine.state().objects.values().filter(|o| o.definition.name == name).map(|o| o.zone).collect()
}

fn resolved_markers(engine: &Engine) -> usize {
    engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count()
}

fn assert_resolved_cleanly(engine: &Engine) {
    assert!(engine.state().stack.is_empty(), "stack drained");
    assert_eq!(resolved_markers(engine), 1, "the stack object resolves once, after its last mode");
    assert_eq!(zones_of_named(engine, "Test Spell"), vec![ZoneKind::Graveyard], "the spell ends in the graveyard");
}

fn redirect_battlefield_death(source: u64, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: Some(ZoneKind::Battlefield), to: Some(ZoneKind::Graveyard), affected: everything() },
            payload: ReplacementPayload::ReplaceZoneDestination(to),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn add_competing_redirects(engine: &mut Engine) {
    for (id, to) in [(901, ZoneKind::Exile), (902, ZoneKind::Hand)] {
        engine.state_mut().objects.insert(ObjectId(id), card_object(ObjectId(id), card(id, "Replacer", CardTypeSet::ENCHANTMENT), P0, ZoneKind::Battlefield));
        engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(id));
        engine.state_mut().replacement_effects.push(redirect_battlefield_death(id, to));
    }
}

// ---- The resolving instant's own zone change ---------------------------------

#[test]
fn a_destroy_instant_logs_its_own_stack_to_graveyard_move() {
    let mut spell = card(99, "Test Spell", CardTypeSet::INSTANT);
    spell.effect = Some(AbilityEffect::Destroy);
    spell.target_requirement = creature();
    let mut engine = engine_with(spell, 0, 0);
    cast(&mut engine, Vec::new(), vec![Target::Object(BEAR)]);
    pass_both(&mut engine);

    let spell_moves = engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(e.kind, GameEventKind::ZoneChange { from: ZoneKind::Stack, to: ZoneKind::Graveyard, .. }))
        .count();
    assert_eq!(spell_moves, 1, "the instant's move off the stack is in the ledger");
}

// ---- Discard ---------------------------------------------------------------

#[test]
fn a_discard_mode_pauses_for_the_targeted_player_and_the_next_mode_waits() {
    let spell = modal_spell(vec![mode(player(), AbilityEffect::Discard { count: 1 }), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2));
    let mut engine = engine_with(spell, 0, 2);
    cast(&mut engine, vec![0, 1], vec![Target::Player(P1)]);
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingDiscardChoice { player, candidates, .. } = status else { panic!("expected a discard choice, got {status:?}") };
    assert_eq!(player, P1, "the targeted player chooses");
    assert_eq!(life(&engine, P0), 20, "the next mode waits for the discard");

    engine.submit_discard_choice(P1, candidates[0]).unwrap();
    drain(&mut engine);

    assert_eq!(life(&engine, P0), 23);
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(P1)).unwrap().contents.len(), 1);
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P1)).unwrap().contents.len(), 1);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_discard_mode_with_an_empty_hand_does_not_pause_and_the_next_mode_runs() {
    let spell = modal_spell(vec![mode(player(), AbilityEffect::Discard { count: 1 }), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2));
    let mut engine = engine_with(spell, 0, 0);
    cast(&mut engine, vec![0, 1], vec![Target::Player(P1)]);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

// ---- Surveil ---------------------------------------------------------------

#[test]
fn a_surveil_mode_pauses_for_its_choice_and_the_next_mode_runs_after_it() {
    let spell = modal_spell(vec![mode(None, AbilityEffect::Surveil(1)), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2));
    let mut engine = engine_with(spell, 5, 0);
    cast(&mut engine, vec![0, 1], Vec::new());
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingSurveilChoice { cards, .. } = status else { panic!("expected a surveil choice, got {status:?}") };
    assert_eq!(life(&engine, P0), 20, "the next mode waits for the surveil");

    engine.submit_surveil_choice(Vec::new(), cards).unwrap();
    drain(&mut engine);

    assert_eq!(life(&engine, P0), 23);
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(P0)).unwrap().contents.len(), 2, "the surveilled card and the spell");
    assert_resolved_cleanly(&engine);
}

// ---- Regenerate ------------------------------------------------------------

#[test]
fn a_regenerate_mode_shields_the_target_from_a_later_destroy_mode() {
    let spell = modal_spell(vec![mode(creature(), AbilityEffect::Regenerate), mode(creature(), AbilityEffect::Destroy)], ModeChoice::Exactly(2));
    let mut engine = engine_with(spell, 0, 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR), Target::Object(BEAR)]);
    pass_both(&mut engine);

    let bear = engine.state().object(BEAR).expect("regenerated, so still the same object");
    assert_eq!(bear.zone, ZoneKind::Battlefield);
    assert!(bear.tapped, "CR 701.19a: regeneration taps it");
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_regenerate_mode_printed_after_the_destroy_mode_is_too_late() {
    let spell = modal_spell(vec![mode(creature(), AbilityEffect::Destroy), mode(creature(), AbilityEffect::Regenerate)], ModeChoice::Exactly(2));
    let mut engine = engine_with(spell, 0, 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR), Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert!(!zones_of_named(&engine, "Bear").contains(&ZoneKind::Battlefield), "CR 700.2d: destroy resolved first");
    assert_resolved_cleanly(&engine);
}

// ---- Counter ---------------------------------------------------------------

/// P0 casts a plain "gain 1 life" spell, then the modal spell in response.
fn engine_with_a_spell_to_counter(modes: Vec<SpellMode>) -> Engine {
    let mut engine = engine_with(modal_spell(modes, ModeChoice::Exactly(2)), 0, 0);
    let mut victim = card(98, "Victim", CardTypeSet::INSTANT);
    victim.effect = Some(AbilityEffect::GainLife(1));
    engine.state_mut().objects.insert(VICTIM, card_object(VICTIM, victim, P0, ZoneKind::Hand));
    engine.state_mut().zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(VICTIM);
    cast_card(&mut engine, VICTIM, Vec::new(), Vec::new());
    engine
}

#[test]
fn a_counter_mode_counters_its_spell_and_the_next_mode_still_resolves() {
    let mut engine = engine_with_a_spell_to_counter(vec![mode(spell_on_the_stack(), AbilityEffect::Counter), mode(None, AbilityEffect::GainLife(3))]);
    let victim_stack_id = engine.state().stack.last().expect("the victim is on the stack").stack_id;
    cast(&mut engine, vec![0, 1], vec![Target::Object(victim_stack_id)]);
    pass_both(&mut engine);

    assert!(engine.state().stack.is_empty(), "both stack objects are gone");
    assert_eq!(life(&engine, P0), 23, "the countered spell's +1 never happened, the mode's +3 did");
    assert_eq!(zones_of_named(&engine, "Victim"), vec![ZoneKind::Graveyard]);
    assert_eq!(zones_of_named(&engine, "Test Spell"), vec![ZoneKind::Graveyard]);
}

#[test]
fn a_counter_mode_followed_by_a_pausing_mode_resumes_after_the_pause() {
    let mut engine = engine_with_a_spell_to_counter(vec![mode(spell_on_the_stack(), AbilityEffect::Counter), mode(creature(), AbilityEffect::Destroy)]);
    add_competing_redirects(&mut engine);
    let victim_stack_id = engine.state().stack.last().unwrap().stack_id;
    cast(&mut engine, vec![0, 1], vec![Target::Object(victim_stack_id), Target::Object(BEAR)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "{status:?}");

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901))).unwrap();
    drain(&mut engine);

    assert_eq!(zones_of_named(&engine, "Bear"), vec![ZoneKind::Exile]);
    assert_eq!(life(&engine, P0), 20, "the victim was countered");
    assert!(engine.state().stack.is_empty());
}

// ---- Chosen by the opponent (CR 700.2e) -------------------------------------

fn chosen_by_opponent(modes: Vec<SpellMode>) -> CardDefinition {
    modal_spell(modes, ModeChoice::ExactlyChosenByOpponent(2))
}

fn cast_and_reach_the_mode_choice(engine: &mut Engine) {
    cast(engine, Vec::new(), Vec::new());
    let status = pass_both(engine);
    assert!(matches!(status, StepStatus::AwaitingModeChoice { .. }), "{status:?}");
}

#[test]
fn an_opponent_chosen_pair_of_pausing_modes_both_resolve() {
    let spell = chosen_by_opponent(vec![mode(None, AbilityEffect::DrawCards(1)), mode(None, AbilityEffect::DestroyAll(creatures()))]);
    let mut engine = engine_with(spell, 5, 0);
    cast_and_reach_the_mode_choice(&mut engine);
    engine.submit_mode_choice(vec![0, 1]).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents.len(), 1, "mode 0 drew");
    assert!(!zones_of_named(&engine, "Bear").contains(&ZoneKind::Battlefield), "mode 1 destroyed every creature");
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_opponent_chosen_pause_in_the_first_mode_resumes_the_second() {
    let spell = chosen_by_opponent(vec![mode(None, AbilityEffect::DestroyAll(creatures())), mode(None, AbilityEffect::GainLife(3))]);
    let mut engine = engine_with(spell, 0, 0);
    add_competing_redirects(&mut engine);
    cast_and_reach_the_mode_choice(&mut engine);
    engine.submit_mode_choice(vec![0, 1]).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "CR 616.1: {status:?}");
    assert_eq!(life(&engine, P0), 20);

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901))).unwrap();
    drain(&mut engine);

    assert_eq!(zones_of_named(&engine, "Bear"), vec![ZoneKind::Exile]);
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_opponent_chosen_combination_with_an_unsupported_kind_is_rejected() {
    let spell = chosen_by_opponent(vec![mode(None, AbilityEffect::GainLife(3)), mode(None, AbilityEffect::Cascade)]);
    let mut engine = engine_with(spell, 0, 0);
    cast_and_reach_the_mode_choice(&mut engine);
    let err = engine.submit_mode_choice(vec![0, 1]).unwrap_err();

    assert!(matches!(err, stackfish_engine::engine::ModeChoiceError::UnsupportedModeCombination), "{err:?}");
}
