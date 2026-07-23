//! Phase 5 vertical slice: CR 702.38a Amplify N - "as this object enters,
//! reveal any number of cards from your hand that share a creature type
//! with it. This permanent enters with N +1/+1 counters on it for each card
//! revealed this way." The last of the four "dynamic ETB counts" backlog
//! items (Bloodthirst/Sunburst/Devour/Amplify).
//!
//! Same category of gap as Devour: `EntersWithCountersAmplify` is never
//! applied by `replacement::apply_enters_battlefield_modifiers` at all -
//! `replacement::pending_amplify_choice` is checked directly by
//! `ResolveStackTop`, before that function runs, pausing on
//! `ProcessState::AwaitingAmplifyChoice`/`StepStatus::AwaitingAmplifyChoice`
//! and resuming via `Engine::submit_amplify_choice`.
//!
//! Unlike Devour, revealing a hand card changes nothing about its zone - no
//! sacrifice, no dies triggers - so `submit_amplify_choice` needs no
//! `continue_scripted_zone_move`; it folds `events_so_far` plus a
//! `CounterAdded` straight into the same `StackObjectResolved`/
//! `ReplacementCheck` tail any other synchronous-only resolution ends on.
//!
//! Candidates are cards in the entering permanent's own controller's hand
//! whose `EffectiveCharacteristics::subtypes` overlap the entering
//! permanent's own - "shares a creature type" approximated as raw subtype
//! overlap (this engine has no separate "creature type" category, same
//! imprecision `landwalk_subtypes`/`bands_with_other_subtypes` already
//! accept). CR 702.38a's "can't reveal cards entering the battlefield at the
//! same time as this card" is unmodeled (no simultaneous-entry producer
//! exists). CR 702.38b (multiple separate Amplify instances) is unmodeled -
//! only the first registered instance is matched.

use stackfish_engine::engine::{AmplifyChoiceError, Engine, StepStatus};
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, mana_cost: ManaCost, subtypes: Vec<Subtype>, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes,
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
    }
}

fn amplify_effect(source: ObjectId, per_card: i32) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter { controller: None, card_types: None, self_only: true },
            },
            payload: ReplacementPayload::EntersWithCountersAmplify { per_card },
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let controller = object.controller;
    state.zone_mut(ZoneKind::Hand, Some(controller)).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

// ---------------------------------------------------------------------
// pending_amplify_choice - unit level.
// ---------------------------------------------------------------------

#[test]
fn pending_amplify_choice_only_offers_hand_cards_sharing_a_subtype() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let human = Subtype("Human".to_string());
    let entrant = ObjectId(0);
    let own_zombie_in_hand = ObjectId(1);
    let own_human_in_hand = ObjectId(2);
    let opponents_zombie_in_hand = ObjectId(3);
    state.objects.insert(
        entrant,
        card_object(entrant, creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], ""), p0, ZoneKind::Battlefield),
    );
    state.replacement_effects.push(amplify_effect(entrant, 1));
    put_in_hand(&mut state, card_object(own_zombie_in_hand, creature_definition("Own Zombie", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], ""), p0, ZoneKind::Hand));
    put_in_hand(&mut state, card_object(own_human_in_hand, creature_definition("Own Human", ManaCost { symbols: Vec::new() }, vec![human], ""), p0, ZoneKind::Hand));
    put_in_hand(&mut state, card_object(opponents_zombie_in_hand, creature_definition("Foe Zombie", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p1, ZoneKind::Hand));

    let runtime = ScriptRuntime::new();
    let (per_card, candidates) =
        replacement::pending_amplify_choice(&state, &runtime, entrant, ZoneKind::Stack, p0).expect("an Amplify effect is registered with real candidates");

    assert_eq!(per_card, 1);
    // Only the own zombie shares a subtype - the human doesn't share one,
    // the opponent's zombie isn't in this controller's hand.
    assert_eq!(candidates, vec![own_zombie_in_hand]);
}

#[test]
fn pending_amplify_choice_is_none_when_hand_has_no_sharing_cards() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let zombie = Subtype("Zombie".to_string());
    let human = Subtype("Human".to_string());
    let entrant = ObjectId(0);
    let hand_card = ObjectId(1);
    state.objects.insert(
        entrant,
        card_object(entrant, creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p0, ZoneKind::Battlefield),
    );
    state.replacement_effects.push(amplify_effect(entrant, 1));
    put_in_hand(&mut state, card_object(hand_card, creature_definition("Human", ManaCost { symbols: Vec::new() }, vec![human], ""), p0, ZoneKind::Hand));

    let runtime = ScriptRuntime::new();
    assert!(replacement::pending_amplify_choice(&state, &runtime, entrant, ZoneKind::Stack, p0).is_none());
}

