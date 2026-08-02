//! Phase 5 vertical slice: `Game.CreateManaAbility` (CR 605) - a card's own
//! Lua `register_abilities(c)` can now self-register a real mana ability, the
//! dedicated sibling `Game.CreateActivatedAbility` was never safe to reuse
//! for this (see this builder's own doc comment in script.rs for why: no
//! `:SetTarget` at all, and `:SetOperation` only accepts `Game.Ops.AddMana`/
//! `AddManaChoice`).
//!
//! Also closes the real gap that motivated this slice: a mana ability with
//! no `Game.Costs.*` component at all (`Game.CreateManaAbility(c):
//! SetOperation(Game.Ops.AddMana("C")):Register()`, Wastes' own shape)
//! reproduces `mana_ability` TOML's original hardcoded-tap, no-payment-pause
//! behavior unchanged - but a mana ability with a *real* cost (`:AddCost`,
//! e.g. a bare `Game.Costs.TapSelf`, or a Treasure/Lotus-Petal-style
//! `Sacrifice`) now pauses for a real `Engine::submit_payment` step first,
//! same as any other ability's cost, while still never reaching the stack
//! (CR 605.1a/605.3b) - see `commands::PaymentResolution::ManaAbility` and
//! `Engine::continue_cost_zone_moves`'s own tail.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, single_player_state, two_player_state as base_two_player_state};

const NO_COST_ADD_MANA: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:SetOperation(Game.Ops.AddMana("C"))
    e:Register()
end
return s
"#;

const TAP_SELF_COST_ADD_MANA: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.AddMana("C"))
    e:Register()
end
return s
"#;

const SACRIFICE_COST_ADD_MANA_CHOICE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:AddCost(Game.Costs.Sacrifice({ card_types = { "Artifact" } }))
    e:SetOperation(Game.Ops.AddManaChoice(1, "WUBRG"))
    e:Register()
end
return s
"#;

const NO_SET_OPERATION: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:Register()
end
return s
"#;

const NON_MANA_OPERATION: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const ADD_MANA_CHOICE_NO_COST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:SetOperation(Game.Ops.AddManaChoice(1, "WU"))
    e:Register()
end
return s
"#;

const SET_TARGET_DOES_NOT_EXIST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:SetTarget({ type = "object" })
    e:SetOperation(Game.Ops.AddMana("C"))
    e:Register()
end
return s
"#;

fn artifact_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Mana Rock".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: "Mana ability self-registration test fixture.".to_string(),
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

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Mana Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "{T}: Add {C}.".to_string(),
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

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::run_card_script's mana-ability registration
// mechanics, without driving the whole Engine pipeline.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_a_mana_ability_with_no_cost() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let rock = ObjectId(0);
    let definition = artifact_definition(NO_COST_ADD_MANA);
    state.objects.insert(rock, card_object(rock, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, rock).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert_eq!(ability.source, rock);
    assert_eq!(ability.kind, AbilityKind::Activated);
    assert!(ability.target.is_none());
    assert!(ability.cost.is_none(), "no AddCost() calls means cost: None, matching mana_ability TOML");
    assert!(matches!(ability.effect, Some(AbilityEffect::AddMana(ref units)) if units == &vec![ManaUnit::Colorless]));
}

#[test]
fn run_card_script_self_registers_a_mana_ability_with_a_real_cost() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let rock = ObjectId(0);
    let definition = artifact_definition(TAP_SELF_COST_ADD_MANA);
    state.objects.insert(rock, card_object(rock, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, rock).expect("script self-registers cleanly");

    let cost = state.abilities[0].cost.as_ref().expect("AddCost(TapSelf) was called - cost must be Some");
    assert!(matches!(cost.additional.as_slice(), [AdditionalCost::TapSelf]));
    assert!(matches!(state.abilities[0].effect, Some(AbilityEffect::AddMana(_))));
}

#[test]
fn run_card_script_self_registers_a_mana_ability_with_add_mana_choice() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let rock = ObjectId(0);
    let definition = artifact_definition(ADD_MANA_CHOICE_NO_COST);
    state.objects.insert(rock, card_object(rock, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, rock).expect("script self-registers cleanly");

    assert!(state.abilities[0].cost.is_none());
    match &state.abilities[0].effect {
        Some(AbilityEffect::AddManaChoice { count, choices }) => {
            assert_eq!(*count, 1);
            assert!(choices.contains_color(Color::White));
            assert!(choices.contains_color(Color::Blue));
            assert!(!choices.contains_color(Color::Red));
        }
        other => panic!("expected AddManaChoice, got {other:?}"),
    }
}

#[test]
fn run_card_script_mana_ability_register_without_set_operation_is_an_error() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let rock = ObjectId(0);
    let definition = artifact_definition(NO_SET_OPERATION);
    state.objects.insert(rock, card_object(rock, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, rock);

    assert!(result.is_err(), "Register() with no SetOperation() must fail");
    assert!(state.abilities.is_empty());
}

