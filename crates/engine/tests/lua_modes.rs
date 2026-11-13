//! CR 700.2: a card script declares a modal spell with `s.modes()` and a modal triggered ability with
//! `:SetModes(choice, modes)`, both decoded by one mode decoder with `Game.ModeChoice.*` counts.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::{ScriptError, ScriptRuntime};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, life, minimal_card_definition, pass_both_and_drain, pay_free_and_drain,
    permanent, put_on_battlefield, two_player_main_phase_state,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CARD: ObjectId = ObjectId(0);
const MINE: ObjectId = ObjectId(1);
const THEIRS: ObjectId = ObjectId(2);

fn definition(types: CardTypeSet, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            1,
            "Test Modal",
            "test_modal",
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn creature(id: u64, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        ..minimal_card_definition(
            id,
            "Test Creature",
            "test_creature",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

/// An engine with `script` on a card in P0's hand, its modes patched in through the engine's own
/// runtime as the scenario crate does.
fn spell_engine(script: &str) -> Engine {
    let mut state = two_player_main_phase_state();
    let card = card_object(
        CARD,
        definition(CardTypeSet::SORCERY, script),
        P0,
        ZoneKind::Hand,
    );
    state.objects.insert(CARD, card);
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(CARD);
    put_on_battlefield(&mut state, permanent(MINE, creature(2, 3, 3), P0));
    put_on_battlefield(&mut state, permanent(THEIRS, creature(3, 2, 4), P1));
    let mut engine = Engine::new(state);
    let (modes, choice) = engine
        .build_modes(script)
        .expect("modes() loads")
        .expect("the script declares modes()");
    let definition = &mut engine
        .state_mut()
        .objects
        .get_mut(&CARD)
        .unwrap()
        .definition;
    definition.modes = modes;
    definition.mode_choice = choice;
    engine
}

fn cast(engine: &mut Engine, modes: Vec<u32>, targets: Vec<Target>) -> Result<(), String> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: CARD,
                modes,
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .map_err(|e| format!("{e:?}"))
}

fn cast_and_resolve(engine: &mut Engine, modes: Vec<u32>, targets: Vec<Target>) {
    cast(engine, modes, targets).expect("the cast is legal");
    pay_free_and_drain(engine);
    pass_both_and_drain(engine, P0, P1);
}

/// An engine with a land in P0's hand whose script registers a modal enters trigger.
fn land_engine(script: &str) -> Engine {
    let mut state = two_player_main_phase_state();
    let land = card_object(
        CARD,
        definition(CardTypeSet::LAND, script),
        P0,
        ZoneKind::Hand,
    );
    state.objects.insert(CARD, land);
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(CARD);
    Engine::new(state)
}

fn play_land(engine: &mut Engine) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PlayLand { card: CARD },
        })
        .expect("p0 holds priority and the land is in hand");
    drain(engine)
}

fn trigger_script(choice: &str, modes: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetModes({choice}, {modes})
    e:Register()
end
return s
"#
    )
}

fn spell_script(choice: &str, modes: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.modes()
    return {{ choice = {choice}, modes = {modes} }}
end
return s
"#
    )
}

const DAMAGE_OR_LIFE: &str = r#"{
    { target = { type = "any", count = 1 }, operation = Game.Ops.Damage(2) },
    { operation = Game.Ops.GainLife(3) },
}"#;

fn load_error(script: &str) -> String {
    match ScriptRuntime::new().build_modes(script) {
        Err(ScriptError::LoadError(message) | ScriptError::RuntimeError(message)) => message,
        Ok(_) => panic!("expected a load error"),
    }
}

fn register_error(script: &str) -> String {
    let mut state = two_player_main_phase_state();
    let definition = definition(CardTypeSet::LAND, script);
    put_on_battlefield(&mut state, permanent(CARD, definition.clone(), P0));
    match ScriptRuntime::new().run_card_script(&mut state, &definition, CARD) {
        Err(ScriptError::LoadError(message) | ScriptError::RuntimeError(message)) => message,
        Ok(()) => panic!("expected a registration error"),
    }
}

