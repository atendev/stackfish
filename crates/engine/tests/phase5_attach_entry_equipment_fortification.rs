//! Phase 5 vertical slice: CR 301.5e/301.6 - Equipment/Fortification
//! entering the battlefield already attached by an effect other than
//! activating Equip/Fortify, the follow-up `phase5_aura_enters_attached.rs`
//! named as still-open ("Equipment's attach legality isn't governed by a
//! `target_requirement` the way an Aura's enchant ability is").
//!
//! Reuses the same `SearchDestination::Battlefield { enters_attached: true }`
//! producer and the same `AwaitingEntryAttachChoice` pause/resume plumbing
//! `phase5_aura_enters_attached.rs` built - `commands::
//! legal_entry_attach_candidates` dispatches by permanent kind instead of
//! reading a `target_requirement` (CR 702.6c: an Equip ability's own quality
//! restrictions don't restrict what the Equipment may be attached to at
//! all), so legality is just "every creature/land on the battlefield."
//!
//! - 1 legal candidate: attaches immediately, no pause (same as Aura).
//! - 2+ legal candidates: the same `AwaitingEntryAttachChoice` pause.
//! - 0 legal candidates: **diverges from Aura.** CR 301.5e/301.6 says the
//!   Equipment/Fortification still enters the battlefield, just unattached -
//!   unlike an Aura (CR 303.4g), which remains in its current zone and never
//!   enters at all.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
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

fn land_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
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

fn equipment_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Equipment".to_string())],
        oracle_text: "Test Equipment.".to_string(),
        power: None,
        toughness: None,
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

fn fortification_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Fortification".to_string())],
        oracle_text: "Test Fortification.".to_string(),
        power: None,
        toughness: None,
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

fn search_spell_definition(destination: SearchDestination) -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Search Spell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Search your library for a card and put it onto the battlefield attached."
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
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
fn equipment_with_one_legal_creature_attaches_immediately_no_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let sword = ObjectId(10);
    let bear = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![sword]);
    state.objects.insert(sword, card_object(sword, equipment_definition(1, "Test Sword"), p0, ZoneKind::Library));
    put_on_battlefield(&mut state, card_object(bear, creature_definition(2, "Bear"), p0, ZoneKind::Battlefield));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: true }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else {
        panic!("expected a Search pause, got {status:?}")
    };
    assert_eq!(candidates, vec![sword]);

    engine.submit_search_choice(vec![sword]).expect("sword is the only candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no attach-choice pause with only one legal candidate, got {status:?}");

    let battlefield: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    let new_sword = *battlefield.iter().find(|id| **id != bear).expect("the sword entered the battlefield");
    assert_eq!(engine.state().object(new_sword).unwrap().attached_to, Some(bear));
    assert_eq!(engine.state().object(bear).unwrap().attachments, vec![new_sword]);
}

#[test]
fn equipment_with_two_legal_creatures_pauses_and_resumes_the_search_afterward() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let sword = ObjectId(10);
    let bear_a = ObjectId(11);
    let bear_b = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![sword]);
    state.objects.insert(sword, card_object(sword, equipment_definition(1, "Test Sword"), p0, ZoneKind::Library));
    put_on_battlefield(&mut state, card_object(bear_a, creature_definition(2, "Bear A"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(bear_b, creature_definition(3, "Bear B"), p0, ZoneKind::Battlefield));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: true }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![sword]).expect("sword is a candidate");

    let status = drain(&mut engine);
    let StepStatus::AwaitingEntryAttachChoice { player, attaching: paused_sword, candidates } = status else {
        panic!("expected a CR 301.5e attach-choice pause, got {status:?}")
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
    assert_eq!(sorted, expected, "both bears are legal equip targets");
    assert_eq!(engine.state().object(paused_sword).unwrap().zone, ZoneKind::Battlefield);
    assert_eq!(engine.state().object(paused_sword).unwrap().attached_to, None);

    engine.submit_entry_attach_choice(Target::Object(bear_b)).expect("bear_b is an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "the search's remaining work (shuffle, resolve) should finish, got {status:?}");

    assert_eq!(engine.state().object(paused_sword).unwrap().attached_to, Some(bear_b));
    assert_eq!(engine.state().object(bear_b).unwrap().attachments, vec![paused_sword]);
}

#[test]
fn equipment_with_zero_legal_creatures_enters_unattached() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let sword = ObjectId(10);
    let card = ObjectId(2);
    // No creatures anywhere - unlike an Aura (which would remain in the
    // library, CR 303.4g), CR 301.5e says the Equipment still enters the
    // battlefield, just unattached.
    let mut state = two_player_state(vec![sword]);
    state.objects.insert(sword, card_object(sword, equipment_definition(1, "Test Sword"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: true }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![sword]).expect("sword is the only candidate");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let battlefield: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(battlefield.len(), 1, "CR 301.5e: the Equipment enters the battlefield despite no legal attach target");
    assert_eq!(engine.state().object(battlefield[0]).unwrap().attached_to, None);
    assert!(engine.state().object(sword).is_none(), "the library object was replaced by the battlefield one");
}

#[test]
fn fortification_with_one_legal_land_attaches_immediately_and_ignores_a_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(10);
    let bear = ObjectId(11);
    let forest = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![wall]);
    state.objects.insert(wall, card_object(wall, fortification_definition(1, "Test Wall"), p0, ZoneKind::Library));
    put_on_battlefield(&mut state, card_object(bear, creature_definition(2, "Bear"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(forest, land_definition(3, "Forest"), p0, ZoneKind::Battlefield));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(SearchDestination::Battlefield { tapped: false, enters_attached: true }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine.submit_search_choice(vec![wall]).expect("wall is the only candidate");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "a single legal land candidate attaches immediately, no pause, got {status:?}");

    let battlefield: Vec<ObjectId> = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    let new_wall = *battlefield.iter().find(|id| **id != bear && **id != forest).expect("the fortification entered the battlefield");
    assert_eq!(engine.state().object(new_wall).unwrap().attached_to, Some(forest), "attached to the land, not the creature present on the battlefield");
    assert_eq!(engine.state().object(bear).unwrap().attachments, Vec::<ObjectId>::new(), "the creature was never a candidate");
}
