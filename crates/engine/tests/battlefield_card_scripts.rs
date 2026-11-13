//! CR 604.1, 113.6: `Engine::run_battlefield_card_scripts` registers the abilities of permanents
//! a host placed on the battlefield, without treating the placement as an entry.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptError;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

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

const RAISES: &str = r#"
local s = {}
function s.register_abilities(c)
    error("boom")
end
return s
"#;

const BOUNCE_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.ReturnSelfToHand)
    a:Register()
end
return s
"#;

fn card(id: u64, script_ref: ScriptReference) -> CardDefinition {
    CardDefinition {
        script_ref,
        ..minimal_card_definition(
            id,
            format!("Card {id}"),
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::ARTIFACT,
        )
    }
}

fn inline(source: &str) -> ScriptReference {
    ScriptReference::Inline(source.to_string())
}

fn state() -> GameState {
    GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            P0,
            P1,
            &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard],
        ))
    }
}

fn place(state: &mut GameState, id: u64, script_ref: ScriptReference, zone: ZoneKind) {
    let object_id = ObjectId(id);
    state.objects.insert(
        object_id,
        card_object(object_id, card(id, script_ref), P0, zone),
    );
    let owner = (zone != ZoneKind::Battlefield).then_some(P0);
    state.zone_mut(zone, owner).unwrap().push_bottom(object_id);
}

fn abilities_from(engine: &Engine, source: ObjectId) -> Vec<AbilityId> {
    engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == source)
        .map(|a| a.id)
        .collect()
}

// CR 604.1
#[test]
fn runs_battlefield_scripts_in_object_id_order() {
    let mut state = state();
    // The zone lists the higher id first, so zone order and id order disagree.
    place(&mut state, 5, inline(ETB_GAIN_LIFE), ZoneKind::Battlefield);
    place(&mut state, 2, inline(ETB_GAIN_LIFE), ZoneKind::Battlefield);
    let mut engine = Engine::new(state);

    assert!(engine.run_battlefield_card_scripts().is_empty());

    let low = abilities_from(&engine, ObjectId(2));
    let high = abilities_from(&engine, ObjectId(5));
    assert_eq!((low.len(), high.len()), (1, 1));
    assert!(low[0].0 < high[0].0, "{low:?} {high:?}");
}

#[test]
fn skips_by_key_empty_and_off_battlefield_sources() {
    let mut state = state();
    place(
        &mut state,
        1,
        ScriptReference::ByKey("bear".to_string()),
        ZoneKind::Battlefield,
    );
    place(&mut state, 2, inline(""), ZoneKind::Battlefield);
    place(&mut state, 3, inline(ETB_GAIN_LIFE), ZoneKind::Hand);
    place(&mut state, 4, inline(ETB_GAIN_LIFE), ZoneKind::Graveyard);
    state.objects.insert(
        ObjectId(6),
        card_object(
            ObjectId(6),
            card(6, inline(ETB_GAIN_LIFE)),
            P0,
            ZoneKind::Stack,
        ),
    );
    let mut engine = Engine::new(state);

    assert!(engine.run_battlefield_card_scripts().is_empty());
    assert!(engine.state().abilities.is_empty());
}

#[test]
fn returns_load_and_runtime_errors_without_a_ledger_event() {
    let mut state = state();
    place(
        &mut state,
        1,
        inline("this is not lua"),
        ZoneKind::Battlefield,
    );
    place(&mut state, 2, inline(RAISES), ZoneKind::Battlefield);
    place(&mut state, 3, inline(ETB_GAIN_LIFE), ZoneKind::Battlefield);
    let mut engine = Engine::new(state);
    let ledger_before = engine.state().ledger.len();

    let failures = engine.run_battlefield_card_scripts();

    assert_eq!(failures.len(), 2, "{failures:?}");
    assert!(matches!(
        failures[0],
        (ObjectId(1), ScriptError::LoadError(_))
    ));
    assert!(matches!(
        failures[1],
        (ObjectId(2), ScriptError::RuntimeError(_))
    ));
    assert_eq!(abilities_from(&engine, ObjectId(3)).len(), 1);
    assert_eq!(engine.state().ledger.len(), ledger_before);
    assert!(engine.state().off_battlefield_scripts_run.is_empty());
}

// CR 603.6a
#[test]
fn placement_fires_no_enters_trigger() {
    let mut state = state();
    place(&mut state, 1, inline(ETB_GAIN_LIFE), ZoneKind::Battlefield);
    let mut engine = Engine::new(state);
    let life_before = engine.state().player(P0).unwrap().life;

    engine.run_battlefield_card_scripts();
    drain(&mut engine);

    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(P0).unwrap().life, life_before);
}

// CR 400.7
#[test]
fn bounced_setup_card_registers_once_in_hand() {
    let mut state = state();
    place(&mut state, 1, inline(BOUNCE_SELF), ZoneKind::Battlefield);
    let mut engine = Engine::new(state);
    engine.run_battlefield_card_scripts();
    assert_eq!(abilities_from(&engine, ObjectId(1)).len(), 1);

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: ObjectId(1),
                ability_index: 0,
                modes: Vec::new(),
                targets: Vec::new(),
                division: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("p0 holds priority and controls the source");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(&mut engine);
    for player in [P0, P1] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        drain(&mut engine);
    }

    let hand = &engine
        .state()
        .zone(ZoneKind::Hand, Some(P0))
        .unwrap()
        .contents;
    assert_eq!(hand.len(), 1);
    let returned = hand[0];
    assert_ne!(returned, ObjectId(1));
    // The hand sweep registers the new object's undeclared ability; a second run would add a copy.
    assert_eq!(abilities_from(&engine, returned).len(), 1);
}
