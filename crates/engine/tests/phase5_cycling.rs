//! Phase 5 vertical slice: CR 702.29 Cycling / Typecycling.
//!
//! "Cycling [cost]" means "[Cost], Discard this card: Draw a card" (CR
//! 702.29a) — an activated ability that functions only while the card sits
//! in its controller's hand (CR 602.1's own "unless the ability itself says
//! otherwise" carve-out). Two new pieces make this real:
//!
//! - `AdditionalCost::DiscardSelf` (types/cost.rs) — the `Discard` sibling
//!   `TapSelf` already is to `TapPermanents`: always the ability's own
//!   source, never a filter choice (unlike the pre-existing `Discard
//!   { filter }`, which would let the payer discard *any* hand card).
//! - `commands::activate::execute_activate_ability`'s zone check now
//!   requires `Hand` instead of `Battlefield` when the found ability's own
//!   cost contains `DiscardSelf` — matched on cost shape, not a new
//!   `RegisteredAbility` field, same precedent Equip's sorcery-speed check
//!   already set.
//!
//! Typecycling (CR 702.29e) reuses the existing `AbilityEffect::Search`
//! primitive with `destination: Hand` — no new resolution code needed.
//!
//! This slice also closes a real, separate gap found while scoping it: every
//! `run_card_script` call site in this engine was battlefield-entry-only, so
//! a card drawn into hand never got a chance to self-register a hand-only
//! ability at all. `Engine::run_card_script_for` (engine/mod.rs) is now
//! called by every real Library->Hand producer (turn-based draw, effect-
//! driven `Game.Ops.DrawCards`, `Game.Ops.Search`'s hand destination, and
//! opening-hand deal) — the "real producer wiring" tests below prove each of
//! those three surfaces (draw, tutor, opening hand) actually registers the
//! ability, not just that the mechanism works when hand-built by hand.
//!
//! CR 702.29c/d ("when you cycle this card" / "cycles or discards"
//! triggers) are real now too (`phase5/cycling-triggers`) — see
//! `phase5_cycling_triggers.rs` for the full slice.
//!
//! A card leaving hand for any zone other than via a fresh draw still purges
//! its `RegisteredAbility` the same way every other ability in this engine
//! already does (CR 702.29b's "the ability continues to exist... in all
//! other zones" has no real consumer today — nothing queries "does this
//! object have an activated ability" — so that half stays a named,
//! unobserved gap rather than something attempted here).

use std::collections::HashMap;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, single_player_state, two_player_state as base_two_player_state, zones};

// ---------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------

const PLAIN_CYCLING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.DiscardSelf)
    e:SetOperation(Game.Ops.DrawCards(1))
    e:Register()
end
return s
"#;

const TYPECYCLING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.DiscardSelf)
    e:SetOperation(Game.Ops.Search({ card_types = { "Land" } }, 1))
    e:Register()
end
return s
"#;

/// A Cycling-shaped card whose script never actually runs in this test (the
/// `RegisteredAbility` is built by hand instead) — used by the mechanism
/// tests below, which care about `DiscardSelf`/the hand-zone activation
/// override, not about self-registration itself.
fn cycler_definition(oracle_text: &str) -> CardDefinition {
    CardDefinition {
        oracle_text: oracle_text.to_string(),
        ..minimal_card_definition(1, "Test Cycler", "test_cycler", ManaCost { symbols: Vec::new() }, CardTypeSet::SORCERY)
    }
}

/// A real, runnable Cycling card script - used by the self-registration and
/// "real producer wiring" tests, which specifically need `run_card_script`
/// to execute.
fn scripted_cycler_definition(definition_id: u64, name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(definition_id, name, "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::SORCERY)
    }
}

fn filler_definition(definition_id: u64, name: &str) -> CardDefinition {
    minimal_card_definition(definition_id, name, "test_filler", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
}

fn basic_land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..minimal_card_definition(definition_id, name, "test_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn discard_self_cost() -> CostSpec {
    CostSpec { mana: None, additional: vec![AdditionalCost::DiscardSelf], alternative: Vec::new() }
}

fn cycling_ability(source: ObjectId, effect: AbilityEffect) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(discard_self_cost()),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(effect),
        intervening_if: None,
    }
}