#[test]
fn pending_amplify_choice_is_none_without_a_registered_amplify_effect() {
    let state = two_player_state();
    let p0 = PlayerId(0);
    let runtime = ScriptRuntime::new();
    assert!(replacement::pending_amplify_choice(&state, &runtime, ObjectId(0), ZoneKind::Stack, p0).is_none());
}

// ---------------------------------------------------------------------
// Lua self-registration.
// ---------------------------------------------------------------------

const AMPLIFY_ONE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersAmplify(1))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

#[test]
fn set_payload_amplify_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let zombie = Subtype("Zombie".to_string());
    let definition = creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie], AMPLIFY_ONE);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { to: Some(ZoneKind::Battlefield), .. },
            payload: ReplacementPayload::EntersWithCountersAmplify { per_card: 1 },
        } => {}
        other => panic!("expected a WouldMoveZone(_, Battlefield)/EntersWithCountersAmplify(1) spec, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// End to end: Lua registration + real CastSpell resolution + the pause.
// ---------------------------------------------------------------------

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
fn revealing_both_offered_cards_grants_two_counters_and_keeps_them_in_hand() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let card = ObjectId(0);
    let hand_a = ObjectId(1);
    let hand_b = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], AMPLIFY_ONE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_in_hand(&mut state, card_object(hand_a, creature_definition("Hand Zombie A", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], ""), p0, ZoneKind::Hand));
    put_in_hand(&mut state, card_object(hand_b, creature_definition("Hand Zombie B", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p0, ZoneKind::Hand));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingAmplifyChoice { player, object, per_card, candidates } = status else {
        panic!("expected a CR 702.38a Amplify choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(per_card, 1);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|id| id.0);
    let mut expected = vec![hand_a, hand_b];
    expected.sort_by_key(|id| id.0);
    assert_eq!(sorted, expected);

    engine.submit_amplify_choice(vec![hand_a, hand_b]).expect("both hand cards were offered candidates");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution should finish cleanly, got {status:?}");

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 2, "1 per card x 2 revealed");
    assert!(engine.state().object(hand_a).is_some(), "revealing doesn't move the card out of hand");
    assert!(engine.state().object(hand_b).is_some(), "revealing doesn't move the card out of hand");
    let hand: Vec<ObjectId> = engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.iter().copied().collect();
    assert!(hand.contains(&hand_a) && hand.contains(&hand_b), "both revealed cards remain in hand");
}

#[test]
fn revealing_nothing_grants_no_counters_but_still_resolves() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let card = ObjectId(0);
    let hand_a = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], AMPLIFY_ONE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_in_hand(&mut state, card_object(hand_a, creature_definition("Hand Zombie", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p0, ZoneKind::Hand));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingAmplifyChoice { object, .. } = status else { panic!("expected an Amplify choice pause, got {status:?}") };

    engine.submit_amplify_choice(Vec::new()).expect("revealing nothing is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 0);
    assert!(engine.state().object(hand_a).is_some(), "the hand card was never touched");
}

#[test]
fn amplify_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], AMPLIFY_ONE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_in_hand(&mut state, card_object(ObjectId(1), creature_definition("Hand Zombie", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p0, ZoneKind::Hand));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_amplify_choice(vec![ObjectId(999)]).unwrap_err();
    assert!(matches!(err, AmplifyChoiceError::InvalidCandidate));
}

#[test]
fn amplify_choice_rejects_a_duplicate_candidate() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let card = ObjectId(0);
    let hand_a = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Zombie Giant", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], AMPLIFY_ONE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_in_hand(&mut state, card_object(hand_a, creature_definition("Hand Zombie", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p0, ZoneKind::Hand));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_amplify_choice(vec![hand_a, hand_a]).unwrap_err();
    assert!(matches!(err, AmplifyChoiceError::DuplicateCandidate));
}

#[test]
fn submit_amplify_choice_while_not_awaiting_is_rejected() {
    let mut engine = Engine::new(two_player_state());
    let err = engine.submit_amplify_choice(Vec::new()).unwrap_err();
    assert!(matches!(err, AmplifyChoiceError::NotAwaitingChoice));
}

#[test]
fn a_creature_without_amplify_never_pauses() {
    let p0 = PlayerId(0);
    let zombie = Subtype("Zombie".to_string());
    let card = ObjectId(0);
    let hand_a = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Plain Zombie", ManaCost { symbols: Vec::new() }, vec![zombie.clone()], ""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_in_hand(&mut state, card_object(hand_a, creature_definition("Other Zombie", ManaCost { symbols: Vec::new() }, vec![zombie], ""), p0, ZoneKind::Hand));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = pay_free_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no Amplify effect registered - no pause, got {status:?}");
}