#[test]
fn new_game_loads_a_spells_modes() {
    // CR 700.2
    let script = spell_script("Game.ModeChoice.Exactly(1)", DAMAGE_OR_LIFE);
    let player = |id: PlayerId| PlayerConfig {
        player_id: id,
        library: vec![definition(CardTypeSet::INSTANT, &script)],
        commanders: Vec::new(),
        life_override: None,
    };
    let engine = Engine::new_game(GameConfig {
        players: vec![player(P0), player(P1)],
        turn_order: vec![P0, P1],
        settings: GameSettings::default(),
        rng_seed: 1,
    });
    let cards: Vec<&GameObject> = engine
        .state()
        .objects
        .values()
        .filter(|o| o.definition.name == "Test Modal")
        .collect();
    assert_eq!(cards.len(), 2);
    for card in cards {
        assert_eq!(card.definition.mode_choice, ModeChoice::Exactly(1));
        assert_eq!(card.definition.modes.len(), 2);
        assert_eq!(
            card.definition.modes[1].effect,
            Some(AbilityEffect::GainLife(3))
        );
        assert!(card.definition.modes[0].target_requirement.is_some());
        assert!(card.definition.effect.is_none());
    }
}

#[test]
fn modes_beside_operation_log_a_script_error_and_load_no_modes() {
    let script = r#"
local s = {}
function s.operation() return Game.Ops.GainLife(1) end
function s.modes()
    return { choice = Game.ModeChoice.Exactly(1), modes = {
        { operation = Game.Ops.GainLife(2) }, { operation = Game.Ops.GainLife(3) } } }
end
return s
"#;
    let player = |id: PlayerId| PlayerConfig {
        player_id: id,
        library: vec![definition(CardTypeSet::INSTANT, script)],
        commanders: Vec::new(),
        life_override: None,
    };
    let engine = Engine::new_game(GameConfig {
        players: vec![player(P0), player(P1)],
        turn_order: vec![P0, P1],
        settings: GameSettings::default(),
        rng_seed: 1,
    });
    // A library card's error text is hidden, so only the count is checked: one per copy.
    let errors = engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(e.kind, GameEventKind::ScriptError { .. }))
        .count();
    assert_eq!(errors, 2);
    assert!(
        engine
            .state()
            .objects
            .values()
            .all(|o| o.definition.modes.is_empty())
    );
}

#[test]
fn a_choose_one_spell_resolves_only_the_chosen_mode() {
    // CR 700.2a
    let script = spell_script("Game.ModeChoice.Exactly(1)", DAMAGE_OR_LIFE);
    let mut engine = spell_engine(&script);
    cast_and_resolve(&mut engine, vec![0], vec![Target::Player(P1)]);
    assert_eq!(life(&engine, P1), 18);
    assert_eq!(life(&engine, P0), 20);
}

#[test]
fn a_choose_two_spell_resolves_both_modes_in_printed_order() {
    // CR 700.2c
    let script = spell_script("Game.ModeChoice.Exactly(2)", DAMAGE_OR_LIFE);
    let mut engine = spell_engine(&script);
    cast_and_resolve(&mut engine, vec![1, 0], vec![Target::Player(P1)]);
    assert_eq!(life(&engine, P1), 18);
    assert_eq!(life(&engine, P0), 23);
}

#[test]
fn a_pawprint_budget_limits_the_chosen_weight() {
    // CR 700.2i
    let script = spell_script(
        "Game.ModeChoice.PawprintBudget(3)",
        r#"{
            { operation = Game.Ops.GainLife(1), pawprint = 1 },
            { operation = Game.Ops.GainLife(2), pawprint = 2 },
            { operation = Game.Ops.GainLife(4), pawprint = 1 },
        }"#,
    );
    let mut engine = spell_engine(&script);
    assert!(
        cast(&mut engine, vec![0, 1, 2], Vec::new()).is_err(),
        "weight 4 is over the budget"
    );
    cast_and_resolve(&mut engine, vec![0, 1], Vec::new());
    assert_eq!(life(&engine, P0), 23);
}

#[test]
fn a_per_mode_cost_is_paid_only_when_its_mode_is_chosen() {
    // CR 700.2h
    let script = spell_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { operation = Game.Ops.GainLife(1) },
            { operation = Game.Ops.GainLife(5), cost = Game.Costs.Life(2) },
        }"#,
    );
    let mut engine = spell_engine(&script);
    cast(&mut engine, vec![1], Vec::new()).expect("the cast is legal");
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Life(2)],
        })
        .expect("mode 2's own life cost");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!(life(&engine, P0), 23);
}

