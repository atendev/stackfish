//! CR 303.4f/g/h: an Aura entering through `SearchDestination::Battlefield { enters_attached }`.
//! One candidate attaches at once, several pause on `AwaitingEntryAttachChoice` (CR 303.4f), none leaves the Aura in the library (CR 303.4g), and a non-Aura enters unattached (CR 303.4h).
//! Player candidates are in aura_enters_attached_player.rs and Equipment in attach_entry_equipment_fortification.rs.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{AttachChoiceError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

fn creature_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn aura_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Test Aura: enchant creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
            },
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
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Search your library for an Aura card and put it onto the battlefield \
            attached to a creature."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_search_attach".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Search {
            filter: FilterSpec {
                card_types: None,
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
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
    zones.push(Zone {
        kind: ZoneKind::Library,
        owner: Some(p0),
        contents: VecDeque::from(library),
    });
    zones.push(Zone::new(ZoneKind::Library, Some(p1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones)
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

#[test]
fn aura_with_one_legal_candidate_attaches_immediately_no_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let bear = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![aura]);
    state.objects.insert(
        aura,
        card_object(aura, aura_definition(1, "Test Aura"), p0, ZoneKind::Library),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(2, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            search_spell_definition(SearchDestination::Battlefield {
                tapped: false,
                enters_attached: true,
            }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else {
        panic!("expected a Search pause, got {status:?}")
    };
    assert_eq!(candidates, vec![aura]);

    engine
        .submit_search_choice(vec![aura])
        .expect("aura is the only candidate");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no attach-choice pause with only one legal candidate, got {status:?}"
    );

    let battlefield: Vec<ObjectId> = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .copied()
        .collect();
    let new_aura = *battlefield
        .iter()
        .find(|id| **id != bear)
        .expect("the aura entered the battlefield");
    assert_eq!(
        engine.state().object(new_aura).unwrap().attached_to,
        Some(bear)
    );
    assert_eq!(
        engine.state().object(bear).unwrap().attachments,
        vec![new_aura]
    );
}

#[test]
fn aura_with_two_legal_candidates_pauses_and_resumes_the_search_afterward() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let bear_a = ObjectId(11);
    let bear_b = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![aura]);
    state.objects.insert(
        aura,
        card_object(aura, aura_definition(1, "Test Aura"), p0, ZoneKind::Library),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_a,
            creature_definition(2, "Bear A"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_b,
            creature_definition(3, "Bear B"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            search_spell_definition(SearchDestination::Battlefield {
                tapped: false,
                enters_attached: true,
            }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine
        .submit_search_choice(vec![aura])
        .expect("aura is a candidate");

    let status = drain(&mut engine);
    let StepStatus::AwaitingEntryAttachChoice {
        player,
        attaching: paused_aura,
        candidates,
    } = status
    else {
        panic!("expected a CR 303.4f attach-choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    let mut expected = vec![Target::Object(bear_a), Target::Object(bear_b)];
    expected.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    assert_eq!(sorted, expected, "both bears are legal enchant targets");
    // The Aura is already an unattached battlefield permanent when this pause is raised.
    assert_eq!(
        engine.state().object(paused_aura).unwrap().zone,
        ZoneKind::Battlefield
    );
    assert_eq!(
        engine.state().object(paused_aura).unwrap().attached_to,
        None
    );

    engine
        .submit_entry_attach_choice(Target::Object(bear_b))
        .expect("bear_b is an offered candidate");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "the search's remaining work (shuffle, resolve) should finish, got {status:?}"
    );

    assert_eq!(
        engine.state().object(paused_aura).unwrap().attached_to,
        Some(bear_b)
    );
    assert_eq!(
        engine.state().object(bear_b).unwrap().attachments,
        vec![paused_aura]
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0)),
        "shuffle_after: true should still have run after the pause resolved"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Attached { object, attached_to } if object == paused_aura && attached_to == bear_b)),
        "an Attached event should be logged"
    );
}

#[test]
fn aura_entry_attach_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let bear_a = ObjectId(11);
    let bear_b = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![aura]);
    state.objects.insert(
        aura,
        card_object(aura, aura_definition(1, "Test Aura"), p0, ZoneKind::Library),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_a,
            creature_definition(2, "Bear A"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_b,
            creature_definition(3, "Bear B"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            search_spell_definition(SearchDestination::Battlefield {
                tapped: false,
                enters_attached: true,
            }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine
        .submit_search_choice(vec![aura])
        .expect("aura is a candidate");
    drain(&mut engine);

    // No creature with this id exists anywhere - never a legal candidate.
    let err = engine
        .submit_entry_attach_choice(Target::Object(ObjectId(999)))
        .unwrap_err();
    assert!(matches!(err, AttachChoiceError::InvalidCandidate));
}

#[test]
fn aura_with_zero_legal_candidates_stays_in_the_library() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(10);
    let card = ObjectId(2);
    // No creatures anywhere, so "enchant creature" has no legal candidate.
    let mut state = two_player_state(vec![aura]);
    state.objects.insert(
        aura,
        card_object(aura, aura_definition(1, "Test Aura"), p0, ZoneKind::Library),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            search_spell_definition(SearchDestination::Battlefield {
                tapped: false,
                enters_attached: true,
            }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine
        .submit_search_choice(vec![aura])
        .expect("aura is the only candidate");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .is_empty(),
        "CR 303.4g: no legal enchant target - the Aura never entered the battlefield"
    );
    assert!(
        engine.state().object(aura).is_some(),
        "the Aura's original library object is untouched"
    );
    assert_eq!(engine.state().object(aura).unwrap().zone, ZoneKind::Library);
}

#[test]
fn non_aura_card_with_enters_attached_set_enters_unattached() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(10);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![bear]);
    state.objects.insert(
        bear,
        card_object(
            bear,
            creature_definition(1, "Fetched Bear"),
            p0,
            ZoneKind::Library,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            search_spell_definition(SearchDestination::Battlefield {
                tapped: false,
                enters_attached: true,
            }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine
        .submit_search_choice(vec![bear])
        .expect("bear is the only candidate");

    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "CR 303.4h: a non-Aura just enters unattached, no pause, got {status:?}"
    );

    let battlefield: Vec<ObjectId> = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .copied()
        .collect();
    assert_eq!(battlefield.len(), 1);
    assert_eq!(
        engine.state().object(battlefield[0]).unwrap().attached_to,
        None
    );
}
