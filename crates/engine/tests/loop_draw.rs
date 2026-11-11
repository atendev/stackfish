//! CR 104.4b: a loop of mandatory actions ends the game as a draw. The loop is a
//! 0/0 creature that returns itself whenever it dies (CR 704.5f), so every lap
//! repeats the state up to fresh object, stack, ability and script ids.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition,
    put_on_battlefield, two_player_state,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const LAPS: usize = 20;

const RETURNER: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Dies)
    e:SetOperation(Game.Ops.ReturnSelfWithCounter(Game.Counter.Charge))
    e:Register()
end
return s
"#;

fn definition(name: &str, types: CardTypeSet, toughness: Option<i32>) -> CardDefinition {
    CardDefinition {
        power: toughness.map(|_| PTValue::Fixed(0)),
        toughness: toughness.map(PTValue::Fixed),
        ..minimal_card_definition(
            1,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn ability(
    source: ObjectId,
    condition: ConditionPrimitive,
    effect: AbilityEffect,
) -> RegisteredAbility {
    RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(source.0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(effect),
        intervening_if: None,
    }
}

/// A main-phase state with the returning 0/0 on P0's battlefield. It dies on the
/// first state-based action check.
fn loop_state() -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: std::collections::HashMap::from([(P0, 1), (P1, 1)]),
        ..two_player_state(hand_battlefield_graveyard_zones(P0, P1))
    };
    let returner = ObjectId(0);
    let card = CardDefinition {
        script_ref: ScriptReference::Inline(RETURNER.to_string()),
        ..definition("Returner", CardTypeSet::CREATURE, Some(0))
    };
    put_on_battlefield(
        &mut state,
        card_object(returner, card.clone(), P0, ZoneKind::Battlefield),
    );
    // A fixture permanent never entered, so its script is registered by hand.
    ScriptRuntime::new()
        .run_card_script(&mut state, &card, returner)
        .expect("script registers");
    state
}

fn pass(engine: &mut Engine, player: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

/// Passes for whoever holds priority until the game ends or `laps` rounds of
/// two passes have gone by. Returns the final status.
fn pass_around(engine: &mut Engine, laps: usize) -> StepStatus {
    let mut status = drain(engine);
    for _ in 0..laps * 2 {
        let StepStatus::AwaitingAction { player } = status else {
            return status;
        };
        status = pass(engine, player);
    }
    status
}

fn is_draw(status: &StepStatus) -> bool {
    matches!(status, StepStatus::GameOver { winner: None })
}

#[test]
fn a_mandatory_trigger_loop_is_a_draw() {
    // CR 104.4b
    let mut engine = Engine::new(loop_state());
    let status = pass_around(&mut engine, 3);
    assert!(is_draw(&status), "expected a draw, got {status:?}");
    assert!(is_draw(&drain(&mut engine)));
}

#[test]
fn a_castable_instant_keeps_the_game_going() {
    // CR 104.4b
    let mut state = loop_state();
    let shock = ObjectId(50);
    state
        .zone_mut(ZoneKind::Hand, Some(P1))
        .unwrap()
        .push_bottom(shock);
    state.objects.insert(
        shock,
        card_object(
            shock,
            definition("Instant", CardTypeSet::INSTANT, None),
            P1,
            ZoneKind::Hand,
        ),
    );
    let mut engine = Engine::new(state);
    let status = pass_around(&mut engine, LAPS);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "{status:?}"
    );
}

#[test]
fn a_mana_ability_with_a_sacrifice_cost_keeps_the_game_going() {
    // CR 732.5
    let mut state = loop_state();
    let lotus = ObjectId(60);
    put_on_battlefield(
        &mut state,
        card_object(
            lotus,
            definition("Sac Rock", CardTypeSet::ARTIFACT, None),
            P1,
            ZoneKind::Battlefield,
        ),
    );
    state.abilities.push(RegisteredAbility {
        text: None,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec {
            mana: None,
            additional: vec![AdditionalCost::TapSelf, AdditionalCost::SacrificeSelf],
            alternative: Vec::new(),
        }),
        ..ability(
            lotus,
            ConditionPrimitive::Always,
            AbilityEffect::AddMana(vec![ManaUnit::Colorless]),
        )
    });
    let mut engine = Engine::new(state);
    let status = pass_around(&mut engine, LAPS);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "{status:?}"
    );
}

#[test]
fn a_plain_tap_mana_ability_does_not_stop_the_draw() {
    // CR 732.5
    let mut state = loop_state();
    let land = ObjectId(70);
    put_on_battlefield(
        &mut state,
        card_object(
            land,
            definition("Wastes", CardTypeSet::LAND, None),
            P1,
            ZoneKind::Battlefield,
        ),
    );
    state.abilities.push(RegisteredAbility {
        text: None,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec {
            mana: None,
            additional: vec![AdditionalCost::TapSelf],
            alternative: Vec::new(),
        }),
        ..ability(
            land,
            ConditionPrimitive::Always,
            AbilityEffect::AddMana(vec![ManaUnit::Colorless]),
        )
    });
    let mut engine = Engine::new(state);
    assert!(is_draw(&pass_around(&mut engine, 3)));
}

#[test]
fn a_finite_repeat_is_not_a_draw() {
    // CR 104.4b
    // P1's life grows each lap, so no state repeats.
    let mut state = loop_state();
    let altar = ObjectId(80);
    put_on_battlefield(
        &mut state,
        card_object(
            altar,
            definition("Altar", CardTypeSet::ENCHANTMENT, None),
            P1,
            ZoneKind::Battlefield,
        ),
    );
    state.abilities.push(ability(
        altar,
        ConditionPrimitive::DiesMatching(FilterSpec {
            card_types: Some(CardTypeSet::CREATURE),
            supertypes: None,
            subtypes: None,
            controller: None,
            random: false,
            extra: None,
        }),
        AbilityEffect::GainLife(1),
    ));
    let mut engine = Engine::new(state);
    let status = pass_around(&mut engine, LAPS);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "{status:?}"
    );
    assert!(engine.state().players[1].life > 20);
}

#[test]
fn a_rejected_action_leaves_the_watch_alone() {
    // CR 104.4b
    // A refused pass from the wrong player must neither clear nor grow the watch.
    let mut clean = Engine::new(loop_state());
    let mut clean_passes = 0;
    let mut status = drain(&mut clean);
    while let StepStatus::AwaitingAction { player } = status
        && clean_passes < LAPS * 2
    {
        status = pass(&mut clean, player);
        clean_passes += 1;
    }

    let mut noisy = Engine::new(loop_state());
    let mut noisy_passes = 0;
    let mut status = drain(&mut noisy);
    while let StepStatus::AwaitingAction { player } = status
        && noisy_passes < LAPS * 2
    {
        let other = if player == P0 { P1 } else { P0 };
        assert!(
            noisy
                .submit_action(PlayerAction {
                    player: other,
                    kind: PlayerActionKind::PassPriority,
                })
                .is_err()
        );
        status = pass(&mut noisy, player);
        noisy_passes += 1;
    }
    assert!(is_draw(&status));
    assert_eq!(noisy_passes, clean_passes);
}