#[test]
fn run_card_script_mana_ability_rejects_a_non_mana_operation() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let rock = ObjectId(0);
    let definition = artifact_definition(NON_MANA_OPERATION);
    state.objects.insert(rock, card_object(rock, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, rock);

    assert!(result.is_err(), "Game.Ops.GainLife(1) isn't AddMana/AddManaChoice - must be rejected");
    assert!(state.abilities.is_empty(), "no partial ability should have been committed");
}

#[test]
fn run_card_script_mana_ability_has_no_set_target_method() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let rock = ObjectId(0);
    let definition = artifact_definition(SET_TARGET_DOES_NOT_EXIST);
    state.objects.insert(rock, card_object(rock, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, rock);

    assert!(result.is_err(), "ManaAbilityBuilder has no :SetTarget method at all - calling it is a Lua error");
    assert!(state.abilities.is_empty());
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline.
// ---------------------------------------------------------------------

fn no_stack_object_pushed(engine: &Engine) -> bool {
    !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::StackObjectPushed { .. }))
}

/// The Wastes shape: no `:AddCost` at all reproduces the original
/// `mana_ability` TOML behavior exactly - immediate, hardcoded tap, no
/// `Engine::submit_payment` step, never touches the stack.
#[test]
fn play_land_self_registered_mana_ability_with_no_cost_resolves_instantly() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(p0, p1));
    state.objects.insert(land, card_object(land, land_definition(NO_COST_ADD_MANA), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().abilities.len(), 1, "the land's mana ability self-registered");

    let new_land_id = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.front().unwrap();

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: new_land_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("p0 controls the land and holds priority");
    let status = drain(&mut engine);
    assert!(
        // CR 605.3a: activating a mana ability doesn't cause a player to
        // lose priority - p0 still holds it, same as before activating.
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no-cost mana ability resolves instantly and p0 keeps priority - got {status:?}"
    );

    assert!(engine.state().object(new_land_id).unwrap().tapped);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 1);
    assert!(engine.state().stack.is_empty());
    assert!(no_stack_object_pushed(&engine), "a mana ability must never push a StackObject");
}

/// A mana ability with an explicit `Game.Costs.TapSelf` cost now pauses for
/// a real payment - still never touching the stack.
#[test]
fn activated_mana_ability_with_explicit_tap_cost_pauses_for_payment() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let rock = ObjectId(0);
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(p0, p1));
    state.objects.insert(rock, card_object(rock, artifact_definition(TAP_SELF_COST_ADD_MANA), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(rock);
    let mut runtime = ScriptRuntime::new();
    let definition = artifact_definition(TAP_SELF_COST_ADD_MANA);
    runtime.run_card_script(&mut state, &definition, rock).expect("self-registers cleanly");

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: rock,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("p0 controls the rock and holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "a real cost must pause for payment, got {status:?}");
    assert!(!engine.state().object(rock).unwrap().tapped, "not tapped until payment commits");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 0);
    assert!(engine.state().stack.is_empty(), "still never pushed onto the stack, even while paused");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(rock)] })
        .expect("Tap(rock) pays the self-registered TapSelf cost");
    let status = drain(&mut engine);
    // CR 605.3a: still p0's priority, same as the no-cost case.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().object(rock).unwrap().tapped);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 1);
    assert!(engine.state().stack.is_empty());
    assert!(no_stack_object_pushed(&engine), "a cost-bearing mana ability must still never push a StackObject");
}

/// Treasure/Lotus-Petal shape: `Sacrifice this artifact: Add one mana of any
/// color.` Proves the existing Sacrifice-cost machinery
/// (`Engine::continue_cost_zone_moves`) is fully reusable for a mana
/// ability's own cost, and that the mana is added only once the sacrifice
/// itself has actually committed.
#[test]
fn activated_mana_ability_with_sacrifice_cost_resolves_without_ever_using_the_stack() {
    let p0 = PlayerId(0);
    let rock = ObjectId(0);
    let mut state = single_player_state(p0, hand_battlefield_graveyard_zones(p0, PlayerId(1)));
    state.objects.insert(rock, card_object(rock, artifact_definition(SACRIFICE_COST_ADD_MANA_CHOICE), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(rock);
    let mut runtime = ScriptRuntime::new();
    let definition = artifact_definition(SACRIFICE_COST_ADD_MANA_CHOICE);
    runtime.run_card_script(&mut state, &definition, rock).expect("self-registers cleanly");

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: rock,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: vec![Color::Blue],
            },
        })
        .expect("p0 controls the rock and holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the Sacrifice cost, got {status:?}");
    assert!(engine.state().object(rock).is_some(), "not sacrificed until payment commits");
    assert!(engine.state().stack.is_empty());

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(rock)] })
        .expect("sacrificing the rock itself pays its own Sacrifice{Artifact} cost");
    drain(&mut engine);

    assert!(engine.state().object(rock).is_none(), "the original id is retired");
    assert!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len() == 1,
        "the sacrificed rock now exists in the graveyard under a new id"
    );
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 1, "the chosen blue mana was added");
    assert!(engine.state().stack.is_empty(), "never touched the stack, even mid-sacrifice");
    assert!(no_stack_object_pushed(&engine), "a mana ability must never push a StackObject");
}
