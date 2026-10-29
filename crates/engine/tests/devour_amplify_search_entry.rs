//! CR 614.12a: the Devour (CR 702.82a) and Amplify (CR 702.38a) choice pauses also run when a creature enters the battlefield through `Game.Ops.Search`.
//! `ScriptedZoneMoveThen` (`StackResolution` or `SearchResume`) decides what happens once the sacrifice queue drains, so a Devour choice raised mid-Search still finishes the search.
//! The last test nests a CR 616.1 ordering choice inside a sacrifice and must still resume the search.
// TODO: PlayLand and CreateToken do not run the Devour or Amplify choice (see engine/action.rs and engine/resolve/tokens.rs)

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones};

fn creature_definition(definition_id: u64, name: &str, subtypes: Vec<Subtype>, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn shield_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn exile_instead_effect(source: ObjectId, self_only: bool, consumption: ConsumptionPolicy) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {
                    card_types: Some(CardTypeSet::CREATURE),
                    controller: Some(FilterController::You),
                    self_only,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption,
    }
}

const DEVOUR_TWO: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersDevour(2))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

const AMPLIFY_ONE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersAmplify(1))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

fn creature_filter() -> FilterSpec {
    FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None }
}

fn search_spell_definition(destination: SearchDestination) -> CardDefinition {
    CardDefinition {
        definition_id: 50,
        name: "Test Creature Tutor".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Search your library for a creature card.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_creature_tutor".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Search { filter: creature_filter(), count: 1, destination, shuffle_after: true }),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn base_state(library: Vec<ObjectId>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..stackfish_testutil::two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile, ZoneKind::Library],
        ))
    };
    for id in library {
        state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(id);
    }
    state
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state.zone_mut(ZoneKind::Hand, Some(owner)).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
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
fn search_fetches_a_devour_creature_and_pauses_for_the_search_devour_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let devour_creature = ObjectId(10);
    let bear_a = ObjectId(1);
    let bear_b = ObjectId(2);
    let card = ObjectId(3);
    let mut state = base_state(vec![devour_creature]);
    state.objects.insert(
        devour_creature,
        card_object(devour_creature, creature_definition(1, "Gluttonous Beast", Vec::new(), DEVOUR_TWO), p0, ZoneKind::Library),
    );
    put_on_battlefield(&mut state, card_object(bear_a, creature_definition(2, "Bear A", Vec::new(), ""), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(bear_b, creature_definition(3, "Bear B", Vec::new(), ""), p0, ZoneKind::Battlefield));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else { panic!("expected a Search pause, got {status:?}") };
    assert_eq!(candidates, vec![devour_creature]);

    engine.submit_search_choice(vec![devour_creature]).expect("devour_creature is the only candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingSearchDevourChoice { player, object, per_creature, candidates } = status else {
        panic!("expected a Devour choice raised mid-Search, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(per_creature, 2);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|id| id.0);
    let mut expected = vec![object, bear_a, bear_b];
    expected.sort_by_key(|id| id.0);
    assert_eq!(sorted, expected, "the entrant itself plus both bears are all legal candidates");

    engine.submit_search_devour_choice(vec![bear_a, bear_b]).expect("both bears were offered candidates");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "the search must finish resolving cleanly, got {status:?}");

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 4, "2 per creature x 2 sacrificed");
    assert!(engine.state().object(bear_a).is_none(), "bear_a was sacrificed");
    assert!(engine.state().object(bear_b).is_none(), "bear_b was sacrificed");
    assert!(engine.state().object(devour_creature).is_none(), "the old library id retired (CR 400.7) once fetched");
}

#[test]
fn search_devour_choosing_nothing_still_resolves_the_search_with_no_counters() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let devour_creature = ObjectId(10);
    let card = ObjectId(3);
    let mut state = base_state(vec![devour_creature]);
    state.objects.insert(
        devour_creature,
        card_object(devour_creature, creature_definition(1, "Gluttonous Beast", Vec::new(), DEVOUR_TWO), p0, ZoneKind::Library),
    );
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![devour_creature]).expect("devour_creature is the only candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingSearchDevourChoice { object, .. } = status else { panic!("expected a Devour choice pause, got {status:?}") };

    engine.submit_search_devour_choice(Vec::new()).expect("choosing to sacrifice nothing is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 0);
    let shuffled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0));
    assert!(shuffled, "the search's own shuffle_after must still run once the devour pause resolves");
}

