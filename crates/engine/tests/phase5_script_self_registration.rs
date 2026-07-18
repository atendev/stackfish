//! Phase 5 vertical slice: real card-script self-registration. A card's own
//! Lua `initial_effect(c)` can now call `Game.CreateTriggeredAbility(c)` to
//! register its own triggered ability - mirroring ygopro's
//! `Effect.CreateEffect(c)`/`e1:SetCondition(...)`/`c:RegisterEffect(e1)`
//! convention (see `docs/ygo_pro_reference.md`) - instead of the
//! test/scenario harness building a `RegisteredAbility` by hand via
//! `Engine::load_condition`/`load_operation` (the prior two slices' path,
//! which remains fully supported and unaffected).
//!
//! The registration-time mutability gap flagged since Phase 2 is fixed via a
//! queued side-channel: `Game.CreateTriggeredAbility`'s builder only ever
//! touches a small `Rc<RefCell<_>>` registry `ScriptRuntime` owns - never
//! `GameState` itself. `ScriptRuntime::run_card_script` drains whatever got
//! `:Register()`ed into `state.abilities` only after the whole script call
//! returns.
//!
//! Production call sites (self-registration runs unconditionally, every
//! time a new battlefield object id is minted, per CR 400.7's "no memory of
//! its previous existence"): `commands.rs`'s `PlayLand` arm (via
//! `CommandOutcome::battlefield_entries`, drained in `engine.rs`) and
//! `engine.rs`'s `ResolveStackTop` permanent-entry branch (called directly).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

const SELF_REGISTER_GAIN_LIFE_PRIMITIVE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const SELF_REGISTER_GAIN_LIFE_CUSTOM: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(function(source, controller, event)
        return event ~= nil
           and event.kind == "zone_change"
           and event.object == source
           and event.to == "battlefield"
    end)
    e:SetOperation(function(source, controller)
        return { gain_life = 1 }
    end)
    e:Register()
end
return s
"#;

const SELF_REGISTER_UNCONDITIONAL: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const SELF_REGISTER_NO_OPERATION: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:Register()
end
return s
"#;

const SELF_REGISTER_TWO_ABILITIES: &str = r#"
local s = {}
function s.initial_effect(c)
    local e1 = Game.CreateTriggeredAbility(c)
    e1:SetCondition(Game.Conditions.Enters)
    e1:SetOperation(Game.Ops.GainLife(1))
    e1:Register()

    local e2 = Game.CreateTriggeredAbility(c)
    e2:SetCondition(Game.Conditions.Enters)
    e2:SetOperation(Game.Ops.GainLife(2))
    e2:Register()
end
return s
"#;

fn bear_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "When this creature enters the battlefield, you gain 1 life.".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "When this land enters the battlefield, you gain 1 life.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
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

/// Mirrors `phase5_script_bridge_trigger::two_player_state`.
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
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
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

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::run_card_script's self-registration mechanics,
// without driving the whole Engine pipeline.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_a_primitive_ability() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert_eq!(ability.source, bear);
    assert_eq!(ability.kind, AbilityKind::Triggered);
    assert!(matches!(
        ability.condition,
        Some(ConditionSpec::Primitive(ConditionPrimitive::EntersBattlefield))
    ));
    assert!(matches!(ability.effect, Some(AbilityEffect::GainLife(1))));
}

#[test]
fn run_card_script_self_registers_a_custom_function_ability() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_REGISTER_GAIN_LIFE_CUSTOM);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert_eq!(ability.source, bear);
    assert!(
        matches!(ability.condition, Some(ConditionSpec::Script(_))),
        "a custom function passed to SetCondition must decode to ConditionSpec::Script, not a primitive"
    );
    assert!(ability.effect.is_none(), "a custom function passed to SetOperation has no Rust-native effect");
}

#[test]
fn run_card_script_without_set_condition_registers_unconditionally() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_REGISTER_UNCONDITIONAL);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    assert!(
        state.abilities[0].condition.is_none(),
        "omitting SetCondition means unconditional - same meaning as RegisteredAbility::condition's \
         documented None case"
    );
}

