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

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
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
            filter: TargetFilter::Object { mana_value: None,
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
            filter: TargetFilter::Object { mana_value: None,
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

/// "Test Exclude: Counter target spell with mana value X." A real `{X}`
/// mana cost - CR 202.3/107.3a's "this casting's own announced X" is only
/// meaningful on a card whose own cost actually has an `{X}` to announce a
/// value for.
fn counter_mana_value_x_spell_definition() -> CardDefinition {
    CardDefinition {
        mana_cost: ManaCost { symbols: vec![ManaSymbol::X] },
        oracle_text: "Counter target spell with mana value X.".to_string(),
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
                mana_value: Some(ManaValueRequirement { comparison: ManaValueComparison::Equal, bound: ManaValueBound::X }),
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

/// Same as `two_player_state`, but p0 starts with `colorless` mana already
/// in their pool - needed to actually pay a real `{X}` cost (`pay_free_and_
/// drain`'s empty `PaymentPlan` only works for a `{0}` cost).
fn two_player_state_with_p0_colorless(colorless: u32) -> GameState {
    let mut state = two_player_state();
    state.players[0].mana_pool.colorless = colorless;
    state
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

/// Same as `push_creature_spell_on_stack`, but with a caller-chosen
/// `mana_cost` (and no `x_value` of its own - CR 202.3b's off-the-stack-only
/// concern doesn't apply since this target is never itself an X spell in
/// these tests) so the target's own mana value can be controlled.
fn push_spell_with_mana_value_on_stack(state: &mut GameState, stack_id: ObjectId, owner: PlayerId, mana_value: u32) {
    let mut definition = creature_definition(1, "Bear");
    definition.mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(mana_value))] };
    state.objects.insert(stack_id, card_object(stack_id, definition, owner, ZoneKind::Stack));
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

fn cast_spell_with_x(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>, x_value: u32) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: Some(x_value), alternative_cost: None, pay_buyback: false },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell_with_x(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>, x_value: u32) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: Some(x_value), alternative_cost: None, pay_buyback: false },
        })
        
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
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: vec![Target::Object(target)], x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a creature spell is not a legal target for \"counter target noncreature spell\", got {err:?}");
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

#[test]
fn counter_with_mana_value_x_counters_a_spell_whose_mana_value_equals_the_announced_x() {
    let mut state = two_player_state_with_p0_colorless(3);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_spell_with_mana_value_on_stack(&mut state, target, p1, 3);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counter_mana_value_x_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell_with_x(&mut engine, p0, card, vec![Target::Object(target)], 3);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "X=3 matched the target's own mana value 3, cast is legal");
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colorless; 3], phyrexian_life: 0 }],
        })
        .expect("3 colorless covers {X} with X=3");
    drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().stack.is_empty());
    assert!(engine.state().object(target).is_none(), "the mana-value-3 spell was countered");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.len(), 1);
    let countered = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Countered { stack_id } if stack_id == target));
    assert!(countered);
}

#[test]
fn counter_with_mana_value_x_rejects_a_target_whose_mana_value_does_not_equal_x() {
    let mut state = two_player_state_with_p0_colorless(2);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    // Target's own mana value is 3, but this Counterspell announces X=2 -
    // CR 601.2c/115: not a legal target at all, rejected before the cast
    // even goes on the stack (same "checked at cast time, not just
    // resolution" treatment every other TargetFilter key already gets).
    push_spell_with_mana_value_on_stack(&mut state, target, p1, 3);

    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counter_mana_value_x_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell_with_x(&mut engine, p0, card, vec![Target::Object(target)], 2).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "mana value 3 != announced X=2, got {err:?}");
}

#[test]
fn mana_value_equals_x_decodes_correctly() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { zone = "Stack", mana_value_equals = "X", count = 1 }
        end
        return s
    "#;
    let spec = runtime.build_target_requirement(source).expect("script should load").expect("declares a target_requirement");
    let TargetSpec::Primitive(req) = spec else { panic!("expected a Primitive requirement") };
    match req.filter {
        TargetFilter::Object { zone: Some(ZoneKind::Stack), mana_value: Some(mv), .. } => {
            assert_eq!(mv.comparison, ManaValueComparison::Equal);
            assert_eq!(mv.bound, ManaValueBound::X);
        }
        other => panic!("expected an Object filter with a mana_value_equals X requirement, got {other:?}"),
    }
}

#[test]
fn mana_value_at_most_decodes_a_fixed_bound() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { zone = "Stack", mana_value_at_most = 3, count = 1 }
        end
        return s
    "#;
    let spec = runtime.build_target_requirement(source).expect("script should load").expect("declares a target_requirement");
    let TargetSpec::Primitive(req) = spec else { panic!("expected a Primitive requirement") };
    match req.filter {
        TargetFilter::Object { mana_value: Some(mv), .. } => {
            assert_eq!(mv.comparison, ManaValueComparison::AtMost);
            assert_eq!(mv.bound, ManaValueBound::Fixed(3));
        }
        other => panic!("expected an Object filter with a mana_value_at_most 3 requirement, got {other:?}"),
    }
}
