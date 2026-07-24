//! Phase 5 vertical slice: `Game.CreateCostModifier` (CR 601.2f). A card's
//! own Lua `initial_effect(c)` can now call `Game.CreateCostModifier(c)` to
//! register a real `CostModifier` onto `GameState.cost_modifiers` - mirrors
//! `Game.CreateStaticAbility`/`Game.CreateReplacementEffect`'s own
//! queue-and-drain builder shape exactly (`CostModifierBuilder` in
//! script.rs). Building one by hand (a direct `CostModifier` struct literal)
//! remains fully supported and unaffected - this only adds the missing Lua
//! construction path the type's own doc comment named as a TODO since
//! `phase5/cost-modifier-order`.
//!
//! `controller` (CR 601.2f's "you"/"opponent" perspective the modifier's own
//! `filter` is relative to) can't be set from Lua directly - the builder has
//! no `&GameState` inside a callback to read the source's live controller
//! from, so `:Register()` queues a placeholder and `ScriptRuntime::
//! run_card_script`'s drain step resolves the real value once it has one.
//!
//! Same self-registration call site as the static-ability slice (`PlayLand`,
//! via `run_card_script`) - no new call site needed.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const CREATURES_COST_ONE_LESS: &str = r#"
local s = {}
function s.initial_effect(c)
    local m = Game.CreateCostModifier(c)
    m:SetFilter({ card_types = { "Creature" } })
    m:SetAmount(-1)
    m:Register()
end
return s
"#;

const NO_SET_FILTER: &str = r#"
local s = {}
function s.initial_effect(c)
    local m = Game.CreateCostModifier(c)
    m:SetAmount(-1)
    m:Register()
end
return s
"#;

const NO_SET_AMOUNT: &str = r#"
local s = {}
function s.initial_effect(c)
    local m = Game.CreateCostModifier(c)
    m:SetFilter({ card_types = { "Creature" } })
    m:Register()
end
return s
"#;

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Cost-Reducing Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "Creature spells you cast cost {1} less to cast.".to_string(),
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
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn bear_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("vanilla_bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
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

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::run_card_script's cost-modifier self-
// registration mechanics, without driving the whole Engine pipeline.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_a_cost_modifier() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(CREATURES_COST_ONE_LESS);
    state.objects.insert(land, card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, land).expect("script self-registers cleanly");

    assert_eq!(state.cost_modifiers.len(), 1);
    let modifier = &state.cost_modifiers[0];
    assert_eq!(modifier.source, land);
    assert_eq!(modifier.controller, PlayerId(0), "resolved from the source's own live controller at drain time");
    assert_eq!(modifier.amount, -1);
    assert!(matches!(modifier.filter.card_types, Some(types) if types == CardTypeSet::CREATURE));
}

#[test]
fn register_without_set_filter_is_an_error() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(NO_SET_FILTER);
    state.objects.insert(land, card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, land);

    assert!(result.is_err(), "Register() with no SetFilter() must fail");
}

#[test]
fn register_without_set_amount_is_an_error() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(NO_SET_AMOUNT);
    state.objects.insert(land, card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, land);

    assert!(result.is_err(), "Register() with no SetAmount() must fail");
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline, PlayLand triggers self-registration,
// the resulting CostModifier is picked up by a real cast's cost resolution.
// ---------------------------------------------------------------------

#[test]
fn play_land_self_registered_cost_modifier_reduces_a_matching_spells_cost_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(land, card_object(land, land_definition(CREATURES_COST_ONE_LESS), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    drain(&mut engine);
    assert_eq!(engine.state().cost_modifiers.len(), 1, "the land's self-registered cost modifier is live");

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: bear,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
            },
        })
        .expect("p0 holds priority, bear is in hand");
    let status = drain(&mut engine);

    // Bear's printed cost is {1} - if the modifier weren't actually applied,
    // an empty payment plan would be rejected here (CR 118.5a: a real,
    // unreduced {1} generic component needs a unit spent on it).
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the (reduced) cast cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("cost modifier reduced {1} to {0} - empty plan pays it");
    drain(&mut engine);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);

    assert!(engine.state().stack.is_empty());
    // Resolution mints a *new* ObjectId (CR 400.7) - check by card type/zone
    // rather than the stale pre-cast id.
    let creatures_on_battlefield = engine
        .state()
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && o.definition.card_types.contains(CardTypeSet::CREATURE))
        .count();
    assert_eq!(creatures_on_battlefield, 1, "bear resolved and entered the battlefield");
}
