//! Phase 5 vertical slice: source-bound effects as modes of a modal triggered
//! ability. `SacrificeSelf`, `BecomeCopy`, `Attach` and the one-target-word
//! `Fight`/`Bite` act on the ability's own source permanent, so a spell (whose
//! source is the card on the stack) can't use them; a modal triggered ability
//! (CR 603.3c, `RegisteredAbility::modes`) can. Also covers the trigger-path
//! chosen-by-opponent case (`submit_trigger_mode_choice`) with pausing modes.
//!
//! There is no modal *activated* ability yet (`commands/activate.rs` has no
//! modes), so every vehicle here is an upkeep trigger.

use stackfish_engine::engine::{Engine, StepStatus, TriggerModeChoiceError};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SOURCE: ObjectId = ObjectId(0);
const BEAR: ObjectId = ObjectId(1);

fn card(id: u64, name: &str, types: CardTypeSet, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
        power: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(power)),
        toughness: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(toughness)),
        ..minimal_card_definition(id, name, "unused", ManaCost { symbols: Vec::new() }, types)
    }
}

fn equipment(id: u64, name: &str) -> CardDefinition {
    CardDefinition { subtypes: vec![Subtype("Equipment".to_string())], ..card(id, name, CardTypeSet::ARTIFACT, 0, 0) }
}

fn creature_target(exclude_source: bool) -> Option<TargetSpec> {
    Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            card_types: Some(CardTypeSet::CREATURE),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source,
    }))
}

fn mode(target_requirement: Option<TargetSpec>, effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn everything() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

/// "At the beginning of your upkeep, choose two - ..." with `source` as the
/// permanent the effects act on.
fn modal_upkeep(source: ObjectId, modes: Vec<SpellMode>, mode_choice: ModeChoice) -> RegisteredAbility {
    RegisteredAbility {
        modes,
        mode_choice,
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    }
}

/// A 3/3 `Reaper` (P0, the default source) and a 2/2 `Bear` (P1) on the
/// battlefield, `library` cards on P0's library, and `ability` registered.
fn engine_with(ability: impl FnOnce() -> RegisteredAbility, library: u64) -> Engine {
    let mut state = GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut place = |id: ObjectId, def: CardDefinition, owner: PlayerId, zone: ZoneKind| {
        state.objects.insert(id, card_object(id, def, owner, zone));
        let scope = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(owner) };
        state.zone_mut(zone, scope).unwrap().push_bottom(id);
    };
    place(SOURCE, card(10, "Reaper", CardTypeSet::CREATURE, 3, 3), P0, ZoneKind::Battlefield);
    place(BEAR, card(11, "Bear", CardTypeSet::CREATURE, 2, 2), P1, ZoneKind::Battlefield);
    for i in 0..library {
        place(ObjectId(200 + i), card(200 + i, &format!("Card {i}"), CardTypeSet::LAND, 0, 0), P0, ZoneKind::Library);
    }
    state.abilities.push(ability());
    Engine::new(state)
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

/// Upkeep fires the trigger, `chosen` answers the mode choice, `targets`
/// answers the combined target pause, and both players pass to resolve it.
fn fire_and_resolve(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) {
    let status = pass_both(engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }), "{status:?}");
    engine.submit_trigger_mode_choice(chosen).unwrap();
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "2+ modes combine into a target pause: {status:?}");
    engine.submit_trigger_targets(targets).unwrap();
    drain(engine);
    assert_eq!(engine.state().stack.len(), 1, "the ability is on the stack");
    pass_both(engine);
}

fn life(engine: &Engine) -> i32 {
    engine.state().player(P0).unwrap().life
}

fn zone_of(engine: &Engine, name: &str) -> Vec<ZoneKind> {
    engine.state().objects.values().filter(|o| o.definition.name == name).map(|o| o.zone).collect()
}

fn assert_resolved_cleanly(engine: &Engine) {
    assert!(engine.state().stack.is_empty(), "stack drained");
    let markers = engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count();
    assert_eq!(markers, 1, "the ability resolves once, after its last mode");
}

