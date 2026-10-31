//! `template = "..."` on a `[[cards]]` entry works end to end through the engine. These tests
//! use `parse`, which expands the template, and not `toml::from_str`.

use stackfish_scenario::{AssertionOutcome, EntryOutcome, parse, run};

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
                panic!(
                    "assertion failed: {} (expected {expected}, got {actual})",
                    entry.description
                )
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
    // The green template with `mana_ability` set to `U`; the pool color shows which value won.
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