/// One player, `Hand`/`Battlefield`/`Graveyard`/`Library` zones, nothing in
/// any of them yet.
fn base_state(p0: PlayerId) -> GameState {
    single_player_state(
        p0,
        vec![
            Zone::new(ZoneKind::Hand, Some(p0)),
            Zone::new(ZoneKind::Battlefield, None),
            Zone::new(ZoneKind::Graveyard, Some(p0)),
            Zone::new(ZoneKind::Library, Some(p0)),
        ],
    )
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("player controls source and holds priority");
}

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::run_card_script's Cycling registration
// mechanics.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_a_cycling_ability() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = base_state(p0);
    let definition = scripted_cycler_definition(1, "Cycler", PLAIN_CYCLING);
    state.objects.insert(card, card_object(card, definition.clone(), p0, ZoneKind::Hand));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, card).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert_eq!(ability.source, card);
    assert_eq!(ability.kind, AbilityKind::Activated);
    let cost = ability.cost.as_ref().expect("AddCost(DiscardSelf) was called - cost must be Some");
    assert!(matches!(cost.additional.as_slice(), [AdditionalCost::DiscardSelf]));
    assert!(matches!(ability.effect, Some(AbilityEffect::DrawCards(1))));
}

#[test]
fn run_card_script_self_registers_a_typecycling_ability() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = base_state(p0);
    let definition = scripted_cycler_definition(1, "Typecycler", TYPECYCLING);
    state.objects.insert(card, card_object(card, definition.clone(), p0, ZoneKind::Hand));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, card).expect("script self-registers cleanly");

    let cost = state.abilities[0].cost.as_ref().expect("AddCost(DiscardSelf) was called");
    assert!(matches!(cost.additional.as_slice(), [AdditionalCost::DiscardSelf]));
    match &state.abilities[0].effect {
        Some(AbilityEffect::Search { filter, count, destination: SearchDestination::Hand, shuffle_after }) => {
            assert_eq!(filter.card_types, Some(CardTypeSet::LAND));
            assert_eq!(*count, 1);
            assert!(*shuffle_after);
        }
        other => panic!("expected Search{{destination: Hand}}, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Mechanism: DiscardSelf validation + the hand-zone activation override,
// via hand-built `RegisteredAbility`s (no script involved).
// ---------------------------------------------------------------------

#[test]
fn plain_cycling_discards_itself_and_draws_a_card() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let filler = ObjectId(1);
    let mut state = base_state(p0);
    state.objects.insert(card, card_object(card, cycler_definition("Cycling {2}"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.objects.insert(filler, card_object(filler, filler_definition(2, "Filler"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(filler);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "DiscardSelf is a real cost - must pause for payment, got {status:?}");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(card)] })
        .expect("discarding the source itself pays DiscardSelf");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "priority stays with the activator, got {status:?}");

    // Sole player - one PassPriority resolves the ability off the stack.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert!(engine.state().object(card).is_none(), "the original card id is retired");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1, "the cycled card itself landed in the graveyard");
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "drew the filler card");
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
}

#[test]
fn cycling_ability_cannot_be_activated_from_the_battlefield() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = base_state(p0);
    state.objects.insert(card, card_object(card, cycler_definition("Cycling {2}"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(card);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))),
        "a DiscardSelf-cost ability must require hand, not battlefield - got {status:?}"
    );
}