fn damage_on(engine: &Engine, id: ObjectId) -> u32 {
    engine.state().object(id).map(|o| o.damage_marked).unwrap_or(0)
}

// ---- SacrificeSelf ---------------------------------------------------------

#[test]
fn a_sacrifice_self_mode_sacrifices_the_source_and_the_next_mode_runs() {
    let mut engine = engine_with(|| modal_upkeep(SOURCE, vec![mode(None, AbilityEffect::SacrificeSelf), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2)), 0);
    fire_and_resolve(&mut engine, vec![0, 1], Vec::new());

    assert_eq!(zone_of(&engine, "Reaper"), vec![ZoneKind::Graveyard]);
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_synchronous_mode_followed_by_a_sacrifice_self_mode_both_resolve() {
    let mut engine = engine_with(|| modal_upkeep(SOURCE, vec![mode(None, AbilityEffect::GainLife(3)), mode(None, AbilityEffect::SacrificeSelf)], ModeChoice::Exactly(2)), 0);
    fire_and_resolve(&mut engine, vec![0, 1], Vec::new());

    assert_eq!(zone_of(&engine, "Reaper"), vec![ZoneKind::Graveyard]);
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_sacrifice_self_mode_then_a_draw_mode_resolve_one_after_the_other() {
    let mut engine = engine_with(|| modal_upkeep(SOURCE, vec![mode(None, AbilityEffect::SacrificeSelf), mode(None, AbilityEffect::DrawCards(1))], ModeChoice::Exactly(2)), 3);
    fire_and_resolve(&mut engine, vec![0, 1], Vec::new());

    assert_eq!(zone_of(&engine, "Reaper"), vec![ZoneKind::Graveyard]);
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents.len(), 1);
    assert_resolved_cleanly(&engine);
}

// ---- Fight and Bite as a one-word mode ----------------------------------------

#[test]
fn a_fight_mode_makes_the_source_fight_its_own_target() {
    let mut engine = engine_with(|| modal_upkeep(SOURCE, vec![mode(creature_target(true), AbilityEffect::Fight), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2)), 0);
    fire_and_resolve(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);

    assert!(!zone_of(&engine, "Bear").contains(&ZoneKind::Battlefield), "3 damage kills the 2/2 bear");
    assert_eq!(damage_on(&engine, SOURCE), 2, "the bear's power came back at the Reaper");
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_bite_mode_deals_one_sided_damage_from_the_source() {
    let mut engine = engine_with(|| modal_upkeep(SOURCE, vec![mode(creature_target(true), AbilityEffect::Bite), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2)), 0);
    fire_and_resolve(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);

    assert!(!zone_of(&engine, "Bear").contains(&ZoneKind::Battlefield));
    assert_eq!(damage_on(&engine, SOURCE), 0, "a bite takes nothing back");
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_fight_mode_followed_by_a_pausing_mode_commits_its_damage_first() {
    let mut engine = engine_with(|| modal_upkeep(SOURCE, vec![mode(creature_target(true), AbilityEffect::Fight), mode(None, AbilityEffect::DrawCards(1))], ModeChoice::Exactly(2)), 3);
    fire_and_resolve(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);

    assert!(!zone_of(&engine, "Bear").contains(&ZoneKind::Battlefield));
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents.len(), 1);
    let damage = engine.state().ledger.iter().position(|e| matches!(e.kind, GameEventKind::DamageDealt { .. })).expect("damage logged");
    let draw = engine.state().ledger.iter().position(|e| matches!(e.kind, GameEventKind::ZoneChange { from: ZoneKind::Library, .. })).expect("draw logged");
    assert!(damage < draw, "CR 700.2d: printed order");
    assert_resolved_cleanly(&engine);
}

// ---- BecomeCopy ------------------------------------------------------------

#[test]
fn a_become_copy_mode_makes_the_source_a_copy_of_its_target() {
    let mut engine = engine_with(
        || modal_upkeep(SOURCE, vec![mode(creature_target(true), AbilityEffect::BecomeCopy(CopyExceptions::default())), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2)),
        0,
    );
    fire_and_resolve(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);

    let copy = engine.state().object(SOURCE).unwrap().copiable_override.as_ref().expect("CR 707.2: the source took the bear's copiable values");
    assert_eq!(copy.name, "Bear");
    assert_eq!(life(&engine), 23);
    assert!(engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::BecameCopy { object, copied } if object == SOURCE && copied == BEAR)));
    assert_resolved_cleanly(&engine);
}

