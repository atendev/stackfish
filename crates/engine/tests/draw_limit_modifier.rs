//! CR 121.2b: "a player can't draw more than one card each turn" effects, a player-level continuous effect outside the CR 613 layers.
//! `commands::propose_draw`, shared by `continue_draw_cards` and the turn-based draw, returns `None` once the limit is hit.
//! `DrawLimitAffected` is `Filter(FilterController)` only, and `cards_drawn_this_turn` counts Library to Hand `ZoneChange`s since the latest `TurnStarted`.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn land_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn drawcards_spell_definition(amount: u32) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Draw".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Draw two cards.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_draw".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::DrawCards(amount)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn ward_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 999,
        name: "Ward".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: Vec::new(),
        oracle_text: "You can't draw more than one card each turn.".to_string(),
        power: None,
        toughness: None,
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

const YOU_CANT_DRAW_MORE_THAN_ONE: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateDrawLimitModifier(c)
    m:SetAffected(Game.Affected.You)
    m:SetLimit(1)
    m:Register()
end
return s
"#;

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
            ],
        ))
    }
}

// Appends a ledger event directly to seed history for a fixture.
fn push_event(state: &mut GameState, kind: GameEventKind) {
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    state.ledger.push(GameEvent {
        event_id,
        sequence_no,
        kind,
        caused_by_command: None,
        parent_event: None,
    });
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
fn run_card_script_self_registers_a_draw_limit_modifier() {
    let mut state = two_player_state();
    let ward = ObjectId(999);
    let definition = ward_definition(YOU_CANT_DRAW_MORE_THAN_ONE);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(ward);
    state.objects.insert(
        ward,
        card_object(ward, definition.clone(), PlayerId(1), ZoneKind::Battlefield),
    );

    let mut runtime = stackfish_engine::script::ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, ward)
        .expect("script self-registers cleanly");

    assert_eq!(state.draw_limit_modifiers.len(), 1);
    let modifier = &state.draw_limit_modifiers[0];
    assert_eq!(modifier.source, ward);
    assert_eq!(
        modifier.controller,
        PlayerId(1),
        "controller resolved from the ward's own live controller at drain time"
    );
    assert!(matches!(
        modifier.affected,
        DrawLimitAffected::Filter(FilterController::You)
    ));
    assert_eq!(modifier.limit, 1);
}

#[test]
fn effective_draw_limit_filter_forms_and_latest_timestamp_wins() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(1);

    let you_only = vec![DrawLimitModifier {
        source,
        controller: p0,
        affected: DrawLimitAffected::Filter(FilterController::You),
        limit: 1,
        timestamp: SequenceNo(0),
    }];
    assert_eq!(effective_draw_limit(&you_only, p0), Some(1));
    assert_eq!(
        effective_draw_limit(&you_only, p1),
        None,
        "the modifier's own controller isn't p1"
    );

    let opponent_only = vec![DrawLimitModifier {
        source,
        controller: p0,
        affected: DrawLimitAffected::Filter(FilterController::Opponent),
        limit: 1,
        timestamp: SequenceNo(0),
    }];
    assert_eq!(effective_draw_limit(&opponent_only, p0), None);
    assert_eq!(
        effective_draw_limit(&opponent_only, p1),
        Some(1),
        "p1 is p0's opponent"
    );

    // Two conflicting modifiers on one player: the latest timestamp wins, with no stacking, as effective_max_hand_size does.
    let conflicting = vec![
        DrawLimitModifier {
            source,
            controller: p0,
            affected: DrawLimitAffected::Filter(FilterController::Any),
            limit: 1,
            timestamp: SequenceNo(0),
        },
        DrawLimitModifier {
            source,
            controller: p0,
            affected: DrawLimitAffected::Filter(FilterController::Any),
            limit: 2,
            timestamp: SequenceNo(1),
        },
    ];
    assert_eq!(
        effective_draw_limit(&conflicting, p0),
        Some(2),
        "the later-timestamped modifier overrides the earlier one"
    );
}

#[test]
fn cards_drawn_this_turn_counts_only_this_turns_draws_owned_by_that_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = two_player_state();
    let prior_turn_card = card_object(ObjectId(1), land_definition("Old"), p0, ZoneKind::Hand);
    let this_turn_card_p0 = card_object(ObjectId(2), land_definition("Mine"), p0, ZoneKind::Hand);
    let this_turn_card_p1 = card_object(ObjectId(3), land_definition("Theirs"), p1, ZoneKind::Hand);

    // A draw from a previous turn - must not count.
    push_event(
        &mut state,
        GameEventKind::ZoneChange {
            object: ObjectId(1),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(prior_turn_card),
        },
    );
    push_event(
        &mut state,
        GameEventKind::TurnStarted {
            player: p0,
            turn_number: 2,
        },
    );
    push_event(
        &mut state,
        GameEventKind::ZoneChange {
            object: ObjectId(2),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(this_turn_card_p0),
        },
    );
    push_event(
        &mut state,
        GameEventKind::ZoneChange {
            object: ObjectId(3),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(this_turn_card_p1),
        },
    );

    assert_eq!(
        cards_drawn_this_turn(&state, p0),
        1,
        "only the post-TurnStarted draw owned by p0 counts"
    );
    assert_eq!(cards_drawn_this_turn(&state, p1), 1);
}