// The chosen sacrifice raises a nested CR 616.1 ordering pause, and `SearchResume` must survive it and hand control back to the search.
#[test]
fn search_devour_sacrifice_can_raise_a_nested_ordering_choice_and_still_resume_the_search() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let devour_creature = ObjectId(10);
    let victim = ObjectId(1);
    let shield_obj = ObjectId(2);
    let card = ObjectId(3);
    let mut state = base_state(vec![devour_creature]);
    state.objects.insert(
        devour_creature,
        card_object(devour_creature, creature_definition(1, "Gluttonous Beast", Vec::new(), DEVOUR_TWO), p0, ZoneKind::Library),
    );
    put_on_battlefield(&mut state, card_object(victim, creature_definition(2, "Victim", Vec::new(), ""), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(shield_obj, shield_definition(3, "Test Shield"), p0, ZoneKind::Battlefield));
    // Two distinct sources both match victim's would-die event, forcing a real 2-way CR 616.1 choice.
    state.replacement_effects.push(exile_instead_effect(victim, true, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(shield_obj, false, ConsumptionPolicy::OneShot));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![devour_creature]).expect("devour_creature is the only candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingSearchDevourChoice { object, candidates, .. } = status else {
        panic!("expected a Devour choice pause, got {status:?}")
    };
    assert!(candidates.contains(&victim));

    engine.submit_search_devour_choice(vec![victim]).expect("victim was an offered candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingReplacementChoice { candidates: repl_candidates, .. } = status else {
        panic!("victim's own 2 matching replacement effects must raise a nested CR 616.1 pause mid-sacrifice, got {status:?}")
    };
    assert_eq!(repl_candidates.len(), 2);
    // The devour counters are not granted yet, since only the sacrifice's inner pause has fired.
    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 0);

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(victim))
        .expect("victim (self_only) is a real candidate");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "the search must resolve cleanly all the way through after the nested pause, got {status:?}"
    );

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 2, "1 creature sacrificed x 2 per creature");
    assert!(engine.state().object(victim).is_none(), "victim's old id retired");
    assert!(
        !engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty(),
        "the chosen exile-instead effect really applied - victim should be in Exile, not the graveyard"
    );
    let shuffled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0));
    assert!(shuffled, "the search's own shuffle_after must still run after both pauses resolve");
}

#[test]
fn search_devour_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let devour_creature = ObjectId(10);
    let card = ObjectId(3);
    let mut state = base_state(vec![devour_creature]);
    state.objects.insert(
        devour_creature,
        card_object(devour_creature, creature_definition(1, "Gluttonous Beast", Vec::new(), DEVOUR_TWO), p0, ZoneKind::Library),
    );
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![devour_creature]).unwrap();
    drain(&mut engine);

    let err = engine.submit_search_devour_choice(vec![ObjectId(999)]).unwrap_err();
    assert!(matches!(err, engine::DevourChoiceError::InvalidCandidate));
}

#[test]
fn search_fetches_an_amplify_creature_and_pauses_for_the_search_amplify_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let amplify_creature = ObjectId(10);
    let hand_a = ObjectId(1);
    let hand_b = ObjectId(2);
    let card = ObjectId(3);
    let mut state = base_state(vec![amplify_creature]);
    state.objects.insert(
        amplify_creature,
        card_object(amplify_creature, creature_definition(1, "Zombie Giant", vec![zombie.clone()], AMPLIFY_ONE), p0, ZoneKind::Library),
    );
    put_in_hand(&mut state, card_object(hand_a, creature_definition(2, "Hand Zombie A", vec![zombie.clone()], ""), p0, ZoneKind::Hand));
    put_in_hand(&mut state, card_object(hand_b, creature_definition(3, "Hand Zombie B", vec![zombie], ""), p0, ZoneKind::Hand));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![amplify_creature]).expect("amplify_creature is the only candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingSearchAmplifyChoice { player, object, per_card, candidates } = status else {
        panic!("expected an Amplify choice raised mid-Search, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(per_card, 1);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|id| id.0);
    let mut expected = vec![hand_a, hand_b];
    expected.sort_by_key(|id| id.0);
    assert_eq!(sorted, expected);

    engine.submit_search_amplify_choice(vec![hand_a, hand_b]).expect("both hand cards were offered candidates");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "the search must finish resolving cleanly, got {status:?}");

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 2, "1 per card x 2 revealed");
    assert!(engine.state().object(hand_a).is_some(), "revealing doesn't move the card out of hand");
    assert!(engine.state().object(hand_b).is_some(), "revealing doesn't move the card out of hand");
}

#[test]
fn search_amplify_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let zombie = Subtype("Zombie".to_string());
    let amplify_creature = ObjectId(10);
    let hand_a = ObjectId(1);
    let card = ObjectId(3);
    let mut state = base_state(vec![amplify_creature]);
    state.objects.insert(
        amplify_creature,
        card_object(amplify_creature, creature_definition(1, "Zombie Giant", vec![zombie.clone()], AMPLIFY_ONE), p0, ZoneKind::Library),
    );
    put_in_hand(&mut state, card_object(hand_a, creature_definition(2, "Hand Zombie", vec![zombie], ""), p0, ZoneKind::Hand));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![amplify_creature]).unwrap();
    drain(&mut engine);

    let err = engine.submit_search_amplify_choice(vec![ObjectId(999)]).unwrap_err();
    assert!(matches!(err, engine::AmplifyChoiceError::InvalidCandidate));
}

#[test]
fn a_plain_creature_fetched_via_search_never_pauses_for_either_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let plain_creature = ObjectId(10);
    let card = ObjectId(3);
    let mut state = base_state(vec![plain_creature]);
    state.objects.insert(plain_creature, card_object(plain_creature, creature_definition(1, "Plain Bear", Vec::new(), ""), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: false }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![plain_creature]).expect("plain_creature is the only candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no Devour/Amplify effect registered - no pause, got {status:?}");

    let battlefield: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(battlefield.len(), 1, "the found creature entered the battlefield normally");
}