// ---- Attach ----------------------------------------------------------------

#[test]
fn an_attach_mode_attaches_the_source_equipment_to_its_target() {
    let cutlass = ObjectId(2);
    let mut engine = engine_with(
        || modal_upkeep(cutlass, vec![mode(creature_target(false), AbilityEffect::Attach), mode(None, AbilityEffect::GainLife(3))], ModeChoice::Exactly(2)),
        0,
    );
    engine.state_mut().objects.insert(cutlass, card_object(cutlass, equipment(12, "Cutlass"), P0, ZoneKind::Battlefield));
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(cutlass);
    fire_and_resolve(&mut engine, vec![0, 1], vec![Target::Object(SOURCE)]);

    assert_eq!(engine.state().object(cutlass).unwrap().attached_to, Some(SOURCE));
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

// ---- Chosen by the opponent (CR 700.2e), trigger path -------------------------

fn chosen_by_opponent(modes: Vec<SpellMode>) -> impl FnOnce() -> RegisteredAbility {
    move || modal_upkeep(SOURCE, modes, ModeChoice::ExactlyChosenByOpponent(2))
}

/// Upkeep fires, the other player answers the mode choice, and the trivially
/// satisfied target pause is answered.
fn fire_chosen_by_opponent(engine: &mut Engine, chosen: Vec<u32>) -> StepStatus {
    let status = pass_both(engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }), "{status:?}");
    engine.submit_trigger_mode_choice(chosen).unwrap();
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "{status:?}");
    engine.submit_trigger_targets(Vec::new()).unwrap();
    drain(engine);
    pass_both(engine)
}

#[test]
fn an_opponent_chosen_sacrifice_self_and_gain_life_both_resolve() {
    let mut engine = engine_with(chosen_by_opponent(vec![mode(None, AbilityEffect::SacrificeSelf), mode(None, AbilityEffect::GainLife(3))]), 0);
    fire_chosen_by_opponent(&mut engine, vec![0, 1]);

    assert_eq!(zone_of(&engine, "Reaper"), vec![ZoneKind::Graveyard]);
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_opponent_chosen_pause_in_the_first_mode_resumes_the_second() {
    let mut engine = engine_with(
        chosen_by_opponent(vec![
            mode(None, AbilityEffect::DestroyAll(EffectFilter { card_types: Some(CardTypeSet::CREATURE), ..everything() })),
            mode(None, AbilityEffect::GainLife(3)),
        ]),
        0,
    );
    for (id, to) in [(901, ZoneKind::Exile), (902, ZoneKind::Hand)] {
        engine.state_mut().objects.insert(ObjectId(id), card_object(ObjectId(id), card(id, "Replacer", CardTypeSet::ENCHANTMENT, 0, 0), P0, ZoneKind::Battlefield));
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
    let status = fire_chosen_by_opponent(&mut engine, vec![0, 1]);
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "CR 616.1: {status:?}");
    assert_eq!(life(&engine), 20);

    // Two creatures die, so there may be one pause per creature.
    let mut status = status;
    while matches!(status, StepStatus::AwaitingReplacementChoice { .. }) {
        engine.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901))).unwrap();
        status = drain(&mut engine);
    }

    assert_eq!(life(&engine), 23, "the second mode ran after every pause");
    assert_eq!(zone_of(&engine, "Reaper"), vec![ZoneKind::Exile]);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_opponent_chosen_combination_with_an_unsupported_kind_is_rejected() {
    let mut engine = engine_with(chosen_by_opponent(vec![mode(None, AbilityEffect::GainLife(3)), mode(None, AbilityEffect::ExchangeTextBoxes)]), 0);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }), "{status:?}");
    let err = engine.submit_trigger_mode_choice(vec![0, 1]).unwrap_err();

    assert!(matches!(err, TriggerModeChoiceError::UnsupportedModeCombination), "{err:?}");
}
