//! CR 607.2a: `Game.Ops.ExileLinked` exiles its target and records it in `GameState.exiled_with`, which a `linked` zone permission reads.
//! Permission matching itself is in zone_permissions.rs.

use stackfish_engine::decision::Entry;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SOURCE: ObjectId = ObjectId(1);
const BEAR: ObjectId = ObjectId(2);

const KEEPER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" } })
    a:SetOperation(Game.Ops.ExileLinked)
    a:Register()
    local p = Game.CreateZonePermission(c)
    p:SetAffected(Game.Affected.You)
    p:SetZone("Exile")
    p:SetKind("Cast")
    p:SetOwner(Game.Affected.Any)
    p:SetLinked()
    p:Register()
end
return s
"#;

fn definition(id: u64, name: &str, types: CardTypeSet, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        power: types
            .contains(CardTypeSet::CREATURE)
            .then_some(PTValue::Fixed(2)),
        toughness: types
            .contains(CardTypeSet::CREATURE)
            .then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            id,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn engine() -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    let keeper = definition(1, "Test Keeper", CardTypeSet::ARTIFACT, KEEPER);
    let bear = definition(
        2,
        "Test Bear",
        CardTypeSet::CREATURE,
        "local s = {}\nreturn s",
    );
    for (id, card, owner) in [(SOURCE, keeper.clone(), P0), (BEAR, bear, P1)] {
        let mut object = card_object(id, card, owner, ZoneKind::Battlefield);
        object.controlled_since_turn = 0;
        state.objects.insert(id, object);
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(id);
    }
    ScriptRuntime::new()
        .run_card_script(&mut state, &keeper, SOURCE)
        .expect("script registers");
    Engine::new(state)
}

fn activate(engine: &mut Engine) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: SOURCE,
                ability_index: 0,
                modes: Vec::new(),
                targets: vec![Target::Object(BEAR)],
                chosen_colors: Vec::new(),
                division: Vec::new(),
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

fn exiled_bear(engine: &Engine) -> ObjectId {
    engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Test Bear" && o.zone == ZoneKind::Exile)
        .expect("the bear is in exile")
        .object_id
}

// CR 607.2a
#[test]
fn exile_linked_records_the_card_and_the_linked_permission_casts_it() {
    let mut engine = engine();
    activate(&mut engine);
    pass_both(&mut engine);
    let bear = exiled_bear(&engine);
    assert_eq!(
        engine.state().exiled_with,
        vec![LinkedExile {
            source: SOURCE,
            exiled: bear,
        }]
    );
    assert!(
        engine
            .legal_actions()
            .iter()
            .any(|entry| matches!(entry, Entry::Cast(cast) if cast.card == bear && cast.normal))
    );
}

// CR 607.2a
// The source leaves with the ability on the stack, so the exile still happens but links nothing.
#[test]
fn exile_linked_records_nothing_when_the_source_left_first() {
    let mut engine = engine();
    activate(&mut engine);
    {
        let state = engine.state_mut();
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .retain(|id| *id != SOURCE);
        state
            .zone_mut(ZoneKind::Graveyard, Some(P0))
            .unwrap()
            .push_bottom(SOURCE);
        state.object_mut(SOURCE).unwrap().zone = ZoneKind::Graveyard;
    }
    pass_both(&mut engine);
    exiled_bear(&engine);
    assert!(engine.state().exiled_with.is_empty());
}
