//! CR 113.6: `:SetFunctionsFrom("Graveyard"|"Exile")` declares the zone a triggered or activated ability exists in.
//! `Engine::sync_off_battlefield_scripts` runs a card's script once per trigger-collection pass, registers only abilities that declared the zone and discards everything else the script queues. The ability is dropped when the card leaves the zone.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const GRAVEYARD_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Graveyard")
    a:SetOperation(Game.Ops.GainLife(2))
    a:Register()
end
return s
"#;

const EXILE_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Exile")
    a:SetOperation(Game.Ops.GainLife(2))
    a:Register()
end
return s
"#;

const GRAVEYARD_COST_EXILE_OTHER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Graveyard")
    a:AddCost(Game.Costs.ExileFromGraveyard({}, 1))
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

const GRAVEYARD_TRIGGER_AND_ETB: &str = r#"
local s = {}
function s.register_abilities(c)
    local t = Game.CreateTriggeredAbility(c)
    t:SetFunctionsFrom("Graveyard")
    t:SetCondition(Game.Conditions.Always)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()

    local etb = Game.CreateTriggeredAbility(c)
    etb:SetCondition(Game.Conditions.Enters)
    etb:SetOperation(Game.Ops.GainLife(5))
    etb:Register()
end
return s
"#;

const BATTLEFIELD_ONLY_ACTIVATED: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.GainLife(2))
    a:Register()
end
return s
"#;

const BAD_ZONE: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Battlefield")
    a:SetOperation(Game.Ops.GainLife(2))
    a:Register()
end
return s
"#;

fn scripted(script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Scripted", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn state_with(zone: ZoneKind, cards: &[(ObjectId, &str)]) -> GameState {
    let p0 = PlayerId(0);
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            p0,
            PlayerId(1),
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile],
        ))
    };
    for (id, script) in cards {
        state.objects.insert(*id, card_object(*id, scripted(script), p0, zone));
        match zone {
            ZoneKind::Exile | ZoneKind::Battlefield => state.zone_mut(zone, None),
            other => state.zone_mut(other, Some(p0)),
        }
        .expect("zone exists")
        .push_bottom(*id);
    }
    state
}

fn pass_both(engine: &mut Engine) {
    for player in [PlayerId(0), PlayerId(1)] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

fn activate(engine: &mut Engine, source: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player: PlayerId(0),
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("activation is legal");
}

#[test]
fn a_graveyard_declared_ability_registers_for_a_card_in_the_graveyard() {
    let card = ObjectId(0);
    let mut engine = Engine::new(state_with(ZoneKind::Graveyard, &[(card, GRAVEYARD_GAIN_LIFE)]));
    assert!(engine.state().abilities.is_empty(), "nothing has run the script yet");

    pass_both(&mut engine);

    let abilities = &engine.state().abilities;
    assert_eq!(abilities.len(), 1);
    assert_eq!(abilities[0].source, card);
    assert_eq!(engine.state().ability_function_zones.get(&abilities[0].id), Some(&ZoneKind::Graveyard));

    // Another pipeline pass must not register it a second time.
    pass_both(&mut engine);
    assert_eq!(engine.state().abilities.len(), 1);
}

#[test]
fn the_ability_can_be_activated_from_the_graveyard_and_resolves() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut engine = Engine::new(state_with(ZoneKind::Graveyard, &[(card, GRAVEYARD_GAIN_LIFE)]));
    pass_both(&mut engine);
    assert_eq!(engine.state().abilities.len(), 1);

    activate(&mut engine, card);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "even a free cost acknowledges: {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty cost");
    drain(&mut engine);
    pass_both(&mut engine);

    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 22);
}

#[test]
fn an_exile_declared_ability_registers_and_activates_from_exile() {
    let card = ObjectId(0);
    let mut engine = Engine::new(state_with(ZoneKind::Exile, &[(card, EXILE_GAIN_LIFE)]));
    pass_both(&mut engine);

    assert_eq!(engine.state().abilities.len(), 1);
    assert_eq!(
        engine.state().ability_function_zones.get(&engine.state().abilities[0].id),
        Some(&ZoneKind::Exile)
    );
    activate(&mut engine, card);
}

