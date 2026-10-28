//! `UnlessPaid` as a mode of a multi-mode spell or modal triggered ability. The mode pauses on `AwaitingUnlessPaidChoice` for a payer who may not be the controller, and both answers (`submit_unless_paid_payment`, `decline_unless_paid`) end in the `ReplacementCheck` tail so `modal_continuation` runs the remaining modes.

use stackfish_engine::engine::{Engine, StepStatus, UnlessPaidError};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const VICTIM: ObjectId = ObjectId(50);
const SOURCE: ObjectId = ObjectId(0);
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

fn spell_on_the_stack() -> Option<TargetSpec> {
    one_target(object_filter(None, Some(ZoneKind::Stack)))
}

fn creature() -> Option<TargetSpec> {
    one_target(object_filter(Some(CardTypeSet::CREATURE), None))
}

fn mode(target_requirement: Option<TargetSpec>, effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn unless_paid(cost: &str, otherwise: UnlessPaidConsequence) -> AbilityEffect {
    AbilityEffect::UnlessPaid { cost: CostSpec::mana_only(ManaCost::parse(cost).expect("valid mana cost")), otherwise }
}

fn everything() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

fn base_state(p1_mana: u32) -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    state.players[1].mana_pool.colorless = p1_mana;
    state.objects.insert(BEAR, card_object(BEAR, card(1, "Bear", CardTypeSet::CREATURE), P1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(BEAR);
    state
}

fn push_victim_spell(state: &mut GameState) {
    state.objects.insert(VICTIM, card_object(VICTIM, card(2, "Victim", CardTypeSet::CREATURE), P1, ZoneKind::Stack));
    state.stack.push(StackObject {
        stack_id: VICTIM,
        kind: StackEntryKind::Spell,
        source: VICTIM,
        source_lki: None,
        owner: P1,
        controller: P1,
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
}

fn spell_engine(modes: Vec<SpellMode>, p1_mana: u32) -> Engine {
    let mut state = base_state(p1_mana);
    push_victim_spell(&mut state);
    let spell = CardDefinition {
        modes,
        mode_choice: ModeChoice::Exactly(2),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    };
    state.objects.insert(SPELL, card_object(SPELL, spell, P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    Engine::new(state)
}

fn cast(engine: &mut Engine, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: vec![0, 1], targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn pay(engine: &mut Engine, payer: PlayerId, units: u32) -> Result<(), UnlessPaidError> {
    let units = vec![ManaUnit::Colorless; units as usize];
    engine.submit_unless_paid_payment(payer, PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
}

fn life(engine: &Engine) -> i32 {
    engine.state().player(P0).unwrap().life
}

fn zones_of(engine: &Engine, name: &str) -> Vec<ZoneKind> {
    engine.state().objects.values().filter(|o| o.definition.name == name).map(|o| o.zone).collect()
}

fn resolved_markers(engine: &Engine) -> usize {
    engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count()
}

fn countered(engine: &Engine) -> bool {
    engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Countered { .. }))
}

fn mana_leak_modes() -> Vec<SpellMode> {
    vec![mode(spell_on_the_stack(), unless_paid("{3}", UnlessPaidConsequence::Counter)), mode(None, AbilityEffect::GainLife(3))]
}

#[test]
fn declining_the_cost_counters_the_target_and_the_next_mode_runs() {
    let mut engine = spell_engine(mana_leak_modes(), 0);
    cast(&mut engine, vec![Target::Object(VICTIM)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { payer, .. } if payer == P1), "the countered spell's controller is asked: {status:?}");
    assert_eq!(life(&engine), 20, "the next mode waits for the answer");

    engine.decline_unless_paid(P1).unwrap();
    drain(&mut engine);

    assert!(countered(&engine));
    assert_eq!(zones_of(&engine, "Victim"), vec![ZoneKind::Graveyard]);
    assert_eq!(life(&engine), 23);
    assert!(engine.state().stack.is_empty());
    assert_eq!(resolved_markers(&engine), 1, "only the modal spell resolved");
    assert_eq!(zones_of(&engine, "Test Spell"), vec![ZoneKind::Graveyard]);
}

#[test]
fn paying_the_cost_spares_the_target_and_the_next_mode_still_runs() {
    let mut engine = spell_engine(mana_leak_modes(), 3);
    cast(&mut engine, vec![Target::Object(VICTIM)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }), "{status:?}");

    pay(&mut engine, P1, 3).expect("3 generic covers {3}");
    drain(&mut engine);

    assert!(!countered(&engine));
    assert_eq!(engine.state().player(P1).unwrap().mana_pool.colorless, 0, "the {{3}} was deducted");
    assert_eq!(life(&engine), 23);
    assert_eq!(engine.state().stack.len(), 1, "only the spared victim is left");
    assert_eq!(resolved_markers(&engine), 1);
}

#[test]
fn an_insufficient_payment_keeps_the_pause_open_and_the_modes_intact() {
    let mut engine = spell_engine(mana_leak_modes(), 3);
    cast(&mut engine, vec![Target::Object(VICTIM)]);
    pass_both(&mut engine);

    let err = pay(&mut engine, P1, 1).unwrap_err();
    assert!(matches!(err, UnlessPaidError::InvalidPlan), "{err:?}");
    let err = pay(&mut engine, P0, 3).unwrap_err();
    assert!(matches!(err, UnlessPaidError::NotCurrentPayer), "{err:?}");

    engine.decline_unless_paid(P1).expect("the pause is still open");
    drain(&mut engine);

    assert!(countered(&engine));
    assert_eq!(life(&engine), 23, "the second mode still ran after the retry");
}

#[test]
fn an_unless_paid_mode_followed_by_a_pausing_mode_resumes_through_both_pauses() {
    let mut engine = spell_engine(vec![mana_leak_modes().remove(0), mode(creature(), AbilityEffect::Destroy)], 0);
    for (id, to) in [(901, ZoneKind::Exile), (902, ZoneKind::Hand)] {
        engine.state_mut().objects.insert(ObjectId(id), card_object(ObjectId(id), card(id, "Replacer", CardTypeSet::ENCHANTMENT), P0, ZoneKind::Battlefield));
        engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(id));
        engine.state_mut().replacement_effects.push(ReplacementEffect {
            source: ObjectId(id),
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::WouldMoveZone { from: Some(ZoneKind::Battlefield), to: Some(ZoneKind::Graveyard), affected: everything() },
                payload: ReplacementPayload::ReplaceZoneDestination(to),
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
    }
    cast(&mut engine, vec![Target::Object(VICTIM), Target::Object(BEAR)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }), "{status:?}");

    engine.decline_unless_paid(P1).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "the second mode's CR 616.1 pause: {status:?}");
    assert!(countered(&engine), "the first mode already finished");

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901))).unwrap();
    drain(&mut engine);

    assert_eq!(zones_of(&engine, "Bear"), vec![ZoneKind::Exile]);
    assert!(engine.state().stack.is_empty());
    assert_eq!(resolved_markers(&engine), 1);
}

#[test]
fn a_synchronous_mode_before_the_unless_paid_mode_resolves_first() {
    let modes = vec![mode(None, AbilityEffect::GainLife(3)), mode(spell_on_the_stack(), unless_paid("{3}", UnlessPaidConsequence::Counter))];
    let mut engine = spell_engine(modes, 0);
    cast(&mut engine, vec![Target::Object(VICTIM)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }), "{status:?}");
    assert_eq!(life(&engine), 23, "mode 0 already resolved while the payer decides");

    engine.decline_unless_paid(P1).unwrap();
    drain(&mut engine);

    assert!(countered(&engine));
    assert_eq!(resolved_markers(&engine), 1);
}

