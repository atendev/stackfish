//! Phase 5 vertical slice: `AbilityEffect::Counter` (CR 701.6a) —
//! `Game.Ops.Counter()`, e.g. "Counter target spell." Unlike every other
//! single-target `AbilityEffect`, the affected object isn't reached through
//! `continue_scripted_zone_move`'s usual battlefield-object path — it's a
//! *different*, still-present `StackObject` in `state.stack` itself, found
//! and removed by matching `stack_id` directly, then (for a spell, not an
//! ability) sent to its owner's graveyard via the same `Engine::
//! move_resolving_spell` helper the CR 608.2b fizzle branch already reuses.
//!
//! A spell on the stack already has a real `GameObject` at `zone: Stack`
//! sharing its `StackObject`'s own `stack_id` (`commands::cast::
//! execute_cast_spell`), so `TargetFilter::Object { zone: Some(ZoneKind::
//! Stack), .. }` already targets it correctly via the ordinary targeting
//! pipeline — no new targeting plumbing needed, just the resolution-side
//! effect.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Counter: Counter target spell." No card_types restriction — any
/// spell on the stack is a legal target (CR 701.6a's plain form).
fn counterspell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Counterspell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Counter target spell.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_counter".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: None,
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: Some(ZoneKind::Stack),
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Counter),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Negate-style "Counter target noncreature spell."
fn counter_noncreature_spell_definition() -> CardDefinition {
    CardDefinition {
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: None,
                excluded_card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: Some(ZoneKind::Stack),
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        ..counterspell_definition()
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

/// Puts a creature spell directly on the stack (bypassing the real cast/pay
/// flow, same shortcut `phase5_cast_spell.rs`'s own
/// `cast_instant_allowed_outside_main_phase_and_with_nonempty_stack` test
/// uses) — this slice is testing `Counter`'s own resolution, not casting
/// mechanics, so the target spell doesn't need a real cast history.
fn push_creature_spell_on_stack(state: &mut GameState, stack_id: ObjectId, owner: PlayerId) {
    state.objects.insert(stack_id, card_object(stack_id, creature_definition(1, "Bear"), owner, ZoneKind::Stack));
    state.stack.push(StackObject {
        stack_id,
        kind: StackEntryKind::Spell,
        source: stack_id,
        source_lki: None,
        owner,
        controller: owner,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });
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

#[test]
fn counter_removes_the_targeted_spell_from_the_stack_into_its_owners_graveyard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_creature_spell_on_stack(&mut state, target, p1);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counterspell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2, "both the target and the Counterspell are on the stack");

    // Counterspell resolves first (LIFO).
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the countered spell never gets its own resolution turn");
    assert!(engine.state().object(target).is_none(), "the original creature-spell id is retired");
    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.len(),
        1,
        "the countered spell's card landed in its owner's graveyard"
    );
    assert!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(),
        "the countered creature spell never resolved to the battlefield"
    );
    let countered = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Countered { stack_id } if stack_id == target));
    assert!(countered, "expected a Countered event for the target's own stack_id");
}

#[test]
fn counter_spell_fizzles_if_its_own_target_already_left_the_stack() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_creature_spell_on_stack(&mut state, target, p1);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counterspell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);

    // CR 608.2b: the target leaves the stack (e.g. countered by something
    // else, or otherwise removed) before this Counterspell would resolve.
    engine.state_mut().stack.retain(|so| so.stack_id != target);
    engine.state_mut().objects.remove(&target);

    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    let fizzled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(fizzled, "the Counterspell itself fizzles when its own target is gone");
    let countered = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Countered { .. }));
    assert!(!countered, "no Countered event - nothing was actually countered");
    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(),
        1,
        "the fizzled Counterspell itself lands in its own owner's graveyard"
    );
}

#[test]
fn counter_noncreature_spell_rejects_a_creature_spell_target_at_cast_time() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_creature_spell_on_stack(&mut state, target, p1);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counter_noncreature_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: vec![Target::Object(target)], x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("player holds priority, spell is in hand");
    // Target legality is checked when the queued command is actually
    // processed (`drain`), not synchronously inside `submit_action` — same
    // "queued, not synchronous" shape `phase5_targeting_basics.rs`'s own
    // `cast_spell_rejects_wrong_target_count`/`cast_spell_rejects_
    // unrequested_target` tests already rely on.
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "a creature spell is not a legal target for \"counter target noncreature spell\", got {status:?}"
    );
}

#[test]
fn game_ops_counter_decodes_to_the_counter_effect() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.Counter
        end
        return s
    "#;
    let (effect, _handle) =
        runtime.build_operation(source).expect("script should load").expect("declares an operation");
    assert!(matches!(effect, Some(AbilityEffect::Counter)), "expected Counter, got {effect:?}");
}
