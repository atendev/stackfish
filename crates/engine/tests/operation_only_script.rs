//! A plain spell's script declares `operation()` and `target_requirement()` and
//! no `register_abilities` (docs/lua/card-script-basics.md). Running such a
//! script when the card changes zones must register nothing and must not fail.

use std::collections::HashMap;

use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state,
};

const OPERATION_ONLY: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DrawCards(1)
end
return s
"#;

const NOT_A_FUNCTION: &str = r#"
local s = {}
s.register_abilities = 5
return s
"#;

fn run(script: &str) -> (Result<(), stackfish_engine::script::ScriptError>, GameState) {
    let (p0, p1) = (PlayerId(0), PlayerId(1));
    let mut state = GameState {
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    };
    let definition = CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            1,
            "Test Spell",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::SORCERY,
        )
    };
    let id = ObjectId(0);
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(id);
    state
        .objects
        .insert(id, card_object(id, definition.clone(), p0, ZoneKind::Hand));
    let result = ScriptRuntime::new().run_card_script(&mut state, &definition, id);
    (result, state)
}

#[test]
fn a_script_without_register_abilities_registers_nothing_and_does_not_fail() {
    let (result, state) = run(OPERATION_ONLY);
    result.expect("an operation-only script loads cleanly");
    assert!(state.abilities.is_empty());
    assert!(state.continuous_effects.is_empty());
}

#[test]
fn a_register_abilities_that_is_not_a_function_is_still_a_load_error() {
    let (result, _) = run(NOT_A_FUNCTION);
    assert!(
        result.is_err(),
        "register_abilities = 5 is a script bug, not an absent function"
    );
}