#[test]
fn an_opponent_chooses_the_mode_of_a_spell_and_a_function_mode_runs() {
    // CR 700.2e
    let script = spell_script(
        "Game.ModeChoice.ExactlyChosenByOpponent(1)",
        r#"{
            { operation = Game.Ops.GainLife(1) },
            { operation = function(c, controller) return Game.Ops.GainLife(4) end },
        }"#,
    );
    let mut engine = spell_engine(&script);
    cast(&mut engine, Vec::new(), Vec::new()).expect("the cast is legal");
    pay_free_and_drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: P1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingModeChoice { player, .. } if player == P1),
        "{status:?}"
    );
    engine.submit_mode_choice(vec![1]).expect("a legal choice");
    drain(&mut engine);
    assert_eq!(life(&engine, P0), 24);
}

#[test]
fn a_function_mode_that_is_the_runtimes_first_function_resolves() {
    // The first function a runtime registers must not read as the `ScriptHandle(0)` placeholder.
    assert_eq!(
        ScriptRuntime::new()
            .load_function("return function() end")
            .expect("loads"),
        ScriptHandle(1)
    );
    let script = spell_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { operation = function(c, controller) return Game.Ops.GainLife(4) end },
            { operation = Game.Ops.GainLife(1) },
        }"#,
    );
    let mut engine = spell_engine(&script);
    assert_eq!(
        engine.state().objects[&CARD].definition.modes[0].effect,
        Some(AbilityEffect::Script(ScriptHandle(1)))
    );
    cast_and_resolve(&mut engine, vec![0], Vec::new());
    assert_eq!(life(&engine, P0), 24);
}

#[test]
fn a_function_or_chain_mode_cannot_join_another_mode() {
    let script = spell_script(
        "Game.ModeChoice.UpTo(2)",
        r#"{
            { operation = function(c, controller) return Game.Ops.GainLife(4) end },
            { operation = Game.Ops.Chain(Game.Ops.GainLife(1), Game.Ops.GainLife(2)) },
            { operation = Game.Ops.GainLife(1) },
        }"#,
    );
    let mut engine = spell_engine(&script);
    assert!(cast(&mut engine, vec![0, 2], Vec::new()).is_err());
    assert!(cast(&mut engine, vec![1, 2], Vec::new()).is_err());
}

#[test]
fn a_chain_mode_resolves_alone_on_a_spell() {
    // CR 608.2c
    let script = spell_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { operation = Game.Ops.GainLife(1) },
            { operation = Game.Ops.Chain(Game.Ops.GainLife(2), Game.Ops.GainLife(3)) },
        }"#,
    );
    let mut engine = spell_engine(&script);
    cast_and_resolve(&mut engine, vec![1], Vec::new());
    assert_eq!(life(&engine, P0), 25);
}

#[test]
fn a_fight_mode_resolves() {
    // CR 701.14a
    let script = spell_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { target = {
                { card_types = { "Creature" }, controller = "you", count = 1 },
                { card_types = { "Creature" }, controller = "opponent", count = 1 },
            }, operation = Game.Ops.Fight },
            { operation = Game.Ops.GainLife(1) },
        }"#,
    );
    let mut engine = spell_engine(&script);
    cast_and_resolve(
        &mut engine,
        vec![0],
        vec![Target::Object(MINE), Target::Object(THEIRS)],
    );
    assert_eq!(engine.state().objects[&MINE].damage_marked, 2);
    assert_eq!(engine.state().objects[&THEIRS].damage_marked, 3);
}

#[test]
fn a_bite_mode_resolves() {
    // CR 701.14a
    let script = spell_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { operation = Game.Ops.GainLife(1) },
            { target = {
                { card_types = { "Creature" }, controller = "you", count = 1 },
                { card_types = { "Creature" }, controller = "opponent", count = 1 },
            }, operation = Game.Ops.Bite },
        }"#,
    );
    let mut engine = spell_engine(&script);
    cast_and_resolve(
        &mut engine,
        vec![1],
        vec![Target::Object(MINE), Target::Object(THEIRS)],
    );
    assert_eq!(engine.state().objects[&MINE].damage_marked, 0);
    assert_eq!(engine.state().objects[&THEIRS].damage_marked, 3);
}