#[test]
fn cycling_ability_cannot_be_activated_from_the_graveyard() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = base_state(p0);
    state.objects.insert(card, card_object(card, cycler_definition("Cycling {2}"), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

/// Proves the Hand-zone override is scoped exactly to `DiscardSelf`-cost
/// abilities, not every ability - an ordinary `TapSelf`-cost ability placed
/// on a card sitting in hand must still be rejected.
#[test]
fn an_ordinary_battlefield_ability_still_requires_the_battlefield_not_hand() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = base_state(p0);
    state.objects.insert(card, card_object(card, cycler_definition(""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: card,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn discard_self_payment_must_be_the_source_not_another_hand_card() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let other_hand_card = ObjectId(1);
    let mut state = base_state(p0);
    state.objects.insert(card, card_object(card, cycler_definition("Cycling {2}"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.objects.insert(other_hand_card, card_object(other_hand_card, filler_definition(2, "Other"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(other_hand_card);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(other_hand_card)] })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert!(engine.state().object(card).is_some(), "no partial mutation - the actual cycling card untouched");
    assert!(engine.state().object(other_hand_card).is_some(), "no partial mutation - the other card untouched");
}

#[test]
fn typecycling_discards_itself_and_searches_for_a_land() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let forest = ObjectId(1);
    let mut state = base_state(p0);
    state.objects.insert(card, card_object(card, cycler_definition("Landcycling {1}"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.objects.insert(forest, card_object(forest, basic_land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(forest);
    let search_filter =
        FilterSpec { card_types: Some(CardTypeSet::LAND), supertypes: None, subtypes: None, controller: None, random: false, extra: None };
    state.abilities.push(cycling_ability(
        card,
        AbilityEffect::Search { filter: search_filter, count: 1, destination: SearchDestination::Hand, shuffle_after: true },
    ));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(card)] })
        .expect("discarding the source itself pays DiscardSelf");
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);

    let StepStatus::AwaitingSearchChoice { player, candidates, count } = status else {
        panic!("expected a Search pause, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(count, 1);
    assert_eq!(candidates, vec![forest]);

    engine.submit_search_choice(vec![forest]).expect("forest is the offered candidate");
    drain(&mut engine);

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "the forest is now in hand");
    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(),
        1,
        "the cycled card itself landed in the graveyard"
    );
}

// ---------------------------------------------------------------------
// Real producer wiring: a card's own script actually self-registers its
// Cycling ability once it lands in hand for real, via each of the three
// real Library->Hand producers this engine has.
// ---------------------------------------------------------------------

/// Two players, mid-Beginning-phase turn 2 so a single `pass_both` reaches
/// `Step::Draw` - mirrors `phase5_replaceable_draw.rs::state_at_upkeep_turn_2`.
fn state_at_upkeep_turn_2(p0: PlayerId, p1: PlayerId) -> GameState {
    GameState {
        turn_number: 2,
        phase: Phase::Beginning,
        step: Step::Upkeep,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn a_card_drawn_from_the_library_gets_its_cycling_ability_registered_in_hand() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let filler = ObjectId(2);
    let mut state = state_at_upkeep_turn_2(p0, p1);
    state.objects.insert(card, card_object(card, scripted_cycler_definition(1, "Cycler", PLAIN_CYCLING), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);
    state.objects.insert(filler, card_object(filler, filler_definition(2, "Filler"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(filler);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Draw);

    assert_eq!(engine.state().abilities.len(), 1, "the drawn card's own script self-registered its Cycling ability in hand");
    let hand_id = *engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.front().unwrap();
    assert_eq!(engine.state().abilities[0].source, hand_id);

    // Prove it's really activatable now, not just registered.
    activate(&mut engine, p0, hand_id);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(hand_id)] })
        .expect("discarding the drawn card itself pays DiscardSelf");
    drain(&mut engine);
    pass_both(&mut engine, p0, p1);

    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1, "the cycled card landed in the graveyard");
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "drew the filler card");
}

#[test]
fn opening_hand_cards_get_their_cycling_ability_registered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let settings = GameSettings { skip_mulligan: true, shuffle_libraries: false, ..GameSettings::default() };
    let config = GameConfig {
        players: vec![
            PlayerConfig {
                player_id: p0,
                library: vec![scripted_cycler_definition(1, "Cycler", PLAIN_CYCLING)],
                commanders: Vec::new(),
                life_override: None,
            },
            PlayerConfig {
                player_id: p1,
                library: vec![filler_definition(2, "P1 Filler")],
                commanders: Vec::new(),
                life_override: None,
            },
        ],
        turn_order: vec![p0, p1],
        settings,
        rng_seed: 1,
    };
    let mut engine = Engine::new_game(config);
    loop {
        let result = engine.step();
        if !matches!(result.status, StepStatus::Progressed) {
            break;
        }
    }

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "p0's sole library card became their opening hand");
    assert_eq!(engine.state().abilities.len(), 1, "the opening-hand card's own script self-registered its Cycling ability");
    let hand_id = *engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.front().unwrap();
    assert_eq!(engine.state().abilities[0].source, hand_id);
}

#[test]
fn a_tutored_card_gets_its_cycling_ability_registered_in_hand() {
    let p0 = PlayerId(0);
    let tutor = ObjectId(0);
    let cycler = ObjectId(1);
    let mut state = base_state(p0);
    state.objects.insert(
        tutor,
        card_object(
            tutor,
            minimal_card_definition(9, "Tutor", "tutor", ManaCost { symbols: Vec::new() }, CardTypeSet::ARTIFACT),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(tutor);
    state.objects.insert(cycler, card_object(cycler, scripted_cycler_definition(1, "Cycler", PLAIN_CYCLING), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(cycler);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: tutor,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Search {
            filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            count: 1,
            destination: SearchDestination::Hand,
            shuffle_after: true,
        }),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, tutor);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("the tutor's own ability is free");
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else {
        panic!("expected a Search pause, got {status:?}")
    };
    assert_eq!(candidates, vec![cycler]);
    engine.submit_search_choice(vec![cycler]).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().abilities.len(), 2, "the tutor's own ability plus the tutored card's freshly self-registered Cycling ability");
    let hand_id = *engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.front().unwrap();
    assert!(
        engine.state().abilities.iter().any(|a| a.source == hand_id),
        "the tutored card's own script self-registered its Cycling ability in hand"
    );
}
