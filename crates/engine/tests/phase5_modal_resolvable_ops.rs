//! CR 700.2d with every kind of mode. Each chosen mode goes through the resolver a single effect uses, so the tap family, `LoseLife`, `Regenerate`, `Counter`, `Fight`, `Bite`, scripted zone moves (with CR 616.1 pauses), `DrawCards`, `Discard`, `Scry` and `Surveil` can be a mode.
//! The remaining modes wait in `Engine::modal_continuation` until the mode before them settles.

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const BEAR: ObjectId = ObjectId(1);
const ROCK: ObjectId = ObjectId(2);

fn card(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
        power: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        toughness: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(id, name, "unused", ManaCost { symbols: Vec::new() }, types)
    }
}

fn object_of(types: CardTypeSet) -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            card_types: Some(types),
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
        exclude_source: false,
    })
}

fn creature() -> Option<TargetSpec> {
    Some(object_of(CardTypeSet::CREATURE))
}

fn artifact() -> Option<TargetSpec> {
    Some(object_of(CardTypeSet::ARTIFACT))
}

fn player() -> Option<TargetSpec> {
    Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Player { controller: None },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }))
}

fn mode(target_requirement: Option<TargetSpec>, effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn modal_spell(modes: Vec<SpellMode>) -> CardDefinition {
    CardDefinition {
        modes,
        mode_choice: ModeChoice::Exactly(2),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn engine_with(modes: Vec<SpellMode>, library: u64) -> Engine {
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
    place(ROCK, card(2, "Rock", CardTypeSet::ARTIFACT), P1, ZoneKind::Battlefield);
    for i in 0..library {
        place(ObjectId(200 + i), card(200 + i, &format!("P0 card {i}"), CardTypeSet::LAND), P0, ZoneKind::Library);
        place(ObjectId(300 + i), card(300 + i, &format!("P1 card {i}"), CardTypeSet::LAND), P1, ZoneKind::Library);
    }
    place(SPELL, modal_spell(modes), P0, ZoneKind::Hand);
    Engine::new(state)
}

fn try_cast(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: chosen, targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    drain(engine)
}

fn try_try_cast(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: chosen, targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        ?;
    Ok(drain(engine))
}

fn cast(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) {
    let status = try_cast(engine, chosen, targets);
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

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn zone_of_named(engine: &Engine, name: &str) -> Vec<ZoneKind> {
    engine.state().objects.values().filter(|o| o.definition.name == name).map(|o| o.zone).collect()
}

fn resolved_markers(engine: &Engine) -> usize {
    engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count()
}

fn ledger_position(engine: &Engine, matches: impl Fn(&GameEventKind) -> bool) -> Option<usize> {
    engine.state().ledger.iter().position(|e| matches(&e.kind))
}

fn assert_resolved_cleanly(engine: &Engine) {
    assert!(engine.state().stack.is_empty(), "stack drained");
    assert_eq!(resolved_markers(engine), 1, "the stack object resolves once, after its last mode");
    assert_eq!(zone_of_named(engine, "Test Spell"), vec![ZoneKind::Graveyard], "the spell ends in the graveyard");
}

fn redirect_battlefield_death(source: u64, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter { controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(to),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn tap_and_lose_life_modes_both_resolve_against_their_own_targets() {
    let mut engine = engine_with(vec![mode(creature(), AbilityEffect::Tap), mode(player(), AbilityEffect::LoseLife(2))], 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR), Target::Player(P1)]);
    pass_both(&mut engine);

    assert!(engine.state().object(BEAR).unwrap().tapped, "mode 0 tapped its target");
    assert_eq!(life(&engine, P1), 18, "mode 1 took life from its own player target");
    assert_eq!(life(&engine, P0), 20);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_untargeted_lose_life_mode_hits_the_controller_not_the_other_modes_target() {
    let mut engine = engine_with(vec![mode(None, AbilityEffect::LoseLife(3)), mode(creature(), AbilityEffect::Tap)], 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), 17, "CR 700.2c: a mode with no targets acts on its controller");
    assert_eq!(life(&engine, P1), 20);
    assert!(engine.state().object(BEAR).unwrap().tapped);
    assert_resolved_cleanly(&engine);
}

#[test]
fn untap_and_tap_modes_resolve_in_printed_order_on_one_object() {
    let mut engine = engine_with(vec![mode(creature(), AbilityEffect::Untap), mode(creature(), AbilityEffect::Tap)], 0);
    engine.state_mut().object_mut(BEAR).unwrap().tapped = true;
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR), Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert!(engine.state().object(BEAR).unwrap().tapped, "untap, then tap: ends tapped");
    let untapped = ledger_position(&engine, |k| matches!(k, GameEventKind::Untapped { .. })).expect("untap logged");
    let tapped = ledger_position(&engine, |k| matches!(k, GameEventKind::Tapped { .. })).expect("tap logged");
    assert!(untapped < tapped, "CR 700.2d: printed order");
}

#[test]
fn a_destroy_mode_followed_by_a_synchronous_mode_both_resolve() {
    let mut engine = engine_with(vec![mode(creature(), AbilityEffect::Destroy), mode(None, AbilityEffect::GainLife(3))], 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert!(!zone_of_named(&engine, "Bear").contains(&ZoneKind::Battlefield));
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_synchronous_mode_followed_by_a_destroy_mode_both_resolve() {
    let mut engine = engine_with(vec![mode(None, AbilityEffect::GainLife(3)), mode(creature(), AbilityEffect::Destroy)], 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert!(!zone_of_named(&engine, "Bear").contains(&ZoneKind::Battlefield));
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn damage_from_an_earlier_mode_is_committed_before_a_later_zone_move() {
    let mut engine = engine_with(vec![mode(player(), AbilityEffect::Damage(2)), mode(creature(), AbilityEffect::Destroy)], 0);
    cast(&mut engine, vec![0, 1], vec![Target::Player(P1), Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P1), 18);
    let damage = ledger_position(&engine, |k| matches!(k, GameEventKind::DamageDealt { .. })).expect("damage logged");
    let death = ledger_position(&engine, |k| matches!(k, GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. })).expect("destroy logged");
    assert!(damage < death, "CR 700.2d: the first mode's damage lands before the second mode destroys");
    assert_resolved_cleanly(&engine);
}

#[test]
fn two_different_zone_move_modes_each_move_their_own_target() {
    let mut engine = engine_with(vec![mode(creature(), AbilityEffect::Destroy), mode(artifact(), AbilityEffect::Exile)], 0);
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR), Target::Object(ROCK)]);
    pass_both(&mut engine);

    assert_eq!(zone_of_named(&engine, "Bear"), vec![ZoneKind::Graveyard]);
    assert_eq!(zone_of_named(&engine, "Rock"), vec![ZoneKind::Exile]);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_ordering_pause_in_an_earlier_mode_resumes_the_remaining_modes() {
    let mut engine = engine_with(vec![mode(creature(), AbilityEffect::Destroy), mode(None, AbilityEffect::GainLife(3))], 0);
    for (id, to) in [(901, ZoneKind::Exile), (902, ZoneKind::Hand)] {
        let mut def = card(id, "Replacer", CardTypeSet::ENCHANTMENT);
        def.script_ref = ScriptReference::ByKey("replacer".to_string());
        engine.state_mut().objects.insert(ObjectId(id), card_object(ObjectId(id), def, P0, ZoneKind::Battlefield));
        engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(id));
        engine.state_mut().replacement_effects.push(redirect_battlefield_death(id, to));
    }
    cast(&mut engine, vec![0, 1], vec![Target::Object(BEAR)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "CR 616.1: {status:?}");
    assert_eq!(life(&engine, P0), 20, "the second mode hasn't run while the first is paused");

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901))).unwrap();
    drain(&mut engine);

    assert_eq!(zone_of_named(&engine, "Bear"), vec![ZoneKind::Exile], "the chosen replacement applied");
    assert_eq!(life(&engine, P0), 23, "the paused mode's successor ran after the answer");
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_mill_mode_and_a_tap_mode_resolve_together() {
    let mut engine = engine_with(vec![mode(player(), AbilityEffect::Mill(2)), mode(creature(), AbilityEffect::Tap)], 5);
    cast(&mut engine, vec![0, 1], vec![Target::Player(P1), Target::Object(BEAR)]);
    pass_both(&mut engine);

    let milled = engine.state().zone(ZoneKind::Graveyard, Some(P1)).unwrap().contents.len();
    assert_eq!(milled, 2, "the mill mode acted on its own player target");
    assert_eq!(engine.state().zone(ZoneKind::Library, Some(P0)).unwrap().contents.len(), 5, "not on the controller");
    assert!(engine.state().object(BEAR).unwrap().tapped);
}

#[test]
fn a_draw_mode_and_a_lose_life_mode_resolve_together() {
    let mut engine = engine_with(vec![mode(None, AbilityEffect::DrawCards(2)), mode(None, AbilityEffect::LoseLife(1))], 5);
    cast(&mut engine, vec![0, 1], Vec::new());
    pass_both(&mut engine);

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents.len(), 2);
    assert_eq!(engine.state().zone(ZoneKind::Library, Some(P0)).unwrap().contents.len(), 3);
    assert_eq!(life(&engine, P0), 19);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_lose_life_mode_followed_by_a_draw_mode_resolve_together() {
    let mut engine = engine_with(vec![mode(None, AbilityEffect::LoseLife(1)), mode(None, AbilityEffect::DrawCards(1))], 5);
    cast(&mut engine, vec![0, 1], Vec::new());
    pass_both(&mut engine);

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents.len(), 1);
    assert_eq!(life(&engine, P0), 19);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_scry_mode_pauses_for_its_choice_and_the_next_mode_runs_after_it() {
    let mut engine = engine_with(vec![mode(None, AbilityEffect::Scry(1)), mode(None, AbilityEffect::GainLife(3))], 5);
    cast(&mut engine, vec![0, 1], Vec::new());
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingScryChoice { .. }), "{status:?}");
    assert_eq!(life(&engine, P0), 20, "the next mode waits for the scry");

    let StepStatus::AwaitingScryChoice { cards, .. } = status else { unreachable!() };
    engine.submit_scry_choice(cards, Vec::new()).unwrap();
    drain(&mut engine);

    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_mode_kind_that_still_cannot_be_combined_is_rejected_at_cast_time() {
    let mut engine = engine_with(vec![mode(None, AbilityEffect::GainLife(3)), mode(None, AbilityEffect::Cascade)], 0);
    let err = try_try_cast(&mut engine, vec![0, 1], Vec::new()).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))), "{err:?}");
    assert_eq!(zone_of_named(&engine, "Test Spell"), vec![ZoneKind::Hand], "state untouched");
}

#[test]
fn a_single_chosen_mode_still_resolves_bare() {
    let mut spell = engine_with(vec![mode(creature(), AbilityEffect::Destroy), mode(None, AbilityEffect::GainLife(3))], 0);
    spell.state_mut().objects.get_mut(&SPELL).unwrap().definition.mode_choice = ModeChoice::Exactly(1);
    cast(&mut spell, vec![0], vec![Target::Object(BEAR)]);
    pass_both(&mut spell);

    assert!(!zone_of_named(&spell, "Bear").contains(&ZoneKind::Battlefield));
    assert_eq!(life(&spell, P0), 20, "only the chosen mode ran");
    assert_resolved_cleanly(&spell);
}