fn echo_trigger_engine() -> Engine {
    let mut state = GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    state.objects.insert(SOURCE, card_object(SOURCE, card(10, "Reaper", CardTypeSet::CREATURE), P0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(SOURCE);
    state.abilities.push(RegisteredAbility {
        modes: vec![mode(None, unless_paid("{2}", UnlessPaidConsequence::SacrificeSelf)), mode(None, AbilityEffect::GainLife(3))],
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
    });
    Engine::new(state)
}

// p0_mana goes into the pool only once the trigger is on the stack, since the pool empties as the game enters upkeep.
fn reach_the_unless_paid_pause(engine: &mut Engine, p0_mana: u32) -> StepStatus {
    let status = pass_both(engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }), "{status:?}");
    engine.submit_trigger_mode_choice(vec![0, 1]).unwrap();
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "{status:?}");
    engine.submit_trigger_targets(Vec::new()).unwrap();
    drain(engine);
    engine.state_mut().players[0].mana_pool.colorless = p0_mana;
    pass_both(engine)
}

#[test]
fn declining_an_echo_style_mode_sacrifices_the_source_and_the_next_mode_runs() {
    let mut engine = echo_trigger_engine();
    let status = reach_the_unless_paid_pause(&mut engine, 0);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { payer, .. } if payer == P0), "the controller is asked: {status:?}");

    engine.decline_unless_paid(P0).unwrap();
    drain(&mut engine);

    assert_eq!(zones_of(&engine, "Reaper"), vec![ZoneKind::Graveyard]);
    assert_eq!(life(&engine), 23);
    assert!(engine.state().stack.is_empty());
    assert_eq!(resolved_markers(&engine), 1);
}

#[test]
fn paying_an_echo_style_mode_spares_the_source_and_the_next_mode_runs() {
    let mut engine = echo_trigger_engine();
    let status = reach_the_unless_paid_pause(&mut engine, 2);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }), "{status:?}");

    pay(&mut engine, P0, 2).expect("2 generic covers {2}");
    drain(&mut engine);

    assert_eq!(zones_of(&engine, "Reaper"), vec![ZoneKind::Battlefield]);
    assert_eq!(life(&engine), 23);
    assert_eq!(resolved_markers(&engine), 1);
}
