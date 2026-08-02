//! Phase 5 vertical slice: CR 614's general "instead" framing for battlefield
//! entry (`ReplacementPayload::ReplaceZoneDestination` wired to a real
//! `WouldMoveZone(_, Battlefield)` producer for the first time) - e.g. "if a
//! creature would enter the battlefield under an opponent's control, exile
//! it instead" (comp rules' own Gather Specimens example, redirecting to a
//! different zone here rather than a different controller).
//!
//! `ReplaceZoneDestination` and its `WouldMoveZone(_, Battlefield)` pairing
//! already existed and were already validated as legal at `:Register()`
//! time - what was missing was a real producer that proposed the
//! battlefield-entry `ZoneChange` through `replacement::apply_replacements`
//! *before* physically placing anything, mirroring `sba::resolve_deaths`'s
//! own propose-then-commit shape for the graveyard side. `ResolveStackTop`'s
//! resolving-spell path (the only real producer touched this slice, same
//! narrowing every prior mid-ETB slice already took) now does exactly that
//! for a would-be `Battlefield` destination.
//!
//! A redirected permanent never actually enters the battlefield at all - no
//! self-registration, no summoning sickness, none of that "entering"
//! semantics apply, same as CR 608.2b's fizzle already establishes for a
//! different reason.
//!
//! Named, narrow gap: only an *externally* granted redirect effect can ever
//! fire - the entering permanent's own self-registered effects haven't run
//! yet at the point this check happens (its own `run_card_script` only
//! happens once the real destination is already decided), so a card's own
//! "as this would enter, redirect it instead" text isn't reachable this way.
//!
//! CR 616.1's ordering choice for two *simultaneous* redirects is real now
//! too (`phase5/etb-redirect-ordering-choice`) - two different sources each
//! independently redirecting the same would-be battlefield entry pauses on a
//! genuine `AwaitingReplacementChoice` instead of silently doing nothing.
//! Nothing has been placed anywhere yet at this pause point (unlike every
//! other replacement collision this engine already handles), so the whole
//! not-yet-resolved `StackObject` rides along in a new
//! `ReplacementResumeTarget::PermanentEntry` - `resolve_permanent_entry`'s
//! own tail (`move_resolving_spell`, Aura-attach, Copy/Devour/Amplify pause
//! checks) was pulled out into a shared `continue_permanent_entry`
//! continuation so both the fresh path and this resumed path reach the exact
//! same "spell actually enters the battlefield" logic.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

const SELF_REGISTER_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

