//! Phase 5 vertical slice: CR 614.8/701.19 regeneration. Scoped to CR
//! 704.5g's lethal-damage/deathtouch SBA path (the overwhelmingly common
//! real case, "regenerate this creature" in response to lethal damage) and
//! the scripted `Destroy` op - CR 704.5f (zero toughness) and CR 704.5k
//! (world rule) are explicitly NOT destruction and can't be saved by a
//! shield, the real rules nuance this slice exists to get right (see
//! `sba::DyingCandidate`'s own doc comment).
//!
//! `ReplacementPayload::Regenerate` rewrites the proposed destroy
//! `ZoneChange` into a `GameEventKind::Regenerated` instead (CR 614.6: a
//! replaced event never happens, a modified one occurs instead) -
//! `sba::commit_regeneration` then removes all marked damage, taps the
//! permanent, and removes it from combat. `ConsumptionPolicy::OneShot`/
//! `Repeatable` map directly onto CR 701.19a (spell/ability-sourced, one-
//! shot) vs 701.19b (static-ability-sourced, repeatable) - no new machinery
//! needed for that distinction.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{battlefield_graveyard_exile_zones, card_object, drain, two_player_state};

fn creature_definition(toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject { damage_marked, ..card_object(id, creature_definition(toughness), owner, ZoneKind::Battlefield) }
}

fn regenerate_shield(source: ObjectId, consumption: ConsumptionPolicy) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: true },
            },
            payload: ReplacementPayload::Regenerate,
        },
        consumption,
    }
}

/// Distinct source/payload from `regenerate_shield`, for the CR 616.1
/// ordering-choice test - the two effects must be tellable apart by which
/// candidate a test picks.
fn would_die_exile_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..two_player_state(battlefield_graveyard_exile_zones(p0, p1))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// A non-creature permanent standing in for "whatever grants the Exile
/// replacement effect" in the CR 616.1 ordering-choice tests - distinct from
/// the dying creature itself. `event_target_matches` requires a
/// `ReplacementEffect::source` to actually resolve in `state.objects`, so
/// this has to be a real object, not just an id.
fn shield(id: ObjectId, controller: PlayerId) -> GameObject {
    let definition = CardDefinition {
        definition_id: 2,
        name: "Test Shield".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_shield".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    };
    card_object(id, definition, controller, ZoneKind::Battlefield)
}

fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
}

// ---------------------------------------------------------------------
// Unit-level: sba::check_state_based_actions
// ---------------------------------------------------------------------

#[test]
fn a_one_shot_shield_saves_a_creature_with_lethal_damage() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::Regenerated { object } if object == bear));
    let survivor = state.object(bear).expect("regeneration keeps the same ObjectId - no new object minted");
    assert_eq!(survivor.zone, ZoneKind::Battlefield, "never left the battlefield");
    assert_eq!(survivor.damage_marked, 0, "CR 701.19: all damage removed");
    assert!(survivor.tapped, "CR 701.19: the controller taps it");
    assert!(state.replacement_effects.is_empty(), "OneShot is consumed after saving once");
}

#[test]
fn a_one_shot_shield_does_not_save_a_second_lethal_hit() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));

    progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(state.object(bear).is_some(), "saved the first time");

    // A second lethal hit, same turn - the shield is already spent.
    state.object_mut(bear).unwrap().damage_marked = 2;
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }));
    assert!(state.object(bear).is_none(), "destroyed for real this time - no shield left");
}

#[test]
fn a_repeatable_shield_survives_multiple_lethal_hits() {
    // CR 701.19b: a static ability's own regeneration shield reapplies each
    // time - modeled here as ConsumptionPolicy::Repeatable on a shield
    // sourced from the dying creature itself (self_only), same shape a real
    // "regenerate this creature" static ability would register.
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::Repeatable));

    progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(state.replacement_effects.len(), 1, "Repeatable is never consumed");

    state.object_mut(bear).unwrap().damage_marked = 2;
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(matches!(events[0].kind, GameEventKind::Regenerated { object } if object == bear), "saved again");
    assert_eq!(state.replacement_effects.len(), 1, "still not consumed");
}

