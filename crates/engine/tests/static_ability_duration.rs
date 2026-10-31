//! `StaticAbilityBuilder::SetDuration` (CR 611.2a) gives a Lua static ability a limited duration instead of `EffectDuration::Permanent`. The drain mechanics are in effect_duration_expiration.rs, and one end-to-end case here shows the two connect.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

const UNTIL_END_OF_TURN: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Self)
    e:SetDuration(Game.Durations.UntilEndOfTurn)
    e:Register()
end
return s
"#;

const UNTIL_YOUR_NEXT_TURN: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Self)
    e:SetDuration(Game.Durations.UntilYourNextTurn)
    e:Register()
end
return s
"#;

const UNTIL_END_OF_COMBAT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Self)
    e:SetDuration(Game.Durations.UntilEndOfCombat)
    e:Register()
end
return s
"#;

const EXPLICIT_PERMANENT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Self)
    e:SetDuration(Game.Durations.Permanent)
    e:Register()
end
return s
"#;

const NO_SET_DURATION: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

fn bear_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Static ability duration test fixture.".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn registered_duration(script: &str) -> EffectDuration {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(script);
    state.objects.insert(
        bear,
        card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield),
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, bear)
        .expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    state.continuous_effects[0].duration.clone()
}

#[test]
fn set_duration_until_end_of_turn_is_decoded() {
    assert!(matches!(
        registered_duration(UNTIL_END_OF_TURN),
        EffectDuration::UntilEndOfTurn
    ));
}

#[test]
fn set_duration_until_your_next_turn_is_decoded() {
    assert!(matches!(
        registered_duration(UNTIL_YOUR_NEXT_TURN),
        EffectDuration::UntilYourNextTurn
    ));
}

#[test]
fn set_duration_until_end_of_combat_is_decoded() {
    assert!(matches!(
        registered_duration(UNTIL_END_OF_COMBAT),
        EffectDuration::UntilEndOfCombat
    ));
}

#[test]
fn set_duration_explicit_permanent_is_decoded() {
    assert!(matches!(
        registered_duration(EXPLICIT_PERMANENT),
        EffectDuration::Permanent
    ));
}

#[test]
fn omitting_set_duration_still_defaults_to_permanent() {
    assert!(matches!(
        registered_duration(NO_SET_DURATION),
        EffectDuration::Permanent
    ));
}

fn step_until_pause(engine: &mut Engine) -> StepStatus {
    loop {
        let r = engine.step();
        if !matches!(r.status, StepStatus::Progressed) {
            return r.status;
        }
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .expect("PassPriority should be accepted");
}

fn pass_both_players(engine: &mut Engine, first: PlayerId, second: PlayerId) -> StepStatus {
    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == first));
    pass(engine, first);

    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == second));
    pass(engine, second);

    step_until_pause(engine)
}

#[test]
fn lua_registered_until_end_of_turn_static_ability_drains_at_cleanup() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(1);

    // Register directly through run_card_script, then give the state to an Engine at the End step, so cleanup's drain is under test and not registration.
    let mut state = two_player_state();
    state.phase = Phase::Ending;
    state.step = Step::End;
    state.turn_number = 2;
    state.last_active_turn_number = HashMap::from([(p0, 1), (p1, 1)]);
    let definition = bear_definition(UNTIL_END_OF_TURN);
    state.objects.insert(
        bear,
        card_object(bear, definition.clone(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(bear);
    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, bear)
        .expect("script self-registers cleanly");
    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(
        state.continuous_effects[0].duration,
        EffectDuration::UntilEndOfTurn
    ));

    let mut engine = Engine::new(state);
    pass_both_players(&mut engine, p0, p1);

    // CR 502.4: no priority during Untap, so the engine runs straight
    // through it into Upkeep before pausing again.
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
    assert!(
        engine.state().continuous_effects.is_empty(),
        "the UntilEndOfTurn static effect should have drained during cleanup"
    );
}
