//! Exercises `template = "..."` on a `[[cards]]` entry end to end through
//! the real engine — not just the TOML-merge unit tests in
//! `card_templates.rs` itself. `stackfish_scenario::parse` (not
//! `toml::from_str` directly) is what expands `template` before `CardSpec`
//! ever sees the entry, so these use `parse` the same way `load`/
//! `run_db_tests` do.

use stackfish_scenario::{parse, run, AssertionOutcome, EntryOutcome};

#[test]
fn template_land_taps_for_the_right_color() {
    let toml = r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[cards]]
        key = "forest"
        template = "test_land_green"

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "forest"

        [[steps]]
        type = "activate_ability"
        player = 0
        source = "forest"
        ability_index = 0
        targets = []

        [[steps]]
        type = "assert_tapped"
        card = "forest"
        equals = true

        [[steps]]
        type = "assert_mana_pool"
        player = 0
        color = "green"
        equals = 1
        "#;

    let file = parse(toml);
    let report = run(&file);

    for entry in &report.entries {
        match &entry.outcome {
            EntryOutcome::Assertion(AssertionOutcome::Failed { expected, actual }) => {
                panic!("assertion failed: {} (expected {expected}, got {actual})", entry.description)
            }
            EntryOutcome::Action(stackfish_scenario::StepOutcome::Errored(msg)) => {
                panic!("action errored: {} ({msg})", entry.description)
            }
            _ => {}
        }
    }
}

#[test]
fn entry_fields_override_the_template() {
    // Named after the green template but overrides `mana_ability` to `U` -
    // proves the entry's own field wins over the template's value for that
    // same key, observable via which color actually lands in the pool.
    let toml = r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[cards]]
        key = "weird_land"
        template = "test_land_green"
        mana_ability = ["U"]

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "weird_land"

        [[steps]]
        type = "activate_ability"
        player = 0
        source = "weird_land"
        ability_index = 0
        targets = []

        [[steps]]
        type = "assert_mana_pool"
        player = 0
        color = "blue"
        equals = 1
        "#;

    let file = parse(toml);
    let report = run(&file);
    assert!(!report.has_hard_failures());
}

#[test]
#[should_panic(expected = "unknown card template")]
fn unknown_template_name_panics_clearly() {
    let toml = r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[cards]]
        key = "x"
        template = "does_not_exist"
        "#;

    let _ = parse(toml);
}
