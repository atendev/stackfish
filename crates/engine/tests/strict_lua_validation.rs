//! Strict key, type and counter-name validation in Lua decoders. Unknown table
//! keys, wrong-typed values and unrecognized counter strings must raise a load
//! or runtime error instead of being silently ignored.

use stackfish_engine::script::ScriptRuntime;

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
        &["Game.Ops.Search destination", "library", "hand/battlefield"],
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