#[test]
fn run_card_script_register_without_set_operation_is_an_error() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_REGISTER_NO_OPERATION);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(result.is_err(), "Register() with no SetOperation() must fail, not silently register a broken ability");
    assert!(state.abilities.is_empty(), "no partial ability should have been committed");
}

#[test]
fn run_card_script_can_register_multiple_abilities_from_one_initial_effect() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_REGISTER_TWO_ABILITIES);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("both abilities register cleanly");

    assert_eq!(
        state.abilities.len(),
        2,
        "multi-ability-per-card falls out of the builder mechanism for free - each :Register() \
         independently queues"
    );
    assert!(state.abilities.iter().all(|a| a.source == bear));
}

// ---------------------------------------------------------------------
// SBA: a dying creature's self-registered ability is purged, not left
// dangling in state.abilities.
// ---------------------------------------------------------------------

#[test]
fn dying_creatures_registered_ability_is_purged_by_sba() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let mut object = card_object(bear, bear_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE), PlayerId(0), ZoneKind::Battlefield);
    object.damage_marked = 2; // == toughness, lethal (CR 704.5g).
    state.objects.insert(bear, object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    // Simulates the ability this creature would have self-registered upon
    // an earlier entrance to the battlefield.
    state.abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::EntersBattlefield)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let script_runtime = ScriptRuntime::new();
    let _events = stackfish_engine::sba::check_state_based_actions(&mut state, &script_runtime);

    assert!(
        state.abilities.iter().all(|a| a.source != bear),
        "the dead creature's registered ability must be purged via retire_object, not left dangling"
    );
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline, cast/play a self-registering card, watch
// its own initial_effect's ability fire for real.
// ---------------------------------------------------------------------

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

#[test]
fn cast_self_registering_creature_gains_life_on_etb_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: bear, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("p0 holds priority, bear is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    // Spell is on the stack, not resolved yet - the ETB ability isn't
    // registered until the creature actually reaches the battlefield.
    assert_eq!(engine.state().stack.len(), 1, "only the spell itself is on the stack so far");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no life gain before the creature resolves");
    assert!(engine.state().abilities.is_empty(), "self-registration hasn't run yet - still on the stack");

    // Both players pass - resolves the spell, moving the creature to the
    // battlefield. This is exactly where run_card_script fires, self-
    // registering the ETB ability, which then matches this same ZoneChange
    // and goes on the stack in this same pipeline run.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().stack.len(), 1, "the self-registered ETB ability itself is now on the stack");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "ability hasn't resolved yet");
    assert_eq!(engine.state().abilities.len(), 1, "self-registration ran exactly once");

    // Both players pass again - resolves the self-registered ability.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "both the spell and its self-registered ability have resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "the self-registered ability's GainLife(1) applied");
}

#[test]
fn casting_two_self_registering_creatures_only_fires_each_ones_own_ability() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let first = ObjectId(0);
    let second = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(first, card_object(first, bear_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(first);
    state.objects.insert(second, card_object(second, bear_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(second);

    let mut engine = Engine::new(state);

    let cast_and_resolve = |engine: &mut Engine, card: ObjectId| {
        engine
            .submit_action(PlayerAction {
                player: p0,
                kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
            })
            .expect("p0 holds priority");
        pay_free_and_drain(engine);
        pass(engine, p0);
        drain(engine);
        pass(engine, p1);
        drain(engine);
        pass(engine, p0);
        drain(engine);
        pass(engine, p1);
        drain(engine);
    };

    cast_and_resolve(&mut engine, first);
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "first creature's own self-registered ability fired");
    assert_eq!(engine.state().abilities.len(), 1, "only one ability registered so far");

    cast_and_resolve(&mut engine, second);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        22,
        "second creature's own self-registered ability fired exactly once - no cross-firing, no double-firing"
    );
    assert_eq!(engine.state().abilities.len(), 2, "each creature registered its own distinct ability");
    assert!(engine.state().stack.is_empty());
}

#[test]
fn play_land_self_registering_ability_fires_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);

    // PlayLand doesn't use the stack directly, but the triggered ability it
    // self-registers still does (CR 603.3b/603.4 - any triggered ability
    // goes on the stack, regardless of source type).
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "the land's self-registered ETB ability is on the stack");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "ability hasn't resolved yet");

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "the land's self-registered ability applied GainLife(1)");
}
