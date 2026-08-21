//! Phase 5 vertical slice: a custom-function `s.operation()` can now return
//! `draw_cards`/`deal_damage`/`add_counters` on its result table, not just
//! `gain_life`/`sacrifice_chosen`/`discard_chosen` - the exact gap
//! `ScriptRuntime::handle_operation_result`'s own TODO named.
//!
//! `draw_cards` hands back a real `OperationOutcome::PendingDraw`, resolved
//! via the same `Engine::continue_draw_cards` pipeline `Game.Ops.
//! DrawCards(n)`'s primitive-tag fast path already uses - including its own
//! CR 616.1 ordering-choice pause when 2+ replacement effects compete for
//! one draw. `deal_damage`/`add_counters` hand back `OperationOutcome::
//! PendingEvents` - real `DamageDealt`/`CounterAdded` events folded into the
//! resolution's own event list, going through the same `ReplacementCheck`/
//! `CommitEvents` pipeline as the primitive-tag fast path, so prevention/
//! lifelink/deathtouch and CR 613 counter application all apply for real,
//! not just a marker.
//!
//! `ProcessState::AwaitingScriptChoice` gained a `source: ObjectId` field
//! this slice needed - proven end to end by the yield-then-deal-damage test
//! below (a script that first calls `Game.SelectTargets`, then returns
//! `deal_damage` once resumed).

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(definition_id: u64, name: &str, toughness: i32) -> CardDefinition {
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
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn library_card_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Library Card".to_string(),
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Same shape `phase5_script_operation.rs`'s own `pending_script_only_
/// definition` establishes - `effect` is left `None`, patched in after
/// `Engine::new` via `engine.build_operation(source)` so the decoded
/// custom-function handle is valid against the engine's own runtime, not a
/// throwaway one.
fn pending_script_only_definition(definition_id: u64, name: &str, source: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(source.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
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

fn build_pending_operation(engine: &mut Engine, card: ObjectId, source: &str) {
    let (primitive, handle) = engine.build_operation(source).expect("script should load").expect("script declares an operation");
    let effect = primitive.or(Some(AbilityEffect::Script(handle)));
    engine.state_mut().objects.get_mut(&card).unwrap().definition.effect = effect;
}

// ---------------------------------------------------------------------
// draw_cards
// ---------------------------------------------------------------------

const DRAW_TWO_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        return { draw_cards = 2 }
    end
end
return s
"#;

#[test]
fn casting_a_script_declared_custom_function_that_draws_cards_resolves_via_the_script_bridge() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, pending_script_only_definition(1, "Test Draw Two", DRAW_TWO_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    for i in 0..2 {
        let lib_card = ObjectId(10 + i);
        state.objects.insert(lib_card, card_object(lib_card, library_card_definition(2 + i), p0, ZoneKind::Library));
        state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_card);
    }

    let mut engine = Engine::new(state);
    build_pending_operation(&mut engine, card, DRAW_TWO_SCRIPT);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 2, "both cards should have been drawn for real");
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
}

fn draw_goes_to_exile_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Library),
                to: Some(ZoneKind::Hand),
                affected: EffectFilter { card_types: None, controller: Some(FilterController::You), self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

const DRAW_ONE_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        return { draw_cards = 1 }
    end
end
return s
"#;

#[test]
fn a_script_declared_draw_cards_still_pauses_for_a_cr_616_1_ordering_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward_a = ObjectId(900);
    let ward_b = ObjectId(901);
    state.objects.insert(ward_a, card_object(ward_a, creature_definition(1, "Ward A", 2), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward_a);
    state.objects.insert(ward_b, card_object(ward_b, creature_definition(2, "Ward B", 2), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward_b);
    state.replacement_effects.push(draw_goes_to_exile_effect(ward_a));
    state.replacement_effects.push(draw_goes_to_exile_effect(ward_b));
    let lib_card = ObjectId(2);
    state.objects.insert(lib_card, card_object(lib_card, library_card_definition(3), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_card);
    let card = ObjectId(3);
    state.objects.insert(card, card_object(card, pending_script_only_definition(4, "Test Draw One", DRAW_ONE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    build_pending_operation(&mut engine, card, DRAW_ONE_SCRIPT);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("two matching replacement effects on a script-declared draw must still pause for a CR 616.1 choice, got {status:?}");
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ward_b)).expect("ward_b is a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "the draw was redirected to exile");
}

// ---------------------------------------------------------------------
// deal_damage
// ---------------------------------------------------------------------

const DEAL_DAMAGE_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        return { deal_damage = { { target = { kind = "object", id = %TARGET% }, amount = 3 } } }
    end
end
return s
"#;

#[test]
fn casting_a_script_declared_custom_function_that_deals_damage_resolves_via_the_real_pipeline() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let victim = ObjectId(0);
    state.objects.insert(victim, card_object(victim, creature_definition(1, "Victim", 4), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(victim);
    let source = DEAL_DAMAGE_SCRIPT.replace("%TARGET%", &victim.0.to_string());
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, pending_script_only_definition(2, "Test Zap", &source), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    build_pending_operation(&mut engine, card, &source);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().object(victim).unwrap().damage_marked, 3, "real DamageDealt event applied through the normal pipeline");
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::DamageDealt { target: Target::Object(t), amount: 3, .. } if t == victim)),
        "a real DamageDealt event should be in the ledger, not just a marker"
    );
}

// ---------------------------------------------------------------------
// add_counters
// ---------------------------------------------------------------------

const ADD_COUNTERS_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        return { add_counters = { { target = { kind = "object", id = %TARGET% }, counter = "PlusOnePlusOne", amount = 2 } } }
    end
end
return s
"#;

#[test]
fn casting_a_script_declared_custom_function_that_adds_counters_applies_a_real_counter() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Target", 2), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let source = ADD_COUNTERS_SCRIPT.replace("%TARGET%", &target.0.to_string());
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, pending_script_only_definition(2, "Test Grow", &source), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    build_pending_operation(&mut engine, card, &source);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().object(target).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 2);
}