#[test]
fn an_ordinary_etb_trigger_does_not_register_in_the_graveyard() {
    let card = ObjectId(0);
    let mut engine = Engine::new(state_with(ZoneKind::Graveyard, &[(card, GRAVEYARD_TRIGGER_AND_ETB)]));
    pass_both(&mut engine);

    let abilities = &engine.state().abilities;
    assert_eq!(abilities.len(), 1, "only the graveyard-declared trigger registers, not the ETB one");
    assert!(engine.state().ability_function_zones.contains_key(&abilities[0].id));
}

#[test]
fn an_undeclared_ability_does_not_register_in_the_graveyard() {
    let card = ObjectId(0);
    let mut engine = Engine::new(state_with(ZoneKind::Graveyard, &[(card, BATTLEFIELD_ONLY_ACTIVATED)]));
    pass_both(&mut engine);
    assert!(engine.state().abilities.is_empty());
}

#[test]
fn a_declared_ability_does_not_register_off_its_zone() {
    // Run the script as if the card were in hand and then on the battlefield.
    for zone in [ZoneKind::Hand, ZoneKind::Battlefield] {
        let card = ObjectId(0);
        let mut state = state_with(zone, &[(card, GRAVEYARD_GAIN_LIFE)]);
        let definition = state.object(card).unwrap().definition.clone();
        ScriptRuntime::new().run_card_script(&mut state, &definition, card).unwrap();
        assert!(state.abilities.is_empty(), "a Graveyard-only ability must not register in {zone:?}");
        assert!(state.ability_function_zones.is_empty());
    }
}

#[test]
fn leaving_the_graveyard_drops_the_ability() {
    let p0 = PlayerId(0);
    let fodder = ObjectId(0);
    let user = ObjectId(1);
    let mut engine = Engine::new(state_with(
        ZoneKind::Graveyard,
        &[(fodder, GRAVEYARD_GAIN_LIFE), (user, GRAVEYARD_COST_EXILE_OTHER)],
    ));
    pass_both(&mut engine);
    assert_eq!(engine.state().abilities.len(), 2);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: user,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Exile(fodder)] }).expect("exile the other card");
    drain(&mut engine);

    // The fodder card is in exile with a new id: its graveyard ability is gone and did not return, since its script declared only Graveyard.
    assert!(engine.state().object(fodder).is_none());
    let exiled = engine.state().zone(ZoneKind::Exile, None).unwrap().contents.clone();
    assert_eq!(exiled.len(), 1);
    assert!(engine.state().abilities.iter().all(|a| a.source != fodder && a.source != exiled[0]));
    assert_eq!(engine.state().abilities.len(), 1, "only the paying card's own ability remains");
    assert_eq!(engine.state().ability_function_zones.len(), 1);
}

#[test]
fn a_declared_trigger_only_fires_while_its_source_is_in_the_zone() {
    let card = ObjectId(0);
    let mut engine = Engine::new(state_with(ZoneKind::Graveyard, &[(card, GRAVEYARD_TRIGGER_AND_ETB)]));
    pass_both(&mut engine);
    let ability_id = engine.state().abilities[0].id;
    assert_eq!(engine.state().ability_function_zones.get(&ability_id), Some(&ZoneKind::Graveyard));

    // Defensive check in collect_triggers: pretend the card is somewhere else.
    engine.state_mut().object_mut(card).unwrap().zone = ZoneKind::Hand;
    let event = GameEvent {
        event_id: EventId(900),
        sequence_no: SequenceNo(900),
        kind: GameEventKind::TurnStarted { player: PlayerId(0), turn_number: 1 },
        caused_by_command: None,
        parent_event: None,
    };
    let (matches, _) =
        stackfish_engine::triggers::collect_triggers(engine.state(), &event, &ScriptRuntime::new(), &[]);
    assert!(matches.is_empty());
}

#[test]
fn setting_a_battlefield_function_zone_is_a_clear_error() {
    let card = ObjectId(0);
    let mut state = state_with(ZoneKind::Graveyard, &[(card, BAD_ZONE)]);
    let definition = state.object(card).unwrap().definition.clone();
    let result = ScriptRuntime::new().run_card_script(&mut state, &definition, card);
    assert!(result.is_err(), "SetFunctionsFrom only accepts Graveyard/Exile");
    assert!(state.abilities.is_empty());
}
