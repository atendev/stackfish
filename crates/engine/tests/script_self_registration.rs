//! `Game.CreateTriggeredAbility(c)` in a card's `register_abilities(c)` registers its triggered ability, in the style of ygopro's `Effect.CreateEffect(c)` (see `docs/ygo_pro_reference.md`).
//! The builder touches only a `ScriptRuntime`-owned `Rc<RefCell<_>>` registry, and `run_card_script` drains it into `state.abilities` after the call returns. Self-registration runs each time a new battlefield id is minted (CR 400.7), from `PlayLand` and `ResolveStackTop`.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state as base_two_player_state,
};

const SELF_REGISTER_GAIN_LIFE_PRIMITIVE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const SELF_REGISTER_GAIN_LIFE_CUSTOM: &str = r#"
local s = {}
function s.register_abilities(c)
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
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const SELF_REGISTER_NO_OPERATION: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:Register()
end
return s
"#;

const SELF_REGISTER_TWO_ABILITIES: &str = r#"
local s = {}
function s.register_abilities(c)
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
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        oracle_text: "When this creature enters the battlefield, you gain 1 life.".to_string(),
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Test Bear", "test_bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        oracle_text: "When this land enters the battlefield, you gain 1 life.".to_string(),
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(2, "Test Land", "test_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

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
fn run_card_script_can_register_multiple_abilities_from_one_script_call() {
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

#[test]
fn dying_creatures_registered_ability_is_purged_by_sba() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let mut object = card_object(bear, bear_definition(SELF_REGISTER_GAIN_LIFE_PRIMITIVE), PlayerId(0), ZoneKind::Battlefield);
    object.damage_marked = 2; // == toughness, lethal (CR 704.5g).
    state.objects.insert(bear, object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    // Simulates the ability the creature would have self-registered on an earlier battlefield entry.
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
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
            kind: PlayerActionKind::CastSpell { card: bear, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("p0 holds priority, bear is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    // The spell is on the stack, so the ETB ability is not registered until the creature reaches the battlefield.
    assert_eq!(engine.state().stack.len(), 1, "only the spell itself is on the stack so far");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no life gain before the creature resolves");
    assert!(engine.state().abilities.is_empty(), "self-registration hasn't run yet - still on the stack");

    // Both players pass, which resolves the spell, runs run_card_script and self-registers the ETB ability, which matches this ZoneChange and goes on the stack in the same run.
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
                kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
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
    // The second creature is still in hand, so its script also ran (hand sweep). Count only abilities whose source is on the battlefield.
    let on_battlefield = |engine: &Engine| {
        engine
            .state()
            .abilities
            .iter()
            .filter(|a| engine.state().object(a.source).is_some_and(|o| o.zone == ZoneKind::Battlefield))
            .count()
    };
    assert_eq!(on_battlefield(&engine), 1, "only one ability registered so far");

    cast_and_resolve(&mut engine, second);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        22,
        "second creature's own self-registered ability fired exactly once - no cross-firing, no double-firing"
    );
    assert_eq!(on_battlefield(&engine), 2, "each creature registered its own distinct ability");
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

    // PlayLand does not use the stack, but the triggered ability it self-registers does (CR 603.3b, 603.4).
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
