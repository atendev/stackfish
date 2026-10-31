//! `run_card_script` registrations are idempotent across zone changes. `GameState::script_ability_ids` marks script-registered abilities, which `remap_abilities` drops and re-runs for the new zone, while hand-built abilities carry across.
//! A script run for an object off the battlefield registers no static effects, replacement effects or modifiers (CR 113.6). It also runs for a spell entering the stack (CR 601.2a), where only "when you cast this spell" triggers register.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const ADDITIONAL_LAND: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateLandPlayModifier(c)
    m:SetAffected(Game.Affected.You)
    m:SetAdditional(1)
    m:Register()
end
return s
"#;

const ETB_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const CAST_TRIGGER_AND_ETB: &str = r#"
local s = {}
function s.register_abilities(c)
    local cast = Game.CreateTriggeredAbility(c)
    cast:SetCondition(Game.Conditions.Cast)
    cast:SetOperation(Game.Ops.GainLife(3))
    cast:Register()

    local etb = Game.CreateTriggeredAbility(c)
    etb:SetCondition(Game.Conditions.Enters)
    etb:SetOperation(Game.Ops.GainLife(1))
    etb:Register()
end
return s
"#;

fn scripted(card_types: CardTypeSet, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            1,
            "Scripted",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            card_types,
        )
    }
}

fn main_phase_state() -> GameState {
    let p0 = PlayerId(0);
    GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            p0,
            PlayerId(1),
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
            ],
        ))
    }
}

fn put_in_hand(state: &mut GameState, id: ObjectId, definition: &CardDefinition, owner: PlayerId) {
    state.objects.insert(
        id,
        card_object(id, definition.clone(), owner, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(owner))
        .unwrap()
        .push_bottom(id);
}

fn cast_and_pay(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("cast is legal");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a payment pause, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("{0} cost");
    drain(engine);
}

fn pass_both(engine: &mut Engine) {
    for player in [PlayerId(0), PlayerId(1)] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        drain(engine);
    }
}

#[test]
fn a_land_played_from_hand_registers_its_modifier_exactly_once() {
    let p0 = PlayerId(0);
    let mut state = main_phase_state();
    let land = ObjectId(0);
    let definition = scripted(CardTypeSet::LAND, ADDITIONAL_LAND);
    put_in_hand(&mut state, land, &definition, p0);

    // The hand-entry run, as a draw would do it.
    ScriptRuntime::new()
        .run_card_script(&mut state, &definition, land)
        .unwrap();
    assert!(
        state.land_play_modifiers.is_empty(),
        "a static modifier must not take effect while its card is still in hand"
    );

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().land_play_modifiers.len(), 1);
    let battlefield_id = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents[0];
    assert_eq!(engine.state().land_play_modifiers[0].source, battlefield_id);
}

#[test]
fn a_creature_drawn_then_cast_has_one_etb_trigger_not_two() {
    let p0 = PlayerId(0);
    let mut state = main_phase_state();
    let card = ObjectId(0);
    let definition = scripted(CardTypeSet::CREATURE, ETB_GAIN_LIFE);
    put_in_hand(&mut state, card, &definition, p0);
    ScriptRuntime::new()
        .run_card_script(&mut state, &definition, card)
        .unwrap();
    assert_eq!(
        state.abilities.len(),
        1,
        "the hand-entry run registers the ability"
    );

    let mut engine = Engine::new(state);
    cast_and_pay(&mut engine, p0, card);
    pass_both(&mut engine);

    let battlefield_id = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents[0];
    let on_permanent: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == battlefield_id)
        .collect();
    assert_eq!(
        on_permanent.len(),
        1,
        "exactly one ETB trigger, not a carried-over copy plus a fresh one"
    );
    assert_eq!(
        engine.state().abilities.len(),
        1,
        "and nothing left behind under an older id"
    );
}

#[test]
fn a_hand_built_ability_still_carries_across_a_zone_change() {
    let p0 = PlayerId(0);
    let mut state = main_phase_state();
    let card = ObjectId(0);
    let definition = minimal_card_definition(
        1,
        "Vanilla",
        "unused",
        ManaCost {
            symbols: Vec::new(),
        },
        CardTypeSet::CREATURE,
    );
    put_in_hand(&mut state, card, &definition, p0);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(9999),
        source: card,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    cast_and_pay(&mut engine, p0, card);
    pass_both(&mut engine);

    let battlefield_id = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents[0];
    assert_eq!(
        engine
            .state()
            .abilities
            .iter()
            .filter(|a| a.source == battlefield_id && a.id == AbilityId(9999))
            .count(),
        1,
        "an ability that didn't come from run_card_script is remapped, not dropped"
    );
}

#[test]
fn a_cast_trigger_declared_in_lua_registers_on_the_stack_and_fires() {
    let p0 = PlayerId(0);
    let mut state = main_phase_state();
    let card = ObjectId(0);
    put_in_hand(
        &mut state,
        card,
        &scripted(CardTypeSet::CREATURE, CAST_TRIGGER_AND_ETB),
        p0,
    );

    let mut engine = Engine::new(state);
    cast_and_pay(&mut engine, p0, card);

    let stack_id = engine.state().stack[0].stack_id;
    let on_stack: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == stack_id)
        .collect();
    assert_eq!(
        on_stack.len(),
        1,
        "only the Cast trigger exists on the stack; the ETB trigger waits for the battlefield"
    );
    assert!(matches!(
        on_stack[0].condition,
        Some(ConditionSpec::Primitive(ConditionPrimitive::Cast))
    ));
    assert_eq!(
        engine.state().stack.len(),
        2,
        "the spell plus its own cast trigger"
    );

    // Resolve the cast trigger, then the spell.
    pass_both(&mut engine);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        23,
        "cast trigger gained 3"
    );
    pass_both(&mut engine);
    // The permanent's ETB trigger goes on the stack as it enters.
    pass_both(&mut engine);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        24,
        "ETB trigger gained 1 on entering"
    );
    let battlefield_id = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents[0];
    let remaining: Vec<_> = engine.state().abilities.iter().collect();
    assert_eq!(
        remaining.len(),
        1,
        "the Cast trigger is gone once the spell has left the stack"
    );
    assert_eq!(remaining[0].source, battlefield_id);
}
