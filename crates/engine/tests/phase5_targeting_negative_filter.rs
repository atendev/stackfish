//! Phase 5 vertical slice: CR 601.2c/115/205.4b negative target filters —
//! `TargetFilter::Object`'s own long-standing TODO ("target creature you
//! don't control that isn't a Wall"), left open through every prior
//! targeting slice (subtype/supertype filters, "another target", scripted
//! predicates, "N or more" counts).
//!
//! `TargetFilter::Object` gains `excluded_card_types: Option<CardTypeSet>`
//! ("target nonland permanent") and `excluded_subtypes: Option<Vec<Subtype>>`
//! ("target creature that isn't a Wall") — the negation of the existing
//! `card_types`/`subtypes` "or" matches, checked independently so a filter
//! can combine a positive `card_types` with an `excluded_subtypes` (the
//! worked example above: `card_types: Creature, excluded_subtypes: [Wall],
//! controller: Opponent`).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(id: u64, name: &str, subtypes: Vec<Subtype>) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes,
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
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
        script_ref: ScriptReference::ByKey("land".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// "Test Bolt deals 1 damage to target nonland permanent."
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Bolt deals 1 damage to target nonland permanent.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: None,
                excluded_card_types: Some(CardTypeSet::LAND),
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
    }
}

/// "Test Purge deals 1 damage to target creature that isn't a Wall."
fn purge_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Purge".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Purge deals 1 damage to target creature that isn't a Wall.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_purge".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: Some(vec![Subtype("Wall".to_string())]),
                supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
    }
}

/// This repo's own worked example for the negative-filter gap: "target
/// creature you don't control that isn't a Wall."
fn smite_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 22,
        name: "Test Smite".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Smite deals 1 damage to target creature you don't control that isn't a Wall."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_smite".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: Some(vec![Subtype("Wall".to_string())]),
                supertypes: None,
                controller: Some(FilterController::Opponent),
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
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

fn damage_marked_on(engine: &Engine, id: ObjectId) -> u32 {
    engine.state().object(id).map(|o| o.damage_marked).unwrap_or(0)
}

fn setup(spell: CardDefinition, permanents: Vec<(ObjectId, CardDefinition, PlayerId)>) -> (GameState, ObjectId) {
    let p0 = PlayerId(0);
    let mut state = two_player_state();
    for (id, def, owner) in permanents {
        state.objects.insert(id, card_object(id, def, owner, ZoneKind::Battlefield));
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    }
    let card = ObjectId(1000);
    state.objects.insert(card, card_object(card, spell, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    (state, card)
}

#[test]
fn nonland_filter_rejects_a_land_target() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let (state, card) = setup(bolt_definition(), vec![(land, land_definition(1, "Test Land"), p0)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(land)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)), "got {status:?}");
}

#[test]
fn nonland_filter_accepts_a_creature_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let (state, card) = setup(bolt_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(damage_marked_on(&engine, bear), 1);
}

#[test]
fn isnt_a_wall_filter_rejects_a_wall_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(0);
    let (state, card) =
        setup(purge_definition(), vec![(wall, creature_definition(1, "Test Wall", vec![Subtype("Wall".to_string())]), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(wall)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)), "got {status:?}");
}

#[test]
fn isnt_a_wall_filter_accepts_a_non_wall_creature_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let (state, card) = setup(purge_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(damage_marked_on(&engine, bear), 1);
}

#[test]
fn combined_filter_rejects_your_own_creature() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let (state, card) = setup(smite_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p0)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "\"you don't control\" must still reject your own creature - got {status:?}"
    );
}

#[test]
fn combined_filter_rejects_an_opponents_wall() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(0);
    let (state, card) =
        setup(smite_definition(), vec![(wall, creature_definition(1, "Test Wall", vec![Subtype("Wall".to_string())]), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(wall)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "\"isn't a Wall\" must still reject an opponent's Wall - got {status:?}"
    );
}

#[test]
fn combined_filter_accepts_an_opponents_non_wall_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let (state, card) = setup(smite_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(damage_marked_on(&engine, bear), 1);
}
