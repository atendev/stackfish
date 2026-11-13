//! A scripted `[[setup]]` card on the battlefield registers its abilities before the first step,
//! with no entry. See `docs/scenario_format.md`.

use stackfish_scenario::{build_initial_state, parse, run};

const HEADER: &str = r#"
seed = 1

[[players]]
id = 0
life = 20

[[cards]]
key = "filler"
name = "Filler Land"
types = ["Land"]

# Solo game, so the turn 1 draw step runs and needs a card.
[[setup]]
player = 0
zone = "library"
card = "filler"
"#;

fn scenario(body: &str) -> String {
    format!("{HEADER}\n{body}")
}

fn assert_clean(toml: &str) {
    let report = run(&parse(toml));
    assert!(
        !report.has_hard_failures(),
        "{:?}",
        report.hard_failure_lines()
    );
}

fn relic(script: &str, extra: &str) -> String {
    scenario(&format!(
        r#"
[[cards]]
key = "relic"
name = "Test Relic"
types = ["Artifact"]
{extra}
script = """
{script}
"""

[[setup]]
player = 0
zone = "battlefield"
card = "relic"
"#
    ))
}

const TOUGHNESS_ANTHEM: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(0, 1))
    e:SetAffected(Game.Filter({ controller = "you", card_types = { "Creature" } }))
    e:Register()
end
return s
"#;

const ETB_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local t = Game.CreateTriggeredAbility(c)
    t:SetCondition(Game.Conditions.Enters)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

const ACTIVATED_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

// Parses, so the `operation()` and `target_requirement()` loaders pass it; only the setup run rejects it.
const NOT_A_FUNCTION: &str = r#"
local s = {}
s.register_abilities = 5
return s
"#;

const RAISES: &str = r#"
local s = {}
function s.register_abilities(c)
    error("boom")
end
return s
"#;

// CR 704.5g
#[test]
fn setup_static_ability_applies_from_the_first_step() {
    let toml = relic(TOUGHNESS_ANTHEM, "")
        + r#"
[[cards]]
key = "wisp"
name = "Test Wisp"
types = ["Creature"]
power = 1
toughness = 1

# Lethal without the relic's +0/+1.
[[setup]]
player = 0
zone = "battlefield"
card = "wisp"
damage_marked = 1

[[steps]]
type = "pass_priority"
player = 0

[[steps]]
type = "assert_object_zone"
card = "wisp"
equals = "battlefield"
"#;
    assert_clean(&toml);
}

// CR 603.6a
#[test]
fn setup_enters_trigger_does_not_fire() {
    let toml = relic(ETB_GAIN_LIFE, "")
        + r#"
[[steps]]
type = "pass_priority"
player = 0

[[steps]]
type = "assert_life"
player = 0
equals = 20
"#;
    assert_clean(&toml);
}

// CR 602.2
#[test]
fn setup_activated_ability_can_be_activated() {
    let toml = relic(ACTIVATED_GAIN_LIFE, "")
        + r#"
[[steps]]
type = "activate_ability"
player = 0
source = "relic"
ability_index = 0

[[steps]]
type = "submit_payment"
player = 0

[[steps]]
type = "pass_priority"
player = 0

[[steps]]
type = "assert_life"
player = 0
equals = 21
"#;
    assert_clean(&toml);
}

#[test]
#[should_panic(expected = "battlefield setup card 'relic' script failed: LoadError")]
fn setup_script_load_error_panics() {
    run(&parse(&relic(NOT_A_FUNCTION, "")));
}

#[test]
#[should_panic(expected = "battlefield setup card 'relic' script failed: RuntimeError")]
fn setup_script_runtime_error_panics() {
    run(&parse(&relic(RAISES, "")));
}

#[test]
#[should_panic(expected = "battlefield setup card 'relic' has a script and sets trigger_operation")]
fn script_beside_trigger_operation_panics() {
    build_initial_state(&parse(&relic(
        ETB_GAIN_LIFE,
        r#"trigger_operation = "return Game.Ops.GainLife(1)""#,
    )));
}

#[test]
#[should_panic(expected = "battlefield setup card 'relic' has a script and sets mana_ability")]
fn script_beside_mana_ability_panics() {
    build_initial_state(&parse(&relic(ETB_GAIN_LIFE, r#"mana_ability = ["G"]"#)));
}

#[test]
#[should_panic(expected = "battlefield setup card 'relic' has a script and sets continuous_effect")]
fn script_beside_continuous_effect_panics() {
    build_initial_state(&parse(&relic(
        ETB_GAIN_LIFE,
        r#"continuous_effect = { layer = "power_toughness_modifying", power = 1, toughness = 1 }"#,
    )));
}

#[test]
fn script_beside_empty_mana_ability_is_allowed() {
    build_initial_state(&parse(&relic(ETB_GAIN_LIFE, "mana_ability = []")));
}

#[test]
fn hand_card_with_script_and_shortcut_is_not_checked() {
    let toml = scenario(&format!(
        r#"
[[cards]]
key = "relic"
name = "Test Relic"
types = ["Artifact"]
trigger_operation = "return Game.Ops.GainLife(1)"
script = """
{ETB_GAIN_LIFE}
"""

[[setup]]
player = 0
zone = "hand"
card = "relic"
"#
    ));
    build_initial_state(&parse(&toml));
}
