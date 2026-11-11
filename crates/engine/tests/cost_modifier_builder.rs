//! CR 601.2f: `Game.CreateCostModifier(c)` in a card's `register_abilities` registers a `CostModifier` onto `GameState.cost_modifiers`, through the same queue-and-drain builder as static abilities.
//! `controller` cannot be set from Lua, so `:Register()` queues a placeholder and the drain step in `run_card_script` resolves the real value.
//! Closes the CostModifier TODO for a Lua construction path.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_main_phase_state};

const CREATURES_COST_ONE_LESS: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateCostModifier(c)
    m:SetFilter(Game.Filter({ card_types = { "Creature" } }))
    m:SetAmount(-1)
    m:Register()
end
return s
"#;

// CR 118.7b/c
// `:SetAmount(amount, color)`'s second argument targets a colored pip and not the generic component.
const CREATURES_COST_W_LESS: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateCostModifier(c)
    m:SetFilter(Game.Filter({ card_types = { "Creature" } }))
    m:SetAmount(-1, "W")
    m:Register()
end
return s
"#;

const HAND_CREATURES_COST_ONE_LESS: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateCostModifier(c)
    m:SetFilter(Game.Filter({ card_types = { "Creature" } }))
    m:SetAmount(-1)
    m:SetFunctionsFrom("Hand")
    m:Register()
end
return s
"#;

const HAND_AND_BATTLEFIELD_CREATURES_COST_ONE_LESS: &str = r#"
local s = {}
function s.register_abilities(c)
    local from_hand = Game.CreateCostModifier(c)
    from_hand:SetFilter(Game.Filter({ card_types = { "Creature" } }))
    from_hand:SetAmount(-1)
    from_hand:SetFunctionsFrom("Hand")
    from_hand:Register()
    local in_play = Game.CreateCostModifier(c)
    in_play:SetFilter(Game.Filter({ card_types = { "Creature" } }))
    in_play:SetAmount(-1)
    in_play:Register()
end
return s
"#;

const NO_SET_FILTER: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateCostModifier(c)
    m:SetAmount(-1)
    m:Register()
end
return s
"#;

const NO_SET_AMOUNT: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateCostModifier(c)
    m:SetFilter(Game.Filter({ card_types = { "Creature" } }))
    m:Register()
end
return s
"#;

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        oracle_id: None,
        name: "Test Cost-Reducing Land".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn bear_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        oracle_id: None,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))],
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn white_bear_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 3,
        oracle_id: None,
        name: "Test White Bear".to_string(),
        mana_cost: ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White))],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("white_bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn hand_reducer_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 4,
        name: "Test Hand Reducer".to_string(),
        oracle_text: "Creature spells you cast cost {1} less while this is in your hand."
            .to_string(),
        script_ref: ScriptReference::Inline(HAND_CREATURES_COST_ONE_LESS.to_string()),
        ..bear_definition()
    }
}

fn put_in_hand(state: &mut GameState, id: ObjectId, definition: CardDefinition, owner: PlayerId) {
    state
        .objects
        .insert(id, card_object(id, definition, owner, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(owner))
        .unwrap()
        .push_bottom(id);
}

// Stands in for the engine's off-battlefield script sweep so the hand-time
// modifier exists before the first action.
fn run_hand_script(state: &mut GameState, id: ObjectId) {
    let definition = state.object(id).unwrap().definition.clone();
    ScriptRuntime::new()
        .run_card_script(state, &definition, id)
        .expect("hand script registers cleanly");
    state.off_battlefield_scripts_run.insert(id);
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) -> StepStatus {
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
        .expect("caster holds priority, card is in hand");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
}

#[test]
fn run_card_script_self_registers_a_cost_modifier() {
    let mut state = two_player_main_phase_state();
    let land = ObjectId(0);
    let definition = land_definition(CREATURES_COST_ONE_LESS);
    state.objects.insert(
        land,
        card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield),
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, land)
        .expect("script self-registers cleanly");

    assert_eq!(state.cost_modifiers.len(), 1);
    let modifier = &state.cost_modifiers[0];
    assert_eq!(modifier.source, land);
    assert_eq!(
        modifier.controller,
        PlayerId(0),
        "resolved from the source's own live controller at drain time"
    );
    assert_eq!(modifier.amount, -1);
    assert!(matches!(modifier.filter.card_types, Some(types) if types == CardTypeSet::CREATURE));
}

#[test]
fn run_card_script_self_registers_a_colored_cost_modifier() {
    let mut state = two_player_main_phase_state();
    let land = ObjectId(0);
    let definition = land_definition(CREATURES_COST_W_LESS);
    state.objects.insert(
        land,
        card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield),
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, land)
        .expect("script self-registers cleanly");

    assert_eq!(state.cost_modifiers.len(), 1);
    let modifier = &state.cost_modifiers[0];
    assert_eq!(modifier.amount, -1);
    assert_eq!(modifier.target, CostModifierTarget::Colored(Color::White));
}

#[test]
fn register_without_set_filter_is_an_error() {
    let mut state = two_player_main_phase_state();
    let land = ObjectId(0);
    let definition = land_definition(NO_SET_FILTER);
    state.objects.insert(
        land,
        card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield),
    );

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, land);

    assert!(result.is_err(), "Register() with no SetFilter() must fail");
}