#[test]
fn a_shield_does_not_save_a_zero_toughness_creature() {
    // CR 704.5f (zero-or-less toughness) is "put into the graveyard," not
    // "destroy" (CR 614.8) - the real rules nuance this slice exists to get
    // right. A regeneration shield must not intercept it.
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 0, 0));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
        "0 toughness dies for real - not interceptable by regeneration"
    );
    let GameEventKind::ZoneChange { object: new_id, .. } = events[0].kind else { unreachable!() };
    assert_eq!(state.object(new_id).unwrap().zone, ZoneKind::Graveyard);
    // The shield never matched at all (is_destroy: false for a 704.5f death)
    // - it was never spent on a death it couldn't actually intercept. But
    // the bear's own ObjectId still retired via CR 400.7 (the zero-toughness
    // death is a real zone change, new id and all), and `phase5/static-
    // regeneration`'s `retire_object` fix purges a self_only shield pinned
    // to a since-retired source - it can never match anything again, so it's
    // gone too, not left sitting as a harmless-looking but permanently dead
    // entry.
    assert_eq!(
        state.replacement_effects.len(),
        0,
        "the unconsumed shield was purged when its pinned source (bear's old, now-retired ObjectId) left the game"
    );
}

#[test]
fn a_regenerating_attacker_is_removed_from_combat() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.combat = Some(CombatState {
        attackers: vec![(bear, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });

    progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(
        state.combat.as_ref().unwrap().attackers.is_empty(),
        "CR 701.19: a regenerating creature is removed from combat"
    );
}

#[test]
fn an_unshielded_creature_is_destroyed_normally() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }));
    assert!(state.object(bear).is_none());
}

#[test]
fn regeneration_and_a_would_die_exile_effect_still_pause_for_a_cr_616_1_ordering_choice() {
    // Proves is_destroy threads correctly across the whole CR 616.1
    // pause/resume path - a real risk this slice's own scoping report
    // flagged, not left silently unverified.
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    put_on_battlefield(&mut state, shield(ObjectId(1), p0));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(would_die_exile_effect(ObjectId(1)));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::NeedsOrderingChoice { candidates, affected_player, .. } = outcome else {
        panic!("two matching replacements (one a regeneration shield) on the same death must pause");
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(affected_player, p0);
    // Nothing applied yet.
    assert_eq!(state.object(bear).unwrap().damage_marked, 2);
    assert_eq!(state.replacement_effects.len(), 2);
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline, including the CR 616.1 pause/resume
// choosing the regeneration shield specifically.
// ---------------------------------------------------------------------

#[test]
fn engine_resumes_correctly_after_choosing_the_regeneration_shield_in_an_ordering_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    put_on_battlefield(&mut state, shield(ObjectId(1), p0));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(would_die_exile_effect(ObjectId(1)));

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("expected the SBA pass to pause on the CR 616.1 ordering choice, got {status:?}");
    };
    assert_eq!(player, p0);

    let shield_source = candidates.into_iter().find(|&c| c == bear).expect("the regeneration shield's own source (bear, self_only) is a candidate");
    engine.submit_replacement_choice(shield_source).expect("a real candidate must be accepted");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "SBA settles, priority reopens: {status:?}");

    let survivor = engine.state().object(bear).expect("regeneration kept the same ObjectId, still on the battlefield");
    assert_eq!(survivor.zone, ZoneKind::Battlefield);
    assert_eq!(survivor.damage_marked, 0);
    assert!(survivor.tapped);
    assert!(
        engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty(),
        "the Exile effect must not also have applied - only the chosen one wins"
    );
}

// ---------------------------------------------------------------------
// The scripted `AbilityEffect::Destroy` op also checks for a regeneration
// shield now (this slice widened scope beyond just the SBA path once the
// plumbing turned out cheap to share - see docs/implementation.md).
// ---------------------------------------------------------------------

/// "Test Destroy: Destroy target creature."
fn destroy_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Destroy".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_destroy".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
    }
}

fn state_with_hand_zones() -> GameState {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.zones.push(Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() });
    state.zones.push(Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() });
    state
}

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state.zone_mut(ZoneKind::Hand, Some(owner)).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn a_destroy_spell_is_intercepted_by_a_regeneration_shield() {
    // p0 (already active/holding priority in base_state()) targets their
    // own creature - unusual on a real card, but a fine way to exercise the
    // mechanic without needing to pass priority to a second player first.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let destroy_spell = ObjectId(1);
    let mut state = state_with_hand_zones();
    // No pre-existing damage - the only thing that should destroy this bear
    // is the Destroy spell itself, so the shield is still up when it does.
    put_on_battlefield(&mut state, creature(bear, p0, 2, 0));
    put_in_hand(&mut state, card_object(destroy_spell, destroy_spell_definition(), p0, ZoneKind::Hand));
    state.replacement_effects.push(regenerate_shield(bear, ConsumptionPolicy::OneShot));

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, destroy_spell, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let survivor = engine.state().object(bear).expect("regeneration intercepted the Destroy - same ObjectId, still around");
    assert_eq!(survivor.zone, ZoneKind::Battlefield);
    assert_eq!(survivor.damage_marked, 0);
    assert!(survivor.tapped);
    assert!(engine.state().replacement_effects.is_empty(), "the OneShot shield was consumed");
}