#[test]
fn a_modal_enters_trigger_asks_for_a_mode_then_its_targets() {
    // CR 603.3c
    let script = trigger_script("Game.ModeChoice.Exactly(1)", DAMAGE_OR_LIFE);
    let mut engine = land_engine(&script);
    let status = play_land(&mut engine);
    let StepStatus::AwaitingTriggerModeChoice {
        player, min, max, ..
    } = status
    else {
        panic!("expected a mode choice, got {status:?}")
    };
    assert_eq!((player, min, max), (P0, 1, 1));
    engine
        .submit_trigger_mode_choice(vec![0])
        .expect("a legal choice");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingTriggerTargets { .. }),
        "{status:?}"
    );
    engine
        .submit_trigger_targets(vec![Target::Player(P1)])
        .expect("a legal target");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!(life(&engine, P1), 18);
}

#[test]
fn an_opponent_chooses_the_mode_of_a_trigger() {
    // CR 700.2e
    let script = trigger_script(
        "Game.ModeChoice.ExactlyChosenByOpponent(1)",
        "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(3) } }",
    );
    let mut engine = land_engine(&script);
    let status = play_land(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingTriggerModeChoice { player, .. } if player == P1),
        "{status:?}"
    );
    engine
        .submit_trigger_mode_choice(vec![0])
        .expect("a legal choice");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!(life(&engine, P0), 21);
}

#[test]
fn a_function_mode_and_a_chain_mode_resolve_alone_on_a_trigger() {
    let script = trigger_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { operation = function(c, controller) return Game.Ops.GainLife(4) end },
            { operation = Game.Ops.Chain(Game.Ops.GainLife(2), Game.Ops.GainLife(3)) },
        }"#,
    );
    for (mode, expected) in [(0, 24), (1, 25)] {
        let mut engine = land_engine(&script);
        play_land(&mut engine);
        engine
            .submit_trigger_mode_choice(vec![mode])
            .expect("a legal choice");
        drain(&mut engine);
        pass_both_and_drain(&mut engine, P0, P1);
        assert_eq!(life(&engine, P0), expected, "mode {mode}");
    }
}

#[test]
fn spell_mode_load_errors() {
    // CR 700.2
    let two = "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(2) } }";
    let cases: Vec<(String, &str)> = vec![
        (
            spell_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1) } }",
            ),
            "at least two modes",
        ),
        (
            spell_script("Game.ModeChoice.Exactly(0)", two),
            "at least 1",
        ),
        (
            spell_script("Game.ModeChoice.UpToRepeatable(0)", two),
            "at least 1",
        ),
        (
            spell_script("Game.ModeChoice.PawprintBudget(0)", two),
            "at least 1",
        ),
        (
            spell_script("Game.ModeChoice.Exactly(3)", two),
            "only 2 are declared",
        ),
        (
            spell_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1) }, { target = { type = \"any\", count = 1 } } }",
            ),
            "mode 2 has no operation",
        ),
        (
            spell_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(2), colour = 1 } }",
            ),
            "mode 2 has unknown key 'colour'",
        ),
        (
            spell_script(
                "Game.ModeChoice.Exactly(1)",
                r#"{ { operation = Game.Ops.GainLife(1) },
                     { operation = Game.Ops.ChainEach({ Game.Ops.Damage(1), 1 }, { Game.Ops.Damage(1), 1 }),
                       target = { type = "any", count = 1 } } }"#,
            ),
            "mode 2: Game.Ops.ChainEach",
        ),
        (
            spell_script(
                "Game.ModeChoice.ExactlyChosenByOpponent(1)",
                "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.Damage(1), target = { type = \"any\", count = 1 } } }",
            ),
            "mode 2: a mode chosen by an opponent",
        ),
        (
            spell_script(
                "Game.ModeChoice.ExactlyChosenByOpponent(1)",
                "{ { operation = Game.Ops.GainLife(1), cost = Game.Costs.Life(1) }, { operation = Game.Ops.GainLife(2) } }",
            ),
            "mode 1: a mode chosen by an opponent",
        ),
        (
            spell_script(
                "Game.ModeChoice.PawprintBudget(2)",
                "{ { operation = Game.Ops.GainLife(1), pawprint = 1 }, { operation = Game.Ops.GainLife(2) } }",
            ),
            "mode 2 needs a pawprint",
        ),
        (
            spell_script(
                "Game.ModeChoice.PawprintBudget(2)",
                "{ { operation = Game.Ops.GainLife(1), pawprint = 0 }, { operation = Game.Ops.GainLife(2), pawprint = 1 } }",
            ),
            "mode 1 needs a pawprint",
        ),
        (
            spell_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1), pawprint = 1 }, { operation = Game.Ops.GainLife(2) } }",
            ),
            "mode 1: pawprint needs",
        ),
        (
            spell_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1), cost = Game.Costs.Mana(\"{1}\") }, { operation = Game.Ops.GainLife(2) } }",
            ),
            "mode 1: a per-mode cost can't be Game.Costs.Mana",
        ),
        (
            spell_script("3", two),
            "must be a Game.ModeChoice",
        ),
        (
            "local s = {} function s.modes() return { choice = Game.ModeChoice.Exactly(1), modes = {}, extra = 1 } end return s".to_string(),
            "unknown key 'extra'",
        ),
    ];
    for (script, expected) in cases {
        let message = load_error(&script);
        assert!(
            message.contains(expected),
            "expected {expected:?} in {message:?}"
        );
    }
}

