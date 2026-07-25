//! Phase 5 vertical slice: a plain spell's own resolution effect (CR
//! 608.2m) declared by its own script, via the optional `s.operation()`
//! module convention - another sibling to `s.register_abilities(c)`/
//! `s.target_requirement()`, not a replacement for either.
//!
//! A plain instant/sorcery has no ability wrapper at all, and
//! `register_abilities(c)` only ever runs once a card becomes a
//! *battlefield* object - a spell that resolves straight to the graveyard
//! never reaches that point, so its own effect can't be declared there
//! either. `operation()` sidesteps this the same way `target_requirement()`
//! already does: called once per `CardDefinition` build, no `GameState`/
//! `ObjectId` needed. Returns the same `Game.Ops.*` tag or custom-function
//! shape `SetOperation` already accepts everywhere else in this API.
//!
//! **Custom-function decode is runtime-instance-bound.** A `Game.Ops.*`
//! primitive tag decodes to a plain Rust value - no dependency on which
//! `ScriptRuntime` did the decoding. A custom Lua function decodes to a
//! `ScriptHandle`, which is only meaningful against the specific runtime
//! that registered it. Building a `CardDefinition` with a throwaway
//! `ScriptRuntime::new()` and then handing it to `Engine::new` (which
//! builds its *own*, separate runtime internally) leaves that handle
//! dangling - the custom function silently never runs. The
//! `script_only_definition` helper below is fine for primitive-tag scripts;
//! `pending_script_only_definition` + a post-`Engine::new` `build_operation`
//! call is the one that's actually safe for a custom-function script (see
//! `ScriptRuntime::build_operation`'s own "Caller trap" doc note).

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::{OperationOutcome, ScriptRuntime};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

// ---------------------------------------------------------------------
// Decode-level: ScriptRuntime::build_operation
// ---------------------------------------------------------------------

#[test]
fn a_card_with_no_operation_function_returns_none() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.register_abilities(c) end
        return s
    "#;
    let operation = runtime.build_operation(source).expect("script should load");
    assert!(operation.is_none(), "a card script with no operation() function has no script-declared effect");
}

#[test]
fn operation_function_decodes_a_primitive_tag() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.GainLife(3)
        end
        return s
    "#;
    let (effect, _handle) =
        runtime.build_operation(source).expect("script should load").expect("declares an operation");
    assert!(matches!(effect, Some(AbilityEffect::GainLife(3))), "expected GainLife(3), got {effect:?}");
}

#[test]
fn operation_function_decodes_a_custom_function_to_a_real_handle() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return function(source, controller)
                return { gain_life = 1 }
            end
        end
        return s
    "#;
    let (effect, handle) =
        runtime.build_operation(source).expect("script should load").expect("declares an operation");
    assert!(effect.is_none(), "a custom function has no Rust-native fast path");
    // `ScriptHandle(0)` is a legitimate first-allocated handle in a fresh
    // registry, not a reserved "unbound" sentinel value - so the only real
    // proof this is a genuine, callable handle is invoking it for real.
    let outcome = runtime
        .invoke_operation(handle, &mut two_player_state(Vec::new()), ObjectId(0), PlayerId(0))
        .expect("a freshly-decoded custom-function handle should be callable");
    assert!(
        matches!(outcome, OperationOutcome::Finished),
        "expected the custom function to run to completion, got {outcome:?}"
    );
}

// ---------------------------------------------------------------------
// End-to-end: casting a script-declared plain spell
// ---------------------------------------------------------------------

const GAIN_THREE_LIFE_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.GainLife(3)
end
return s
"#;

const CUSTOM_GAIN_ONE_LIFE_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        return { gain_life = 1 }
    end
end
return s
"#;

const BOLT_ANY_TARGET_SCRIPT: &str = r#"
local s = {}
function s.target_requirement()
    return { type = "any", count = 1 }
end
function s.operation()
    return Game.Ops.Damage(2)
end
return s
"#;

fn script_only_definition(definition_id: u64, name: &str, source: &str) -> CardDefinition {
    let runtime = ScriptRuntime::new();
    let (effect, handle) = runtime
        .build_operation(source)
        .expect("script should load")
        .expect("script declares an operation");
    let effect = effect.or(Some(AbilityEffect::Script(handle)));
    let target_requirement = runtime.build_target_requirement(source).expect("script should load");
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
        target_requirement,
        alternative_costs: Vec::new(),
        effect,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Same shape `script_only_definition` builds, but with `effect` left as the
/// pre-decode default (`None`) - for a script whose `operation()` resolves
/// to a custom function, which must be decoded through the *same*
/// `ScriptRuntime` that will later invoke it (an `Engine`'s own, not a
/// throwaway one - see this file's own header note on why `Engine::new`
/// can't be hand-fed a handle from elsewhere). Callers patch `effect` in
/// afterward via `engine.build_operation(source)`, mirroring exactly what
/// the scenario crate's own `register_script_operation` does.
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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Creature".to_string(),
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
        script_ref: ScriptReference::ByKey("creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
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

#[test]
fn casting_a_script_declared_primitive_operation_spell_resolves_for_real() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, script_only_definition(1, "Test Gain Three Life", GAIN_THREE_LIFE_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "GainLife(3) should have resolved for real, no script call needed");
    let zone = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert!(!zone.contents.is_empty(), "a resolved sorcery goes to its owner's graveyard");
}

#[test]
fn casting_a_script_declared_custom_function_operation_spell_resolves_via_the_script_bridge() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            pending_script_only_definition(1, "Test Custom Gain One Life", CUSTOM_GAIN_ONE_LIFE_SCRIPT),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    // Decoded through the Engine's own runtime, after construction - a
    // custom-function operation's `ScriptHandle` is only ever valid against
    // the specific `ScriptRuntime` that decoded it (mirrors the scenario
    // crate's own `register_script_operation`).
    let mut engine = Engine::new(state);
    let (primitive, handle) = engine
        .build_operation(CUSTOM_GAIN_ONE_LIFE_SCRIPT)
        .expect("script should load")
        .expect("script declares an operation");
    let effect = primitive.or(Some(AbilityEffect::Script(handle)));
    engine.state_mut().objects.get_mut(&card).unwrap().definition.effect = effect;

    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        21,
        "the custom function's {{gain_life = 1}} intent table should have been invoked via the script bridge"
    );
}

#[test]
fn casting_a_script_declared_targeted_spell_combines_target_requirement_and_operation() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    let card = ObjectId(3);
    state.objects.insert(
        card,
        card_object(card, script_only_definition(1, "Test Bolt", BOLT_ANY_TARGET_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(
            e.kind,
            GameEventKind::DamageDealt { target: Target::Object(t), amount: 2, .. }
            if t == creature
        )),
        // `source` isn't checked - CR 400.7 mints a new stack-object id for
        // the spell the moment it's cast, distinct from `card`'s own
        // pre-cast hand-zone id.
        "the script-declared target_requirement()/operation() pair should have dealt 2 damage to the chosen target"
    );
}

#[test]
fn casting_a_script_declared_spell_at_an_illegal_target_is_rejected() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, script_only_definition(1, "Test Bolt", BOLT_ANY_TARGET_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    let nonexistent = ObjectId(999);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(nonexistent)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
}
