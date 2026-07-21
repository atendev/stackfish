//! Phase 5 vertical slice: `AbilityEffect::Exile` (backlog item 8, fifth
//! Script API primitive) — `Game.Ops.Exile()`, e.g. "Exile target creature."
//!
//! Like `Destroy`, exile is a zone move that can't be modeled as an unapplied
//! event — `apply_replacements` is consulted first, then the move is inlined.
//! Unlike `Destroy`, exile ignores indestructibility entirely (CR 702.12b is
//! Destroy-specific). `sba::commit_death` cannot be reused: it hardcodes
//! `Some(owner)` for the destination zone lookup, but the Exile zone is
//! shared (`owner = None`), so that lookup would silently miss and the object
//! would disappear rather than land in exile. The `Exile` arm therefore
//! inlines its own zone move using `state.zone_mut(Exile, None)`.
//!
//! Scoped to battlefield targets only — "exile target creature/permanent" is
//! the overwhelmingly common real card text. Exiling from graveyard/hand
//! requires knowing which zone to remove from at resolution time; deferred.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(definition_id: u64, name: &str, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id,
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
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// "Test Exile: Exile target creature."
fn exile_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Exile".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Exile target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_exile".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Exile),
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

fn leaves_battlefield_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility { id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::LeavesBattlefield)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
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
            // Exile is a single shared zone (owner = None) — same as Battlefield.
            // The engine's own build_initial_state initializes it this way; see
            // the flashback slice's deviation report for why per-player exile
            // zones are wrong (and the move_resolving_spell fix that caught it).
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
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

fn cast_exile_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, target: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: vec![Target::Object(target)],
                x_value: None,
                alternative_cost: None,
            },
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
fn exile_spell_moves_target_from_battlefield_to_exile() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Target Bear", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "exiled creature left the battlefield");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "exiled creature is in the exile zone");
}

#[test]
fn exile_activated_ability_also_works() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let target = ObjectId(1);
    state.objects.insert(source, card_object(source, creature_definition(1, "Exiler", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(target, card_object(target, creature_definition(2, "Prey", KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.abilities.push(RegisteredAbility { id: AbilityId(1),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Exile),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: vec![Target::Object(target)],
                chosen_colors: Vec::new(),
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] }).expect("Tap pays TapSelf");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert!(!bf.contains(target), "target was exiled off the battlefield");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "target is in exile");
}

#[test]
fn exile_fires_leaves_battlefield_trigger() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Watchful Bear", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    // "When this leaves the battlefield, gain 1 life." — exile is a
    // Battlefield → Exile departure, so LeavesBattlefield must fire here
    // even though the creature did not die (no Dies trigger would match).
    state.abilities.push(leaves_battlefield_ability(target));
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);

    // Exile resolves — creature departs via exile, not death.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "creature is in exile");
    assert_eq!(engine.state().stack.len(), 1, "LeavesBattlefield trigger was collected off the exile ZoneChange");

    // Let the trigger resolve.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the LeavesBattlefield trigger");
}

#[test]
fn exile_ignores_indestructible() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    // Destroy would leave an indestructible target untouched (CR 702.12b).
    // Exile has no such restriction — it bypasses indestructibility entirely.
    state.objects.insert(target, card_object(target, creature_definition(1, "Indestructible Bear", KeywordSet::INDESTRUCTIBLE), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "indestructible creature was still exiled");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1);
}

#[test]
fn exile_leaves_unrelated_permanent_untouched() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let bystander = ObjectId(1);
    state.objects.insert(target, card_object(target, creature_definition(1, "Target", KeywordSet::empty()), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.objects.insert(bystander, card_object(bystander, creature_definition(2, "Bystander", KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bystander);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert!(!bf.contains(target), "exiled creature left the battlefield");
    assert!(bf.contains(bystander), "bystander remains on the battlefield");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "only one object in exile");
}