#[test]
fn trigger_mode_load_errors() {
    let two = "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(2) } }";
    let set_modes = |extra: &str| {
        format!(
            r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetModes(Game.ModeChoice.Exactly(1), {two})
    {extra}
    e:Register()
end
return s
"#
        )
    };
    let cases: Vec<(String, &str)> = vec![
        (
            trigger_script("Game.ModeChoice.PawprintBudget(2)", two),
            "PawprintBudget is only for a spell",
        ),
        (
            trigger_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1), cost = Game.Costs.Life(1) }, { operation = Game.Ops.GainLife(2) } }",
            ),
            "SetModes mode 1: a per-mode cost is only for a spell",
        ),
        (
            trigger_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(2), pawprint = 1 } }",
            ),
            "SetModes mode 2: pawprint is only for a spell",
        ),
        (
            set_modes("e:SetOperation(Game.Ops.GainLife(1))"),
            "both SetOperation() and SetModes()",
        ),
        (
            set_modes("e:SetTarget({ type = \"player\" })"),
            "both SetTarget() and SetModes()",
        ),
    ];
    for (script, expected) in cases {
        let message = register_error(&script);
        assert!(
            message.contains(expected),
            "expected {expected:?} in {message:?}"
        );
    }
}

fn activated_script(choice: &str, modes: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetModes({choice}, {modes})
    e:Register()
end
return s
"#
    )
}

/// A land in play whose script registered a modal activated ability, beside a creature each.
fn activated_engine(script: &str, with_creatures: bool) -> Engine {
    let mut engine = land_engine(script);
    if with_creatures {
        let state = engine.state_mut();
        put_on_battlefield(state, permanent(MINE, creature(2, 3, 3), P0));
        put_on_battlefield(state, permanent(THEIRS, creature(3, 2, 4), P1));
    }
    play_land(&mut engine);
    engine
}

/// The played land, which is a new object on the battlefield (CR 400.7).
fn land_id(engine: &Engine) -> ObjectId {
    let battlefield = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    *battlefield
        .contents
        .iter()
        .find(|&&id| engine.state().objects[&id].definition.name == "Test Modal")
        .expect("the land is on the battlefield")
}

fn activate(engine: &mut Engine, modes: Vec<u32>, targets: Vec<Target>) -> Result<(), String> {
    let source = land_id(engine);
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                modes,
                targets,
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .map_err(|e| format!("{e:?}"))
}

fn activate_and_resolve(engine: &mut Engine, modes: Vec<u32>, targets: Vec<Target>) {
    activate(engine, modes, targets).expect("the activation is legal");
    pay_free_and_drain(engine);
    pass_both_and_drain(engine, P0, P1);
}

fn activate_entry(engine: &Engine) -> Option<stackfish_engine::decision::ActivateEntry> {
    let source = land_id(engine);
    engine
        .legal_actions()
        .into_iter()
        .find_map(|entry| match entry {
            stackfish_engine::decision::Entry::Activate(entry) if entry.source == source => {
                Some(entry)
            }
            _ => None,
        })
}

#[test]
fn a_modal_activated_ability_announces_its_mode_and_targets_in_the_activation() {
    // CR 602.2b
    let script = activated_script("Game.ModeChoice.Exactly(1)", DAMAGE_OR_LIFE);
    let mut engine = activated_engine(&script, false);
    activate_and_resolve(&mut engine, vec![0], vec![Target::Player(P1)]);
    assert_eq!((life(&engine, P0), life(&engine, P1)), (20, 18));
    let mut engine = activated_engine(&script, false);
    activate_and_resolve(&mut engine, vec![1], Vec::new());
    assert_eq!((life(&engine, P0), life(&engine, P1)), (23, 20));
}

