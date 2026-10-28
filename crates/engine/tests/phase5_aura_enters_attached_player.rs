//! CR 303.4f/g, 303.4b: an Aura entering through `SearchDestination::Battlefield::enters_attached` may attach to a player.
//! One candidate attaches at once, several pause on `AwaitingEntryAttachChoice`, and none leaves the Aura in the library.
//! An object submitted against a player-only candidate set is rejected like any unoffered id.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{AttachChoiceError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn aura_enchant_player_definition(id: u64, name: &str, filter: TargetFilter) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Test Aura: enchant player.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter,
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn search_spell_definition(destination: SearchDestination) -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Search Spell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Search your library for an Aura card and put it onto the battlefield \
            attached to a creature or player."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_search_attach_player".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Search {
            filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            count: 1,
            destination,
            shuffle_after: true,
        }),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state(library: Vec<ObjectId>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut zones = hand_battlefield_graveyard_zones(p0, p1);
    zones.push(Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::from(library) });
    zones.push(Zone::new(ZoneKind::Library, Some(p1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones)
    }
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

// Scripted-filter tests patch the real filter on after the fixture is built, because a `ScriptHandle` is valid only against its own runtime.
fn engine_with_aura_in_library(aura: ObjectId, card: ObjectId) -> Engine {
    let p0 = PlayerId(0);
    let mut state = two_player_state(vec![aura]);
    state.objects.insert(
        aura,
        card_object_in(aura, aura_enchant_player_definition(1, "Test Aura", TargetFilter::Player { controller: None }), p0, ZoneKind::Library),
    );
    state.objects.insert(
        card,
        card_object_in(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: true }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    Engine::new(state)
}

fn card_object_in(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    stackfish_testutil::card_object(id, definition, owner, zone)
}

fn find_battlefield_object_named<'a>(state: &'a GameState, name: &str) -> Option<&'a GameObject> {
    state.objects.values().find(|o| o.zone == ZoneKind::Battlefield && o.definition.name == name)
}

#[test]
fn aura_enchant_opponent_only_with_one_legal_candidate_attaches_immediately_no_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let card = ObjectId(2);
    let mut engine = engine_with_aura_in_library(aura, card);

    // A scripted opponent-only predicate. Kept beside the built-in filter because only PlayerScript can express "opponent with 10 or less life".
    let handle = engine.load_script("return function(player, source, controller, state) return player ~= controller end").expect("predicate should load");
    engine.state_mut().objects.get_mut(&aura).unwrap().definition.target_requirement = Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::PlayerScript(handle),
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }));

    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else {
        panic!("expected a Search pause, got {status:?}")
    };
    assert_eq!(candidates, vec![aura]);

    engine.submit_search_choice(vec![aura]).expect("aura is the only candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no attach-choice pause with only one legal candidate, got {status:?}");

    let new_aura = find_battlefield_object_named(engine.state(), "Test Aura").expect("the aura entered the battlefield");
    assert_eq!(new_aura.attached_to_player, Some(p1), "the only legal candidate (the opponent) should be attached automatically");
    assert_eq!(new_aura.attached_to, None);
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::AttachedToPlayer { object, player } if object == new_aura.object_id && player == p1)),
        "an AttachedToPlayer event should be logged"
    );
}

#[test]
fn aura_enchant_built_in_opponent_filter_with_one_legal_candidate_attaches_immediately_no_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let card = ObjectId(2);
    let mut engine = engine_with_aura_in_library(aura, card);

    engine.state_mut().objects.get_mut(&aura).unwrap().definition.target_requirement = Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Player { controller: Some(FilterController::Opponent) },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }));

    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else {
        panic!("expected a Search pause, got {status:?}")
    };
    assert_eq!(candidates, vec![aura]);

    engine.submit_search_choice(vec![aura]).expect("aura is the only candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no attach-choice pause with only one legal candidate, got {status:?}");

    let new_aura = find_battlefield_object_named(engine.state(), "Test Aura").expect("the aura entered the battlefield");
    assert_eq!(new_aura.attached_to_player, Some(p1), "the only legal candidate (the opponent) should be attached automatically");
}

#[test]
fn aura_enchant_player_with_two_legal_candidates_pauses_and_resumes_the_search_afterward() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let card = ObjectId(2);
    let mut engine = engine_with_aura_in_library(aura, card);

    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![aura]).expect("aura is a candidate");

    let status = drain(&mut engine);
    let StepStatus::AwaitingEntryAttachChoice { player, attaching: paused_aura, candidates } = status else {
        panic!("expected a CR 303.4f attach-choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    let mut expected = vec![Target::Player(p0), Target::Player(p1)];
    expected.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    assert_eq!(sorted, expected, "a plain enchant-player filter offers every player, including the searcher's own controller");
    assert_eq!(engine.state().object(paused_aura).unwrap().zone, ZoneKind::Battlefield);
    assert_eq!(engine.state().object(paused_aura).unwrap().attached_to_player, None);

    engine.submit_entry_attach_choice(Target::Player(p1)).expect("p1 is an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "the search's remaining work (shuffle, resolve) should finish, got {status:?}");

    assert_eq!(engine.state().object(paused_aura).unwrap().attached_to_player, Some(p1));
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0)),
        "shuffle_after: true should still have run after the pause resolved"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::AttachedToPlayer { object, player } if object == paused_aura && player == p1)),
        "an AttachedToPlayer event should be logged"
    );
}

#[test]
fn aura_entry_attach_choice_rejects_an_object_candidate_when_only_players_were_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let card = ObjectId(2);
    let mut engine = engine_with_aura_in_library(aura, card);

    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![aura]).expect("aura is a candidate");
    drain(&mut engine);

    // No object was ever offered - the candidate set is players only.
    let err = engine.submit_entry_attach_choice(Target::Object(ObjectId(999))).unwrap_err();
    assert!(matches!(err, AttachChoiceError::InvalidCandidate));
}

#[test]
fn aura_enchant_player_with_zero_legal_candidates_stays_in_the_library() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let card = ObjectId(2);
    let mut engine = engine_with_aura_in_library(aura, card);

    let handle = engine.load_script("return function(player, source, controller, state) return false end").expect("predicate should load");
    engine.state_mut().objects.get_mut(&aura).unwrap().definition.target_requirement = Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::PlayerScript(handle),
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }));

    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![aura]).expect("aura is a candidate");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no attach-choice pause with zero candidates, got {status:?}");
    assert!(find_battlefield_object_named(engine.state(), "Test Aura").is_none(), "CR 303.4g: the Aura should remain in the library, never entering the battlefield");
    assert_eq!(engine.state().object(aura).unwrap().zone, ZoneKind::Library, "the Aura's own library object should be untouched");
}
