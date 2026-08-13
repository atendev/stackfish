//! Phase 5 vertical slice: backlog item 13, mana system completeness beyond
//! the flat `AbilityEffect::AddMana(Vec<ManaUnit>)` shape.
//!
//! Two real gaps closed: (1) `Game.Ops.AddMana`/`Game.Ops.AddManaChoice`
//! didn't exist at all - no card script could define a mana ability, only
//! Rust test fixtures could (checked before writing any code - confirmed by
//! grepping `script.rs` for `AddMana`, zero hits). (2) no player-chosen-color
//! mechanism existed for "add one mana of any color" text - new
//! `AbilityEffect::AddManaChoice { count, choices }`, with the actual choice
//! carried on `PlayerActionKind::ActivateAbility::chosen_colors`, validated
//! and applied by `commands.rs`'s `ActivateAbility` arm. "More than one fixed
//! mana" (the backlog's other named case) turned out to already work - `Vec`
//! already supports it - so this slice adds a test proving that instead of
//! reimplementing anything.

use std::collections::VecDeque;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, single_player_state};

fn land_definition() -> CardDefinition {
    minimal_card_definition(1, "Test Land", "test_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
}

fn permanent_object(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

/// One player, main phase, empty stack, a battlefield zone with a single
/// permanent carrying whichever `effect` the caller supplies as its only
/// registered ability.
fn state_with_ability(source: ObjectId, controller: PlayerId, effect: AbilityEffect) -> GameState {
    let mut objects = std::collections::HashMap::new();
    objects.insert(source, permanent_object(source, land_definition(), controller));
    GameState {
        objects,
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(effect),
            intervening_if: None,
        }],
        ..single_player_state(controller, vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::from([source]) }])
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, chosen_colors: Vec<Color>) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors,
                x_value: None,
            },
        })
        .expect("player holds priority");
    drain(engine)
}

// ---------------------------------------------------------------------
// "More than one fixed mana" - already worked mechanically, now proven.
// ---------------------------------------------------------------------

#[test]
fn fixed_mana_ability_can_add_more_than_one_unit() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_ability(
        source,
        p0,
        AbilityEffect::AddMana(vec![ManaUnit::Colorless, ManaUnit::Colorless]),
    );
    let mut engine = Engine::new(state);

    activate(&mut engine, p0, source, Vec::new());
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 2);
}

// ---------------------------------------------------------------------
// AddManaChoice: the real new mechanism.
// ---------------------------------------------------------------------

#[test]
fn add_mana_choice_adds_the_chosen_colors() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state =
        state_with_ability(source, p0, AbilityEffect::AddManaChoice { count: 2, choices: ColorSet::all() });
    let mut engine = Engine::new(state);

    let status = activate(&mut engine, p0, source, vec![Color::Blue, Color::Green]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 1);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 1);
    assert!(engine.state().object(source).unwrap().tapped);
}

#[test]
fn add_mana_choice_rejects_wrong_choice_count() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state =
        state_with_ability(source, p0, AbilityEffect::AddManaChoice { count: 2, choices: ColorSet::all() });
    let mut engine = Engine::new(state);

    let status = activate(&mut engine, p0, source, vec![Color::Blue]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 0);
}

#[test]
fn add_mana_choice_rejects_a_color_outside_the_allowed_set() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_ability(
        source,
        p0,
        AbilityEffect::AddManaChoice { count: 1, choices: ColorSet::WHITE | ColorSet::BLUE },
    );
    let mut engine = Engine::new(state);

    let status = activate(&mut engine, p0, source, vec![Color::Black]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.black, 0);
}

#[test]
fn chosen_colors_rejected_against_a_plain_fixed_mana_ability() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_ability(source, p0, AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)]));
    let mut engine = Engine::new(state);

    let status = activate(&mut engine, p0, source, vec![Color::Blue]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0);
}

// ---------------------------------------------------------------------
// Game.Ops.AddMana / Game.Ops.AddManaChoice - the Lua-facing bindings.
// ---------------------------------------------------------------------

#[test]
fn game_ops_add_mana_decodes_a_fixed_unit_with_default_count() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _) = runtime.load_operation("return Game.Ops.AddMana(\"G\")").unwrap();
    assert!(matches!(effect, Some(AbilityEffect::AddMana(units)) if units == vec![ManaUnit::Colored(Color::Green)]));
}

#[test]
fn game_ops_add_mana_decodes_a_repeated_colorless_unit() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _) = runtime.load_operation("return Game.Ops.AddMana(\"C\", 2)").unwrap();
    assert!(matches!(
        effect,
        Some(AbilityEffect::AddMana(units)) if units == vec![ManaUnit::Colorless, ManaUnit::Colorless]
    ));
}

#[test]
fn game_ops_add_mana_rejects_an_unknown_letter() {
    let mut runtime = ScriptRuntime::new();
    assert!(runtime.load_operation("return Game.Ops.AddMana(\"X\")").is_err());
}

#[test]
fn game_ops_add_mana_choice_decodes_count_and_colors() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _) = runtime.load_operation("return Game.Ops.AddManaChoice(1, \"WUBRG\")").unwrap();
    assert!(matches!(
        effect,
        Some(AbilityEffect::AddManaChoice { count: 1, choices }) if choices == ColorSet::all()
    ));
}

// ---------------------------------------------------------------------
// Game.Ops.AddRestrictedMana - CR 106.11, "spend this mana only to cast
// [card type] spells" (phase5/restricted-mana).
// ---------------------------------------------------------------------

#[test]
fn game_ops_add_restricted_mana_decodes_a_fixed_colored_unit() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _) = runtime.load_operation("return Game.Ops.AddRestrictedMana(\"G\", \"Artifact\")").unwrap();
    assert!(matches!(
        effect,
        Some(AbilityEffect::AddMana(units))
            if units == vec![ManaUnit::RestrictedColored(Color::Green, CardTypeSet::ARTIFACT)]
    ));
}

#[test]
fn game_ops_add_restricted_mana_decodes_a_repeated_colorless_unit() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _) = runtime.load_operation("return Game.Ops.AddRestrictedMana(\"C\", \"Artifact\", 3)").unwrap();
    assert!(matches!(
        effect,
        Some(AbilityEffect::AddMana(units))
            if units == vec![ManaUnit::RestrictedColorless(CardTypeSet::ARTIFACT); 3]
    ));
}

#[test]
fn game_ops_add_restricted_mana_rejects_an_unknown_card_type() {
    let mut runtime = ScriptRuntime::new();
    assert!(runtime.load_operation("return Game.Ops.AddRestrictedMana(\"C\", \"NotAType\")").is_err());
}

#[test]
fn game_ops_add_restricted_mana_rejects_an_unknown_letter() {
    let mut runtime = ScriptRuntime::new();
    assert!(runtime.load_operation("return Game.Ops.AddRestrictedMana(\"X\", \"Artifact\")").is_err());
}
