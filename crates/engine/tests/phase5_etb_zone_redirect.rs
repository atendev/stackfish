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
//! CR 616.1's ordering choice for two *simultaneous* redirects is narrowed
//! to "no redirect happens" rather than a real player-choice pause - a
//! genuinely rare combination, same posture this codebase already takes for
//! comparably rare nested-pause interactions.

use stackfish_engine::engine::{Engine, StepStatus};
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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "If a creature would enter the battlefield under an opponent's control,
/// exile it instead" - a hate-bear-style externally granted redirect,
/// relative to `source`'s own controller.
fn hateful_ward(source: ObjectId, consumption: ConsumptionPolicy) -> ReplacementEffect {
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
                    excluded_subtypes: None,
                    self_only: false,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
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
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
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
