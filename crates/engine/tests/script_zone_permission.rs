//! CR 601.3, 305.1: the `Game.CreateZonePermission` Lua round trip into `GameState.zone_permissions`.
//! Command and `legal_actions` enforcement is in zone_permissions.rs.

use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, minimal_card_definition, two_player_state, zones};

const SOURCE: ObjectId = ObjectId(999);

fn state_with_source(script: &str, zone: ZoneKind) -> (GameState, CardDefinition) {
    let mut state = two_player_state(zones(
        PlayerId(0),
        PlayerId(1),
        &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard],
    ));
    let definition = CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            999,
            "Test Crucible",
            "test_crucible",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::ARTIFACT,
        )
    };
    let owner = (zone != ZoneKind::Battlefield).then_some(PlayerId(1));
    state.zone_mut(zone, owner).unwrap().push_bottom(SOURCE);
    state.objects.insert(
        SOURCE,
        card_object(SOURCE, definition.clone(), PlayerId(1), zone),
    );
    (state, definition)
}

fn script(body: &str) -> String {
    format!("local s = {{}}\nfunction s.register_abilities(c)\n{body}\nend\nreturn s")
}

fn run(body: &str, zone: ZoneKind) -> (GameState, Result<(), String>) {
    let (mut state, definition) = state_with_source(&script(body), zone);
    let result = ScriptRuntime::new()
        .run_card_script(&mut state, &definition, SOURCE)
        .map_err(|e| format!("{e:?}"));
    (state, result)
}

fn permission(setters: &str) -> String {
    format!("local p = Game.CreateZonePermission(c)\n{setters}\np:Register()")
}

// CR 305.1
#[test]
fn a_script_registers_a_graveyard_land_permission() {
    let (state, result) = run(
        &permission(
            "p:SetAffected(Game.Affected.You)\np:SetZone(\"Graveyard\")\np:SetKind(\"PlayLand\")",
        ),
        ZoneKind::Battlefield,
    );
    result.expect("the script registers cleanly");
    assert_eq!(
        state.zone_permissions,
        vec![ZonePermission {
            source: SOURCE,
            controller: PlayerId(1),
            affected: FilterController::You,
            zone: ZoneKind::Graveyard,
            owners: FilterController::You,
            kind: ZonePermissionKind::PlayLand,
            cards: FilterSpec::ANY,
        }]
    );
}

// CR 601.3
#[test]
fn set_owner_and_set_filter_narrow_an_exile_cast_permission() {
    let (state, result) = run(
        &permission(
            "p:SetAffected(Game.Affected.Any)\np:SetZone(\"Exile\")\np:SetKind(\"Cast\")\np:SetOwner(Game.Affected.Opponent)\np:SetFilter(Game.Filter({ card_types = { \"Instant\" } }))",
        ),
        ZoneKind::Battlefield,
    );
    result.expect("the script registers cleanly");
    let [permission] = state.zone_permissions.as_slice() else {
        panic!("one permission, got {:?}", state.zone_permissions);
    };
    assert_eq!(permission.zone, ZoneKind::Exile);
    assert_eq!(permission.kind, ZonePermissionKind::Cast);
    assert_eq!(permission.affected, FilterController::Any);
    assert_eq!(permission.owners, FilterController::Opponent);
    assert_eq!(permission.cards.card_types, Some(CardTypeSet::INSTANT));
}

#[test]
fn register_without_a_required_setter_is_an_error() {
    for (setters, missing) in [
        (
            "p:SetZone(\"Graveyard\")\np:SetKind(\"Cast\")",
            "SetAffected",
        ),
        (
            "p:SetAffected(Game.Affected.You)\np:SetKind(\"Cast\")",
            "SetZone",
        ),
        (
            "p:SetAffected(Game.Affected.You)\np:SetZone(\"Graveyard\")",
            "SetKind",
        ),
    ] {
        let (state, result) = run(&permission(setters), ZoneKind::Battlefield);
        let err = result.expect_err("a required setter is missing");
        assert!(err.contains(&format!("no {missing}()")), "{err}");
        assert!(state.zone_permissions.is_empty());
    }
}

#[test]
fn unsupported_zones_kinds_and_the_enchanted_player_are_rejected() {
    for zone in ["Library", "Hand", "Battlefield", "Stack", "Command"] {
        let (_, result) = run(
            &format!("local p = Game.CreateZonePermission(c)\np:SetZone(\"{zone}\")"),
            ZoneKind::Battlefield,
        );
        let err = result.expect_err("only Graveyard and Exile");
        assert!(err.contains("SetZone"), "{zone}: {err}");
    }
    let (_, result) = run(
        "local p = Game.CreateZonePermission(c)\np:SetKind(\"Activate\")",
        ZoneKind::Battlefield,
    );
    assert!(result.expect_err("no such kind").contains("SetKind"));
    for setter in ["SetAffected", "SetOwner"] {
        let (_, result) = run(
            &format!(
                "local p = Game.CreateZonePermission(c)\np:{setter}(Game.Affected.EnchantedPlayer)"
            ),
            ZoneKind::Battlefield,
        );
        assert!(result.is_err(), "{setter}");
    }
}

// CR 113.6
#[test]
fn a_zone_permission_functions_only_from_its_declared_zone() {
    let setters =
        "p:SetAffected(Game.Affected.You)\np:SetZone(\"Graveyard\")\np:SetKind(\"PlayLand\")";
    let (state, result) = run(&permission(setters), ZoneKind::Hand);
    result.expect("the script runs cleanly");
    assert!(state.zone_permissions.is_empty());

    let from_hand = format!("p:SetFunctionsFrom(\"Hand\")\n{setters}");
    let (state, result) = run(&permission(&from_hand), ZoneKind::Hand);
    result.expect("the script runs cleanly");
    assert_eq!(state.zone_permissions.len(), 1);
}