#[test]
fn a_modal_activation_records_its_modes_on_the_stack_in_printed_order() {
    // CR 608.2c
    let script = activated_script("Game.ModeChoice.Exactly(2)", DAMAGE_OR_LIFE);
    let mut engine = activated_engine(&script, false);
    activate(&mut engine, vec![1, 0], vec![Target::Player(P1)]).expect("a legal activation");
    drain(&mut engine);
    let top = engine
        .state()
        .stack
        .last()
        .expect("the ability is on the stack");
    assert_eq!(top.chosen_modes, vec![0, 1]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!((life(&engine, P0), life(&engine, P1)), (23, 18));
}

#[test]
fn a_function_mode_and_a_chain_mode_resolve_alone_on_an_activated_ability() {
    let script = activated_script(
        "Game.ModeChoice.UpTo(2)",
        r#"{
            { operation = function(c, controller) return Game.Ops.GainLife(4) end },
            { operation = Game.Ops.Chain(Game.Ops.GainLife(2), Game.Ops.GainLife(3)) },
            { operation = Game.Ops.GainLife(1) },
        }"#,
    );
    for (mode, expected) in [(0, 24), (1, 25)] {
        let mut engine = activated_engine(&script, false);
        activate_and_resolve(&mut engine, vec![mode], Vec::new());
        assert_eq!(life(&engine, P0), expected, "mode {mode}");
    }
    let mut engine = activated_engine(&script, false);
    assert!(activate(&mut engine, vec![0, 2], Vec::new()).is_err());
    assert!(activate(&mut engine, vec![1, 2], Vec::new()).is_err());
}

#[test]
fn an_activation_refuses_a_selection_its_modes_do_not_allow() {
    // CR 700.2a, 700.2d
    let script = activated_script("Game.ModeChoice.Exactly(1)", DAMAGE_OR_LIFE);
    let mut engine = activated_engine(&script, false);
    for (modes, targets) in [
        (Vec::new(), Vec::new()),
        (vec![0, 1], vec![Target::Player(P1)]),
        (vec![2], Vec::new()),
        (vec![1], vec![Target::Player(P1)]),
    ] {
        assert!(
            activate(&mut engine, modes.clone(), targets).is_err(),
            "{modes:?}"
        );
    }
    let repeat = activated_script("Game.ModeChoice.UpTo(2)", DAMAGE_OR_LIFE);
    let mut engine = activated_engine(&repeat, false);
    assert!(activate(&mut engine, vec![1, 1], Vec::new()).is_err());
}

#[test]
fn an_activation_with_modes_on_a_non_modal_ability_is_refused() {
    let script = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;
    let mut engine = activated_engine(script, false);
    let message = activate(&mut engine, vec![0], Vec::new()).unwrap_err();
    assert!(message.contains("isn't modal"), "{message}");
}

#[test]
fn the_activate_entry_lists_the_mode_range_and_targets_per_mode() {
    // CR 700.2a
    let script = activated_script(
        "Game.ModeChoice.Exactly(1)",
        r#"{
            { target = { type = "object", card_types = { "Creature" }, count = 1 },
              operation = Game.Ops.Damage(1) },
            { operation = Game.Ops.GainLife(2) },
        }"#,
    );
    let engine = activated_engine(&script, true);
    let entry = activate_entry(&engine).expect("the ability is listed");
    let range = entry.modes.expect("a modal ability lists its range");
    assert_eq!((range.mode_count, range.min, range.max), (2, 1, 1));
    let stackfish_engine::decision::EntryTargets::PerMode(per_mode) = entry.targets else {
        panic!("expected per-mode slots");
    };
    assert_eq!(per_mode[0].len(), 1);
    assert!(per_mode[0][0].candidates.contains(&Target::Object(THEIRS)));
    assert!(per_mode[1].is_empty());
}

#[test]
fn a_modal_ability_with_no_mode_whose_targets_exist_is_not_listed() {
    // CR 700.2a
    let creature_target = r#"target = { type = "object", card_types = { "Creature" }, count = 1 }"#;
    let script = activated_script(
        "Game.ModeChoice.Exactly(1)",
        &format!(
            "{{ {{ {creature_target}, operation = Game.Ops.Damage(1) }}, \
               {{ {creature_target}, operation = Game.Ops.Tap }} }}"
        ),
    );
    let engine = activated_engine(&script, false);
    assert!(activate_entry(&engine).is_none());
    let engine = activated_engine(&script, true);
    assert!(activate_entry(&engine).is_some());
}

