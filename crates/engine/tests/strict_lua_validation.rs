//! Strict key, type and counter-name validation in Lua decoders. Unknown table
//! keys, wrong-typed values and unrecognized counter strings must raise a load
//! or runtime error instead of being silently ignored.

use std::collections::BTreeSet;

use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::{ObjectId, PlayerId};
use stackfish_testutil::two_player_state;

fn assert_load_error(script: &str, expected_substrings: &[&str]) {
    let mut runtime = ScriptRuntime::new();
    let err = runtime
        .load_operation(script)
        .expect_err("script should fail to load due to strict validation");
    let msg = format!("{err:?}");
    for sub in expected_substrings {
        assert!(
            msg.contains(sub),
            "expected error to contain '{sub}'\nfull error: {msg}"
        );
    }
}

#[test]
fn game_filter_rejects_unknown_object_filter_key() {
    assert_load_error(
        "return Game.Filter({ foo = true })",
        &["Game.Filter", "unknown key", "foo"],
    );
}

#[test]
fn game_filter_rejects_misplaced_affected_flag_key() {
    assert_load_error(
        "return Game.Filter({ self_only = true })",
        &["Game.Filter has no `self_only` key", "Game.Affected.Self"],
    );
}

#[test]
fn game_ops_search_rejects_unknown_option_key() {
    assert_load_error(
        "return Game.Ops.Search({}, 1, { tappedd = true })",
        &["Game.Ops.Search opts", "unknown key", "tappedd"],
    );
}

#[test]
fn game_ops_search_rejects_unknown_destination() {
    assert_load_error(
        "return Game.Ops.Search({}, 1, { destination = \"library\" })",
        &[
            "Game.Ops.Search destination",
            "library",
            "Hand",
            "Battlefield",
        ],
    );
}

#[test]
fn game_ops_search_rejects_lowercase_destination() {
    assert_load_error(
        "return Game.Ops.Search({}, 1, { destination = \"battlefield\" })",
        &[
            "Game.Ops.Search destination",
            "battlefield",
            "Hand",
            "Battlefield",
        ],
    );
}

#[test]
fn return_to_battlefield_rejects_capitalized_control() {
    assert_load_error(
        "return Game.Ops.ReturnToBattlefield({ control = \"Owner\" })",
        &["ReturnToBattlefield control", "Owner", "you", "owner"],
    );
}

#[test]
fn enters_as_copy_rejects_old_counter_keys() {
    assert_load_error(
        "return Game.ReplacementPayloads.EntersAsCopy({ counter_kind = \"PlusOnePlusOne\", counter_amount = 1 })",
        &["counter_", "unknown key"],
    );
}

#[test]
fn game_ops_cast_spell_rejects_unknown_option_key() {
    assert_load_error(
        "return Game.Ops.CastSpell(0, { fre = true })",
        &["Game.Ops.CastSpell opts", "unknown key", "fre"],
    );
}

#[test]
fn game_choose_n_rejects_unknown_option_key() {
    assert_load_error(
        "return Game.ChooseN({}, 1, { upto = true })",
        &["Game.ChooseN opts", "unknown key", "upto"],
    );
}

#[test]
fn game_counter_exposes_common_kinds_and_custom_constructor() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation("return Game.Ops.Counters(Game.Counter.PlusOnePlusOne, 1)")
        .expect("known counter tag should decode");
    assert!(
        matches!(effect, Some(stackfish_engine::types::ability::AbilityEffect::Counters { kind, amount: 1 }) if kind == stackfish_engine::types::counter::CounterKind::PlusOnePlusOne)
    );
}

#[test]
fn unknown_counter_string_is_rejected_at_load_time() {
    assert_load_error(
        "return Game.Ops.Counters(\"Skull\", 3)",
        &["unknown counter kind", "Skull", "Game.Counter.Custom"],
    );
}

#[test]
fn game_counter_custom_tag_round_trips() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation("return Game.Ops.Counters(Game.Counter.Custom(\"Charge\"), 2)")
        .expect("custom counter tag should decode");
    assert!(
        matches!(effect, Some(stackfish_engine::types::ability::AbilityEffect::Counters { kind: stackfish_engine::types::counter::CounterKind::Custom(ref name), amount: 2 }) if name == "Charge")
    );
}

#[test]
fn token_creature_opts_reject_unknown_key() {
    assert_load_error(
        "return Game.Tokens.Creature(\"Bear\", \"\", 2, 2, { flyng = { \"Flying\" } })",
        &["Game.Tokens.Creature opts", "unknown key", "flyng"],
    );
}

#[test]
fn landwalk_quality_rejects_unknown_key() {
    assert_load_error(
        "return Game.Effects.AddLandwalkQuality({ sbtypes = { \"Forest\" } })",
        &[
            "Game.Effects.AddLandwalkQuality opts",
            "unknown key",
            "sbtypes",
        ],
    );
}

#[test]
fn become_copy_rejects_unknown_exception_key() {
    assert_load_error(
        "return Game.Ops.BecomeCopy({ powerr = 2 })",
        &[
            "Game.Ops.BecomeCopy/Game.Tokens.CopyOf opts",
            "unknown key",
            "powerr",
        ],
    );
}

#[test]
fn filter_table_rejects_unknown_flat_key() {
    assert_load_error(
        "return Game.Filter({ card_type = { \"Creature\" } })",
        &["unknown key", "card_type"],
    );
}