fn creature_definition(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "If a creature would enter the battlefield under an opponent's control,
/// exile it instead" - a hate-bear-style externally granted redirect,
/// relative to `source`'s own controller.
fn hateful_ward(source: ObjectId, consumption: ConsumptionPolicy) -> ReplacementEffect {
    hateful_ward_to(source, consumption, ZoneKind::Exile)
}

/// Same as `hateful_ward`, but redirecting to an arbitrary `destination` -
/// used to build two simultaneously-matching wards (one to Exile, one to
/// Graveyard) that disagree on where the entering creature ends up, so a CR
/// 616.1 ordering choice between them has an observable difference.
fn hateful_ward_to(source: ObjectId, consumption: ConsumptionPolicy, destination: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {
                    controller: Some(FilterController::Opponent),
                    card_types: Some(CardTypeSet::CREATURE),
                    subtypes: None,
                    excluded_subtypes: None, supertypes: None, excluded_supertypes: None,
                    self_only: false,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(destination),
        },
        consumption,
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(zones(
        PlayerId(0),
        PlayerId(1),
        &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile],
    ))
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
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
fn a_redirected_creature_never_enters_the_battlefield_at_all() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(ward, creature_definition("Hateful Ward", ""), p1, ZoneKind::Battlefield));
    state.replacement_effects.push(hateful_ward(ward, ConsumptionPolicy::Repeatable));
    state.objects.insert(card, card_object(card, creature_definition("Test Creature", SELF_REGISTER_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");

    let bf: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(bf, vec![ward], "only the ward is on the battlefield - the redirected creature never landed there");

    let exile: Vec<ObjectId> = engine.state().zone(ZoneKind::Exile, None).unwrap().contents.iter().copied().collect();
    assert_eq!(exile.len(), 1, "the redirected creature ended up in exile instead");
    let exiled_id = exile[0];
    assert_eq!(engine.state().object(exiled_id).unwrap().definition.name, "Test Creature");

    assert!(
        engine.state().abilities.iter().all(|a| a.source != exiled_id),
        "a redirected permanent never actually entered the battlefield - its own ETB self-registration never ran"
    );
}

#[test]
fn no_matching_redirect_lets_the_creature_enter_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Test Creature", SELF_REGISTER_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(bf.len(), 1, "the creature entered the battlefield normally");
    assert_eq!(
        engine.state().abilities.iter().filter(|a| a.source == bf[0]).count(),
        1,
        "its own ETB self-registration ran normally"
    );
}

#[test]
fn a_redirect_effect_you_dont_control_the_opponent_of_never_matches() {
    // The ward only redirects creatures entering under an *opponent* of its
    // own controller - the ward's own controller's creatures aren't
    // affected by their own ward.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(ward, creature_definition("Hateful Ward", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(hateful_ward(ward, ConsumptionPolicy::Repeatable));
    state.objects.insert(card, card_object(card, creature_definition("Test Creature", SELF_REGISTER_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(bf.len(), 2, "both the ward and the caster's own creature are on the battlefield");
}

#[test]
fn a_one_shot_redirect_only_fires_once() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(0);
    let first_card = ObjectId(1);
    let second_card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(ward, creature_definition("Hateful Ward", ""), p1, ZoneKind::Battlefield));
    state.replacement_effects.push(hateful_ward(ward, ConsumptionPolicy::OneShot));
    state.objects.insert(first_card, card_object(first_card, creature_definition("First Creature", ""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(first_card);
    state.objects.insert(second_card, card_object(second_card, creature_definition("Second Creature", ""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(second_card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, first_card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "the first creature was redirected");
    assert!(engine.state().replacement_effects.iter().all(|r| r.source != ward), "the OneShot ward is consumed after firing");

    cast(&mut engine, p0, second_card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    let bf: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert!(bf.iter().any(|&id| engine.state().object(id).unwrap().definition.name == "Second Creature"), "the second creature entered normally - the ward was already spent");
}

/// CR 616.1: two different sources each independently redirecting the same
/// would-be battlefield entry (one to Exile, one to Graveyard) pauses on a
/// real ordering choice instead of silently applying neither.
#[test]
fn two_simultaneous_redirects_pause_for_a_cr_616_1_ordering_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let exile_ward = ObjectId(0);
    let graveyard_ward = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(exile_ward, creature_definition("Exile Ward", ""), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(graveyard_ward, creature_definition("Graveyard Ward", ""), p1, ZoneKind::Battlefield));
    state.replacement_effects.push(hateful_ward_to(exile_ward, ConsumptionPolicy::Repeatable, ZoneKind::Exile));
    state.replacement_effects.push(hateful_ward_to(graveyard_ward, ConsumptionPolicy::Repeatable, ZoneKind::Graveyard));
    state.objects.insert(card, card_object(card, creature_definition("Test Creature", ""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("expected a CR 616.1 ordering choice between the two wards, got {status:?}");
    };
    assert_eq!(player, p0, "the entering creature's own controller chooses the order (CR 616.1), not the wards' controller");
    assert_eq!(candidates.len(), 2, "both wards matched the same would-be battlefield entry");
    assert!(candidates.contains(&ReplacementCandidate::Registered(exile_ward)));
    assert!(candidates.contains(&ReplacementCandidate::Registered(graveyard_ward)));

    engine.submit_replacement_choice(ReplacementCandidate::Registered(exile_ward)).expect("exile_ward is a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");

    let bf: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(bf.len(), 2, "only the two wards are on the battlefield - the redirected creature landed in neither");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "the chosen (exile) redirect applied");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 0, "the un-chosen (graveyard) redirect never applied - its own filter no longer matches a ZoneChange already rewritten to Exile");
}

/// Same setup as above, but choosing the *other* candidate - proves the
/// outcome genuinely depends on the choice, not a hardcoded winner.
#[test]
fn choosing_the_other_candidate_redirects_to_graveyard_instead() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let exile_ward = ObjectId(0);
    let graveyard_ward = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(exile_ward, creature_definition("Exile Ward", ""), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(graveyard_ward, creature_definition("Graveyard Ward", ""), p1, ZoneKind::Battlefield));
    state.replacement_effects.push(hateful_ward_to(exile_ward, ConsumptionPolicy::Repeatable, ZoneKind::Exile));
    state.replacement_effects.push(hateful_ward_to(graveyard_ward, ConsumptionPolicy::Repeatable, ZoneKind::Graveyard));
    state.objects.insert(card, card_object(card, creature_definition("Test Creature", ""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_replacement_choice(ReplacementCandidate::Registered(graveyard_ward)).expect("graveyard_ward is a real candidate");
    drain(&mut engine);

    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 0, "the un-chosen (exile) redirect never applied");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1, "the chosen (graveyard) redirect applied instead");
}
