//! Phase 5 vertical slice: `Game.CreateActivatedAbility` (CR 602) - a card's
//! own Lua `register_abilities(c)` can now build a real `RegisteredAbility` with
//! `AbilityKind::Activated` and a general `CostSpec`, mirroring
//! `Game.CreateTriggeredAbility`/`Game.CreateStaticAbility`'s builder/
//! queued-registration mechanism exactly (`ActivatedAbilityBuilder` alongside
//! them in script.rs). `:AddCost` accumulates one `Game.Costs.*` component
//! per call, so a single activation can require more than one (e.g. `{T},
//! Sacrifice a creature:`).
//!
//! Same queue as the triggered/static builders (`ScriptRegistry::
//! pending_triggered`, drained into `state.abilities` regardless of
//! `AbilityKind`) and the same 2 self-registration call sites (`PlayLand`,
//! `ResolveStackTop`'s permanent-entry branch) - no new plumbing needed.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const SELF_TAP_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const NO_SET_OPERATION: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:Register()
end
return s
"#;

const NO_COST_AT_ALL: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const COMBINED_COST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:AddCost(Game.Costs.Sacrifice({ card_types = { "Creature" } }))
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const SACRIFICE_SELF_COST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.SacrificeSelf)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const MANA_COST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.Mana("{1}{R}"))
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const MANA_COST_SET_TWICE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.Mana("{1}{R}"))
    e:AddCost(Game.Costs.Mana("{G}"))
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const MALFORMED_MANA_COST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.Mana("{Q}"))
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
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
        oracle_text: "Activated ability self-registration test fixture.".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
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

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::run_card_script's activated self-registration
// mechanics, without driving the whole Engine pipeline.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_an_activated_ability() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_TAP_GAIN_LIFE);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert_eq!(ability.source, bear);
    assert_eq!(ability.kind, AbilityKind::Activated);
    let cost = ability.cost.as_ref().expect("AddCost(TapSelf) was called");
    assert!(cost.mana.is_none());
    assert!(matches!(cost.additional.as_slice(), [AdditionalCost::TapSelf]));
    assert!(matches!(ability.effect, Some(AbilityEffect::GainLife(1))));
}

#[test]
fn run_card_script_register_without_set_operation_is_an_error() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(NO_SET_OPERATION);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(result.is_err(), "Register() with no SetOperation() must fail");
    assert!(state.abilities.is_empty(), "no partial ability should have been committed");
}

#[test]
fn run_card_script_can_register_activated_ability_with_no_cost() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(NO_COST_AT_ALL);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("no AddCost() call is legal - cost: None");

    assert_eq!(state.abilities.len(), 1);
    assert!(state.abilities[0].cost.is_none(), "no AddCost() calls means cost: None, not an empty CostSpec");
}

#[test]
fn run_card_script_accumulates_multiple_add_cost_calls() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(COMBINED_COST);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("two AddCost() calls register cleanly");

    let cost = state.abilities[0].cost.as_ref().expect("both AddCost() calls set a real CostSpec");
    assert_eq!(cost.additional.len(), 2);
    assert!(matches!(cost.additional[0], AdditionalCost::TapSelf));
    assert!(matches!(cost.additional[1], AdditionalCost::Sacrifice { .. }));
}

// ---------------------------------------------------------------------
// Game.Costs.SacrificeSelf — "Sacrifice this permanent," always the
// ability's own source (mirrors Game.Costs.TapSelf/DiscardSelf, distinct
// from the filter-driven Game.Costs.Sacrifice(filter) above).
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_activated_ability_with_sacrifice_self_cost() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SACRIFICE_SELF_COST);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    let cost = state.abilities[0].cost.as_ref().expect("AddCost(SacrificeSelf) was called");
    assert!(matches!(cost.additional.as_slice(), [AdditionalCost::SacrificeSelf]));
}

// ---------------------------------------------------------------------
// Game.Costs.Mana("{...}") — the mana-cost-string parser (6e), wired into
// ActivatedAbilityBuilder::AddCost.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_activated_ability_with_a_mana_cost() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(MANA_COST);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    let cost = state.abilities[0].cost.as_ref().expect("AddCost(Mana(...)) was called");
    let mana = cost.mana.as_ref().expect("Game.Costs.Mana(\"{1}{R}\") set cost.mana");
    assert_eq!(
        mana.symbols,
        vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
        ]
    );
    assert!(matches!(cost.additional.as_slice(), [AdditionalCost::TapSelf]));
}

#[test]
fn run_card_script_rejects_setting_the_mana_cost_twice() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(MANA_COST_SET_TWICE);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(result.is_err(), "a second Game.Costs.Mana(...) call must be rejected");
    assert!(state.abilities.is_empty(), "no partial ability should have been committed");
}

#[test]
fn run_card_script_rejects_a_malformed_mana_cost_string() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(MALFORMED_MANA_COST);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(result.is_err(), "\"{{Q}}\" isn't a recognized mana symbol");
    assert!(state.abilities.is_empty());
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline - activate, pay the self-registered
// cost, resolve.
// ---------------------------------------------------------------------

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

#[test]
fn play_land_self_registered_activated_ability_can_be_activated_paid_and_resolved() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_TAP_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().abilities.len(), 1, "the land's activated ability self-registered");

    let new_land_id = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.front().unwrap();

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: new_land_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("p0 controls the land and holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the {{T}} cost");
    assert!(!engine.state().object(new_land_id).unwrap().tapped, "not tapped until payment commits");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(new_land_id)] })
        .expect("Tap(new_land_id) pays the self-registered TapSelf cost");
    drain(&mut engine);
    assert!(engine.state().object(new_land_id).unwrap().tapped);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "the activated ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "the self-registered GainLife(1) operation resolved");
}

/// End-to-end (6e): an ability whose self-registered cost has a real mana
/// component (`Game.Costs.Mana("{1}{R}")`) alongside `TapSelf` - proves the
/// parsed `ManaCost` reaches `PaymentPlan::validate` unmodified and is paid
/// exactly like a spell's mana cost would be.
#[test]
fn play_land_self_registered_activated_ability_with_mana_cost_can_be_activated_paid_and_resolved() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(land, card_object(land, land_definition(MANA_COST), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    state.player_mut(p0).unwrap().mana_pool = ManaPool { red: 1, colorless: 1, ..ManaPool::default() };

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let new_land_id = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.front().unwrap();

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: new_land_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("p0 controls the land and holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for {{1}}{{R}} + {{T}}");

    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units, phyrexian_life: 0 },
                CostPayment::Tap(new_land_id),
            ],
        })
        .expect("{1}{R} paid with 1 red + 1 generic, plus the TapSelf tap");
    drain(&mut engine);
    assert!(engine.state().object(new_land_id).unwrap().tapped);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.red, 0);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 0);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "the activated ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "the self-registered GainLife(1) operation resolved");
}

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Ability Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "{T}: Gain 1 life.".to_string(),
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