// ---------------------------------------------------------------------
// Combined deal_damage + add_counters in one return table.
// ---------------------------------------------------------------------

const DEAL_DAMAGE_AND_ADD_COUNTERS_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        return {
            deal_damage = { { target = { kind = "object", id = %TARGET% }, amount = 1 } },
            add_counters = { { target = { kind = "object", id = %TARGET% }, counter = "PlusOnePlusOne", amount = 1 } },
        }
    end
end
return s
"#;

#[test]
fn a_single_return_table_can_combine_deal_damage_and_add_counters() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Target", 4), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let source = DEAL_DAMAGE_AND_ADD_COUNTERS_SCRIPT.replace("%TARGET%", &target.0.to_string());
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, pending_script_only_definition(2, "Test Combo", &source), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    build_pending_operation(&mut engine, card, &source);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().object(target).unwrap().damage_marked, 1);
    assert_eq!(engine.state().object(target).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 1);
}

// ---------------------------------------------------------------------
// Resumed-coroutine path: a script that yields via Game.SelectTargets
// first, then returns deal_damage once resumed - proves `source` now
// threads through ProcessState::AwaitingScriptChoice correctly.
// ---------------------------------------------------------------------

const SELECT_TARGET_THEN_DEAL_DAMAGE_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.SelectTargets({ card_types = { "Creature" } })
        return { deal_damage = { { target = chosen, amount = 2 } } }
    end
end
return s
"#;

#[test]
fn a_resumed_coroutine_can_deal_damage_after_yielding_once() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let victim = ObjectId(0);
    state.objects.insert(victim, card_object(victim, creature_definition(1, "Victim", 4), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(victim);
    let card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(card, pending_script_only_definition(2, "Test Delayed Zap", SELECT_TARGET_THEN_DEAL_DAMAGE_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    build_pending_operation(&mut engine, card, SELECT_TARGET_THEN_DEAL_DAMAGE_SCRIPT);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    let StepStatus::AwaitingScriptChoice { player, .. } = status else {
        panic!("expected the operation to yield on Game.SelectTargets, got {status:?}");
    };
    assert_eq!(player, p0);

    engine.submit_script_choice(Target::Object(victim)).expect("victim is a legal creature candidate");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().object(victim).unwrap().damage_marked, 2, "the resumed coroutine's own deal_damage applied for real");
}
