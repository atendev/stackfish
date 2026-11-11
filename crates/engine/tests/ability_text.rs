//! CR 405.4, 113.7a: an ability on the stack carries the text a script gave it and its index on the source, both captured at push.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SOURCE: ObjectId = ObjectId(1);
const LIBRARY_CARD: ObjectId = ObjectId(2);

const DRAW_THEN_GAIN: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetText("Draw a card.")
    a:SetOperation(Game.Ops.DrawCards(1))
    a:Register()
    local t = Game.CreateTriggeredAbility(c)
    t:SetText("Whenever you draw a card, you gain 1 life.")
    t:SetCondition(Game.Conditions.DrawsCard)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

const NO_TEXT: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

const EMPTY_TEXT: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetText("  ")
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

const TEXT_TWICE: &str = r#"
local s = {}
function s.register_abilities(c)
    local t = Game.CreateTriggeredAbility(c)
    t:SetText("One.")
    t:SetText("Two.")
    t:SetCondition(Game.Conditions.DrawsCard)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

fn definition(id: u64, name: &str, script: Option<&str>) -> CardDefinition {
    CardDefinition {
        script_ref: script
            .map(|s| ScriptReference::Inline(s.to_string()))
            .unwrap_or(ScriptReference::ByKey(name.to_string())),
        ..minimal_card_definition(
            id,
            name,
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::ARTIFACT,
        )
    }
}

fn setup(script: &str) -> (GameState, Result<(), String>) {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            P0,
            P1,
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield],
        ))
    };
    let source = definition(SOURCE.0, "Source", Some(script));
    state.objects.insert(
        SOURCE,
        card_object(SOURCE, source.clone(), P0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(SOURCE);
    state.objects.insert(
        LIBRARY_CARD,
        card_object(
            LIBRARY_CARD,
            definition(LIBRARY_CARD.0, "Filler", None),
            P0,
            ZoneKind::Library,
        ),
    );
    state
        .zone_mut(ZoneKind::Library, Some(P0))
        .unwrap()
        .push_bottom(LIBRARY_CARD);
    let registered = ScriptRuntime::new()
        .run_card_script(&mut state, &source, SOURCE)
        .map_err(|e| format!("{e:?}"));
    (state, registered)
}

fn engine_with(script: &str) -> Engine {
    let (state, registered) = setup(script);
    registered.expect("script registers");
    Engine::new(state)
}

fn activate(engine: &mut Engine) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: SOURCE,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
}

fn pass_both(engine: &mut Engine) {
    for player in [P0, P1] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        drain(engine);
    }
}

// CR 405.4
#[test]
fn an_activated_ability_carries_its_text_and_index() {
    let mut engine = engine_with(DRAW_THEN_GAIN);
    activate(&mut engine);
    let top = engine
        .state()
        .stack
        .last()
        .expect("the ability is on the stack");
    assert!(matches!(top.kind, StackEntryKind::ActivatedAbility));
    assert_eq!(top.ability_index, Some(0));
    assert_eq!(top.ability_text.as_deref(), Some("Draw a card."));
}

// CR 405.4, 113.7
// The trigger is the source's second ability, so its index counts the activated one first.
#[test]
fn a_triggered_ability_carries_its_text_and_index() {
    let mut engine = engine_with(DRAW_THEN_GAIN);
    activate(&mut engine);
    pass_both(&mut engine);
    let top = engine
        .state()
        .stack
        .last()
        .expect("the trigger is on the stack");
    assert!(matches!(top.kind, StackEntryKind::TriggeredAbility));
    assert_eq!(top.ability_index, Some(1));
    assert_eq!(
        top.ability_text.as_deref(),
        Some("Whenever you draw a card, you gain 1 life.")
    );
}

// CR 405.4
#[test]
fn an_ability_without_set_text_has_no_text() {
    let mut engine = engine_with(NO_TEXT);
    activate(&mut engine);
    let top = engine
        .state()
        .stack
        .last()
        .expect("the ability is on the stack");
    assert_eq!(top.ability_index, Some(0));
    assert_eq!(top.ability_text, None);
}

#[test]
fn set_text_refuses_blank_text() {
    let (_, registered) = setup(EMPTY_TEXT);
    let error = registered.expect_err("blank text is a load error");
    assert!(error.contains("SetText"), "{error}");
}

#[test]
fn set_text_refuses_a_second_call() {
    let (_, registered) = setup(TEXT_TWICE);
    let error = registered.expect_err("a second SetText is a load error");
    assert!(error.contains("SetText"), "{error}");
}