#[test]
fn activated_mode_load_errors() {
    let two = "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(2) } }";
    let with = |extra: &str| {
        format!(
            r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    {extra}
    e:Register()
end
return s
"#
        )
    };
    let modes = format!("e:SetModes(Game.ModeChoice.Exactly(1), {two})");
    let cases: Vec<(String, &str)> = vec![
        (
            activated_script("Game.ModeChoice.ExactlyChosenByOpponent(1)", two),
            "SetModes: an activated ability's modes can't be chosen by an opponent",
        ),
        (
            activated_script("Game.ModeChoice.UpToChosenByOpponent(1)", two),
            "SetModes: an activated ability's modes can't be chosen by an opponent",
        ),
        (
            activated_script("Game.ModeChoice.PawprintBudget(2)", two),
            "PawprintBudget is only for a spell",
        ),
        (
            activated_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1), cost = Game.Costs.Life(1) }, { operation = Game.Ops.GainLife(2) } }",
            ),
            "SetModes mode 1: a per-mode cost is only for a spell",
        ),
        (
            activated_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.GainLife(2), pawprint = 1 } }",
            ),
            "SetModes mode 2: pawprint is only for a spell",
        ),
        (
            activated_script(
                "Game.ModeChoice.Exactly(1)",
                r#"{ { operation = Game.Ops.GainLife(1) }, { operation = Game.Ops.AddMana("G") } }"#,
            ),
            "SetModes mode 2: an activated ability's mode can't add mana",
        ),
        (
            activated_script(
                "Game.ModeChoice.Exactly(1)",
                r#"{ { operation = Game.Ops.AddManaChoice("WUBRG", 1) }, { operation = Game.Ops.GainLife(2) } }"#,
            ),
            "SetModes mode 1: an activated ability's mode can't add mana",
        ),
        (
            activated_script(
                "Game.ModeChoice.Exactly(1)",
                "{ { operation = Game.Ops.GainLife(1) } }",
            ),
            "SetModes: a modal ability needs at least two modes",
        ),
        (
            with(&format!(
                "e:SetOperation(Game.Ops.GainLife(1))\n    {modes}"
            )),
            "both SetOperation() and SetModes()",
        ),
        (
            with(&format!(
                "e:SetTarget({{ type = \"player\" }})\n    {modes}"
            )),
            "both SetTarget() and SetModes()",
        ),
        (
            with("e:AddCost(Game.Costs.TapSelf)"),
            "no SetOperation() or SetModes() - an activated ability needs an operation",
        ),
    ];
    for (script, expected) in cases {
        let message = register_error(&script);
        assert!(
            message.contains(expected),
            "expected {expected:?} in {message:?}"
        );
    }
}

#[test]
fn a_priority_answer_checks_an_activations_modes_against_the_entry() {
    // CR 700.2a
    use stackfish_engine::decision::{Answer, RejectionCode, Reply};
    let script = activated_script("Game.ModeChoice.Exactly(1)", DAMAGE_OR_LIFE);
    let mut engine = activated_engine(&script, false);
    let source = land_id(&engine);
    let answer = |engine: &mut Engine, modes: Vec<u32>, targets: Vec<Target>| {
        let decision = engine.open_decision().expect("a Decision is open");
        engine.answer(
            P0,
            Answer {
                decision: decision.id,
                reply: Reply::Action(PlayerAction {
                    player: P0,
                    kind: PlayerActionKind::ActivateAbility {
                        source,
                        ability_index: 0,
                        modes,
                        targets,
                        chosen_colors: Vec::new(),
                        x_value: None,
                    },
                }),
            },
        )
    };
    for (modes, targets) in [
        (vec![2], Vec::new()),
        (vec![1], vec![Target::Player(P1)]),
        (vec![0, 1], vec![Target::Player(P1)]),
    ] {
        let rejection = answer(&mut engine, modes.clone(), targets).unwrap_err();
        assert_eq!(rejection.code, RejectionCode::InvalidChoice, "{modes:?}");
    }
    answer(&mut engine, vec![0], vec![Target::Player(P1)]).expect("a listed activation");
}
