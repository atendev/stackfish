//! Phase 5 vertical slice: `AbilityEffect::DrawCards(n)` is a real
//! replacement-checked zone-change proposal now.
//!
//! Closes the last direct-mutation holdout among zone-change producers:
//! SBA deaths, scripted Destroy/Sacrifice/Exile, Sacrifice/Discard cost
//! payments, cleanup discard, and CR 504.1's turn-based draw were already
//! replacement-checked; `Game.Ops.DrawCards(n)` was the sole remaining
//! `Engine::draw_one_card` caller.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
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
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("forest".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn drawcards_spell_definition(amount: u32) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Draw".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Draw cards.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_draw".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::DrawCards(amount)),
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

fn draws_card_trigger_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DrawsCard)),
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
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
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

fn draw_goes_to_exile_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Library),
                to: Some(ZoneKind::Hand),
                affected: EffectFilter { card_types: None, controller: Some(FilterController::You), self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn un_replaced_scripted_draw_still_works() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let first = ObjectId(0);
    let second = ObjectId(1);
    state.objects.insert(first, card_object(first, land_definition(1, "First"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(first);
    state.objects.insert(second, card_object(second, land_definition(2, "Second"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(second);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    let hand = &engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents;
    assert_eq!(hand.len(), 2, "two cards should have been drawn");
    let hand_names: Vec<&str> =
        hand.iter().map(|id| engine.state().object(*id).unwrap().definition.name.as_str()).collect();
    assert_eq!(hand_names, vec!["First", "Second"], "drawn in library order");
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty());
}

#[test]
fn draws_card_trigger_still_fires_off_un_replaced_scripted_draw() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    state.objects.insert(permanent, card_object(permanent, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(draws_card_trigger_ability(permanent));
    let lib_a = ObjectId(3);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "DrawsCard trigger collected off the scripted draw");

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) from DrawsCard trigger");
}

#[test]
fn empty_library_still_stops_and_sets_sba_flag() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib_a = ObjectId(0);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(1, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p1));
    assert!(engine.state().player(p0).unwrap().eliminated);
    assert!(engine.state().player(p0).unwrap().drew_from_empty_library);
}

#[test]
fn replacement_redirects_a_scripted_draw_to_exile() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(999);
    state.objects.insert(ward, card_object(ward, creature_definition(1, "Ward"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward);
    state.replacement_effects.push(draw_goes_to_exile_effect(ward));

    let lib_a = ObjectId(1);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty(), "redirect kept the card out of hand");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "card exiled instead of drawn");
}

#[test]
fn replacement_applies_to_each_card_of_a_multi_draw() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(999);
    state.objects.insert(ward, card_object(ward, creature_definition(1, "Ward"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward);
    state.replacement_effects.push(draw_goes_to_exile_effect(ward));

    let lib_a = ObjectId(1);
    let lib_b = ObjectId(2);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    state.objects.insert(lib_b, card_object(lib_b, land_definition(3, "Island"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_b);
    let card = ObjectId(3);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty());
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 2, "both drawn cards redirected to exile");
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
}

#[test]
fn two_matching_replacements_pause_for_cr_616_1_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward_a = ObjectId(999);
    let ward_b = ObjectId(1001);
    state.objects.insert(ward_a, card_object(ward_a, creature_definition(1, "Ward A"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward_a);
    state.objects.insert(ward_b, card_object(ward_b, creature_definition(2, "Ward B"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward_b);
    state.replacement_effects.push(draw_goes_to_exile_effect(ward_a));
    state.replacement_effects.push(draw_goes_to_exile_effect(ward_b));

    let lib_a = ObjectId(2);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(3, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    let card = ObjectId(3);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("two matching replacement effects on a scripted draw must pause for a CR 616.1 choice, got {status:?}");
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);
    assert!(engine.state().object(lib_a).is_some(), "nothing committed yet");

    engine.submit_replacement_choice(ward_b).expect("ward_b is a real candidate");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "draw should finish resolving after the choice, got {status:?}"
    );

    assert!(engine.state().object(lib_a).is_none());
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty());
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "either choice exiles the card");
}