const FRESH_RUNTIMES: usize = 32;

// Lua seeds string hashes per state, so each fresh runtime can walk the table in a different order.
fn one_message_across_runtimes(run: impl Fn() -> String) -> String {
    let messages: BTreeSet<String> = (0..FRESH_RUNTIMES).map(|_| run()).collect();
    assert_eq!(messages.len(), 1, "messages differ: {messages:#?}");
    messages.into_iter().next().unwrap()
}

fn load_error_message(script: &str) -> String {
    let err = ScriptRuntime::new()
        .load_operation(script)
        .expect_err("script should fail to load due to strict validation");
    format!("{err:?}")
}

fn operation_error_message(body: &str) -> String {
    let runtime = ScriptRuntime::new();
    let source = format!(
        "local s = {{}}\nfunction s.operation()\n  return function(source, controller)\n{body}\n  end\nend\nreturn s"
    );
    let (_, handle) = runtime
        .build_operation(&source)
        .expect("script should load")
        .expect("declares an operation");
    let err = runtime
        .invoke_operation(
            handle,
            &mut two_player_state(Vec::new()),
            ObjectId(0),
            PlayerId(0),
        )
        .expect_err("the operation should fail on its bad keys");
    format!("{err:?}")
}

fn assert_contains(message: &str, expected: &str) {
    assert!(
        message.contains(expected),
        "expected error to contain {expected:?}\nfull error: {message}"
    );
}

#[test]
fn unknown_keys_are_named_in_sorted_order_at_load_time() {
    let message = one_message_across_runtimes(|| {
        load_error_message("return Game.Filter({ zeta = true, alpha = true, mid = true })")
    });
    assert_contains(&message, "has unknown keys 'alpha', 'mid', 'zeta'");
}

#[test]
fn entry_options_name_bad_keys_in_sorted_order_at_load_time() {
    let message = one_message_across_runtimes(|| {
        load_error_message("return Game.Ops.ReturnToBattlefield({ zeta = 1, alpha = 1 })")
    });
    assert_contains(
        &message,
        r#"ReturnToBattlefield options 'alpha', 'zeta' are unknown or have the wrong type (expected control = \"you\"|\"owner\", tapped = boolean)"#,
    );
}

#[test]
fn entry_options_name_one_bad_key() {
    let message = load_error_message("return Game.Ops.ReturnToBattlefield({ zeta = 1 })");
    assert_contains(
        &message,
        r#"ReturnToBattlefield option 'zeta' is unknown or has the wrong type (expected control = \"you\"|\"owner\", tapped = boolean)"#,
    );
}

#[test]
fn entry_options_report_bad_keys_before_a_bad_control_value() {
    let message = one_message_across_runtimes(|| {
        load_error_message("return Game.Ops.ReturnToBattlefield({ control = \"Owner\", zeta = 1 })")
    });
    assert_contains(&message, "option 'zeta' is unknown");
    assert!(!message.contains("Owner"), "full error: {message}");

    let message =
        load_error_message("return Game.Ops.ReturnToBattlefield({ control = 5, tapped = true })");
    assert_contains(
        &message,
        "option 'control' is unknown or has the wrong type",
    );
}

#[test]
fn choose_n_names_unknown_opts_keys_in_sorted_order_at_game_time() {
    let message = one_message_across_runtimes(|| {
        operation_error_message("return Game.ChooseN({}, 1, { zeta = true, alpha = true })")
    });
    assert_contains(
        &message,
        "Game.ChooseN opts has unknown keys 'alpha', 'zeta'",
    );
}

#[test]
fn entry_options_name_bad_keys_in_sorted_order_at_game_time() {
    let message = one_message_across_runtimes(|| {
        operation_error_message("return Game.Ops.ReturnToBattlefield({ zeta = 1, alpha = 1 })")
    });
    assert_contains(
        &message,
        r#"ReturnToBattlefield options 'alpha', 'zeta' are unknown or have the wrong type (expected control = \"you\"|\"owner\", tapped = boolean)"#,
    );
}

// Booleans and floats print no stable text, so they are named by type.
#[test]
fn non_string_keys_are_named_by_integer_text_or_type() {
    let message = one_message_across_runtimes(|| {
        load_error_message("return Game.Filter({ [true] = 1, [2] = 1, [2.5] = 1, zeta = 1 })")
    });
    assert_contains(
        &message,
        "has unknown keys '2', '<boolean>', '<number>', 'zeta'",
    );

    let message = one_message_across_runtimes(|| {
        load_error_message(
            "return Game.ChooseN({}, 1, { [true] = 1, [2] = 1, [2.5] = 1, zeta = true })",
        )
    });
    assert_contains(
        &message,
        "Game.ChooseN opts has unknown keys '2', '<boolean>', '<number>', 'zeta'",
    );
}

// `!` sorts below the quote mark, so names must be sorted before they are quoted to match on both sides.
#[test]
fn rust_and_lua_key_checks_sort_names_by_bytes() {
    let message = one_message_across_runtimes(|| {
        load_error_message("return Game.Filter({ [\"a!\"] = 1, a = 1 })")
    });
    assert_contains(&message, "has unknown keys 'a', 'a!'");

    let message = one_message_across_runtimes(|| {
        load_error_message("return Game.ChooseN({}, 1, { [\"a!\"] = true, a = true })")
    });
    assert_contains(&message, "Game.ChooseN opts has unknown keys 'a', 'a!'");
}