#[test]
fn drawcards_spell_with_an_active_limit_partially_carries_out() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib_a = ObjectId(0);
    let lib_b = ObjectId(1);
    state.objects.insert(
        lib_a,
        card_object(lib_a, land_definition("Forest"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(lib_a);
    state.objects.insert(
        lib_b,
        card_object(lib_b, land_definition("Island"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(lib_b);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    state.draw_limit_modifiers.push(DrawLimitModifier {
        source: ObjectId(999),
        controller: p0,
        affected: DrawLimitAffected::Filter(FilterController::You),
        limit: 1,
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no crash/pause from the partially-carried-out draw, got {status:?}"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .len(),
        1,
        "CR 121.2b: only the first of the two instructed draws happens - the spell itself left the stack, one card arrived"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Library, Some(p0))
            .unwrap()
            .contents
            .len(),
        1,
        "the second card never left the library"
    );
    assert!(
        !engine.state().player(p0).unwrap().drew_from_empty_library,
        "the block was the limit, not an empty library - that flag must stay unset"
    );
}

#[test]
fn drawcards_spell_with_no_limit_draws_normally() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib_a = ObjectId(0);
    let lib_b = ObjectId(1);
    state.objects.insert(
        lib_a,
        card_object(lib_a, land_definition("Forest"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(lib_a);
    state.objects.insert(
        lib_b,
        card_object(lib_b, land_definition("Island"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(lib_b);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .len(),
        2,
        "no limit active - both cards drawn as normal"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Library, Some(p0))
            .unwrap()
            .contents
            .is_empty()
    );
}

#[test]
fn a_limit_only_affects_the_player_it_targets() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib_a = ObjectId(0);
    let lib_b = ObjectId(1);
    state.objects.insert(
        lib_a,
        card_object(lib_a, land_definition("Forest"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(lib_a);
    state.objects.insert(
        lib_b,
        card_object(lib_b, land_definition("Island"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(lib_b);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    // From p0's view, "your opponents can't draw more than one card each turn" restricts p1 and not p0.
    state.draw_limit_modifiers.push(DrawLimitModifier {
        source: ObjectId(999),
        controller: p0,
        affected: DrawLimitAffected::Filter(FilterController::Opponent),
        limit: 1,
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .len(),
        2,
        "p0 itself is unaffected by a limit scoped to their opponents"
    );
}

#[test]
fn a_turn_based_draw_is_skipped_once_the_limit_was_already_met_this_turn() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = GameState {
        turn_number: 2,
        phase: Phase::Beginning,
        step: Step::Upkeep,
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
            ],
        ))
    };
    state.last_active_turn_number = std::collections::HashMap::from([(p0, 2), (p1, 1)]);
    state.objects.insert(
        card,
        card_object(card, land_definition("Forest"), p0, ZoneKind::Library),
    );
    state
        .zone_mut(ZoneKind::Library, Some(p0))
        .unwrap()
        .push_bottom(card);
    state.draw_limit_modifiers.push(DrawLimitModifier {
        source: ObjectId(999),
        controller: p0,
        affected: DrawLimitAffected::Filter(FilterController::You),
        limit: 1,
        timestamp: SequenceNo(0),
    });
    // p0 already drew a card this turn through an earlier effect, so the ledger shows one draw before the draw step runs.
    push_event(
        &mut state,
        GameEventKind::TurnStarted {
            player: p0,
            turn_number: 2,
        },
    );
    let already_drawn = card_object(
        ObjectId(50),
        land_definition("Already Drawn"),
        p0,
        ZoneKind::Hand,
    );
    push_event(
        &mut state,
        GameEventKind::ZoneChange {
            object: ObjectId(50),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(already_drawn),
        },
    );

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "expected p0's own draw-step priority, got {status:?}"
    );
    assert_eq!(engine.state().step, Step::Draw);
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Library, Some(p0))
            .unwrap()
            .contents
            .len(),
        1,
        "the limit was already met this turn - the turn-based draw must not happen"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .is_empty(),
        "no card actually landed in the real hand zone from this step"
    );
}

// Forces a real retire_object call on the modifier's source.
fn destroy_permanent_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Destroy Permanent".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy target permanent.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_destroy_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: None,
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
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

// CR 611.3b
// A static ability's continuous effect applies only while its source is on the battlefield.
#[test]
fn a_draw_limit_modifier_is_purged_once_its_source_leaves_the_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    state.objects.insert(
        source,
        card_object(source, ward_definition(""), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(source);
    state.draw_limit_modifiers.push(DrawLimitModifier {
        source,
        controller: p0,
        affected: DrawLimitAffected::Filter(FilterController::You),
        limit: 1,
        timestamp: SequenceNo(0),
    });
    let removal = ObjectId(1);
    state.objects.insert(
        removal,
        card_object(removal, destroy_permanent_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(removal);

    assert_eq!(
        effective_draw_limit(&state.draw_limit_modifiers, p0),
        Some(1),
        "the modifier is live before its source is destroyed"
    );

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: removal,
                modes: Vec::new(),
                targets: vec![Target::Object(source)],
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority, spell is in hand");
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().object(source).is_none(),
        "the source ward was destroyed"
    );
    assert!(
        engine.state().draw_limit_modifiers.is_empty(),
        "the modifier must be purged along with its source"
    );
    assert_eq!(
        effective_draw_limit(&engine.state().draw_limit_modifiers, p0),
        None,
        "back to no draw limit at all"
    );
}
