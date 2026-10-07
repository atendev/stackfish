//! A scenario file's `keywords = [...]` list accepts `Hexproof` and `Shroud`
//! (CR 702.11, 702.18), and an unknown name still fails loudly.

use stackfish_scenario::{parse, run, AssertionOutcome, EntryOutcome, StepOutcome};

fn scenario_with_keywords(keywords: &str) -> String {
    format!(
        r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "bear"
        name = "Test Bear"
        generic_cost = 1
        colors = ["G"]
        types = ["Creature"]
        power = 2
        toughness = 2
        keywords = {keywords}

        [[steps]]
        type = "assert_object_zone"
        card = "bear"
        equals = "battlefield"
        "#
    )
}

#[test]
fn hexproof_and_shroud_are_accepted_scenario_keywords() {
    let report = run(&parse(&scenario_with_keywords(r#"["Hexproof", "Shroud"]"#)));
    for entry in &report.entries {
        match &entry.outcome {
            EntryOutcome::Assertion(AssertionOutcome::Failed { expected, actual }) => {
                panic!("assertion failed: {} (expected {expected}, got {actual})", entry.description)
            }
            EntryOutcome::Action(StepOutcome::Errored(msg)) => panic!("action errored: {} ({msg})", entry.description),
            _ => {}
        }
    }
}

#[test]
#[should_panic(expected = "unknown keyword in scenario file: Ward")]
fn ward_is_still_an_unknown_scenario_keyword() {
    let _ = run(&parse(&scenario_with_keywords(r#"["Ward"]"#)));
}