#[test]
fn register_without_set_amount_is_an_error() {
    let mut state = two_player_main_phase_state();
    let land = ObjectId(0);
    let definition = land_definition(NO_SET_AMOUNT);
    state.objects.insert(
        land,
        card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield),
    );

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, land);

    assert!(result.is_err(), "Register() with no SetAmount() must fail");
}

#[test]
fn play_land_self_registered_cost_modifier_reduces_a_matching_spells_cost_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_main_phase_state();
    state.objects.insert(
        land,
        card_object(
            land,
            land_definition(CREATURES_COST_ONE_LESS),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(land);
    state.objects.insert(
        bear,
        card_object(bear, bear_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .expect("p0 holds priority, land is in hand");
    drain(&mut engine);
    assert_eq!(
        engine.state().cost_modifiers.len(),
        1,
        "the land's self-registered cost modifier is live"
    );

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: bear,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority, bear is in hand");
    let status = drain(&mut engine);

    // Bear's printed cost is {1}, so an empty payment plan is rejected unless the modifier applied (CR 118.5a).
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the (reduced) cast cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("cost modifier reduced {1} to {0} - empty plan pays it");
    drain(&mut engine);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);

    assert!(engine.state().stack.is_empty());
    // Resolution mints a new ObjectId (CR 400.7), so check by card type and zone and not the pre-cast id.
    let creatures_on_battlefield = engine
        .state()
        .objects
        .values()
        .filter(|o| {
            o.zone == ZoneKind::Battlefield
                && o.definition.card_types.contains(CardTypeSet::CREATURE)
        })
        .count();
    assert_eq!(
        creatures_on_battlefield, 1,
        "bear resolved and entered the battlefield"
    );
}

#[test]
fn play_land_self_registered_colored_cost_modifier_reduces_a_matching_spells_cost_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_main_phase_state();
    state.objects.insert(
        land,
        card_object(
            land,
            land_definition(CREATURES_COST_W_LESS),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(land);
    state.objects.insert(
        bear,
        card_object(bear, white_bear_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .expect("p0 holds priority, land is in hand");
    drain(&mut engine);
    assert_eq!(
        engine.state().cost_modifiers.len(),
        1,
        "the land's self-registered cost modifier is live"
    );

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: bear,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority, bear is in hand");
    let status = drain(&mut engine);

    // The white bear's cost is {W} (CR 118.7): an empty plan pays it, so the {W} pip itself disappeared.
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the (reduced) cast cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("colored cost modifier reduced {W} to {0} - empty plan pays it");
    drain(&mut engine);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);

    assert!(engine.state().stack.is_empty());
    let creatures_on_battlefield = engine
        .state()
        .objects
        .values()
        .filter(|o| {
            o.zone == ZoneKind::Battlefield
                && o.definition.card_types.contains(CardTypeSet::CREATURE)
        })
        .count();
    assert_eq!(
        creatures_on_battlefield, 1,
        "the white bear resolved and entered the battlefield"
    );
}

// CR 611.3b, 400.7
// The land's battlefield modifier registers under the new id after the move,
// so only the hand-time one may be purged.
#[test]
fn play_land_drops_the_cost_modifier_registered_from_hand() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = two_player_main_phase_state();
    put_in_hand(
        &mut state,
        land,
        land_definition(HAND_AND_BATTLEFIELD_CREATURES_COST_ONE_LESS),
        p0,
    );
    run_hand_script(&mut state, land);
    let sources: Vec<ObjectId> = state.cost_modifiers.iter().map(|m| m.source).collect();
    assert_eq!(
        sources,
        vec![land],
        "only the hand-time modifier registers in hand"
    );

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .expect("p0 holds priority, land is in hand");
    drain(&mut engine);

    let played = engine
        .state()
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Battlefield)
        .expect("the land is on the battlefield")
        .object_id;
    let sources: Vec<ObjectId> = engine
        .state()
        .cost_modifiers
        .iter()
        .map(|m| m.source)
        .collect();
    assert_eq!(sources, vec![played]);
}

// CR 611.3b, 400.7, 601.2f
#[test]
fn casting_a_hand_cost_reducer_ends_its_reduction() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reducer = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_main_phase_state();
    put_in_hand(&mut state, reducer, hand_reducer_definition(), p0);
    put_in_hand(&mut state, bear, bear_definition(), p0);
    run_hand_script(&mut state, reducer);
    assert_eq!(state.cost_modifiers.len(), 1);

    let mut engine = Engine::new(state);
    let status = cast(&mut engine, p0, reducer);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the reducer's cast cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("the cost is locked in before the card leaves hand, so its own reduction applies");
    drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    assert!(engine.state().stack.is_empty());
    assert!(
        engine.state().cost_modifiers.is_empty(),
        "the hand-time modifier left with its card, got {:?}",
        engine.state().cost_modifiers
    );

    let status = cast(&mut engine, p0, bear);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the bear's cast cost, got {status:?}"
    );
    assert!(
        engine
            .submit_payment(PaymentPlan {
                payments: Vec::new()
            })
            .is_err(),
        "the bear costs its full {{1}} once the reducer has left hand"
    );
}
