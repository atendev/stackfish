//! CR 601.3b: the `Game.CreateTimingPermission` Lua round trip into `GameState.timing_permissions`.
//! Cast and `legal_actions` enforcement is in timing_permissions.rs.

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
            "Test Orrery",
            "test_orrery",
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

// CR 601.3b
#[test]
fn a_script_registers_a_permission_for_every_spell() {
    let (state, result) = run(
        "local p = Game.CreateTimingPermission(c)\np:SetAffected(Game.Affected.You)\np:Register()",
        ZoneKind::Battlefield,
    );
    result.expect("the script registers cleanly");
    assert_eq!(
        state.timing_permissions,
        vec![TimingPermission {
            source: SOURCE,
            controller: PlayerId(1),
            affected: FilterController::You,
            spells: FilterSpec::ANY,
        }]
    );
}

// CR 601.3b
#[test]
fn set_filter_narrows_the_permissions_spells() {
    let (state, result) = run(
        "local p = Game.CreateTimingPermission(c)\np:SetAffected(Game.Affected.Any)\np:SetFilter(Game.Filter({ card_types = { \"Creature\" } }))\np:Register()",
        ZoneKind::Battlefield,
    );
    result.expect("the script registers cleanly");
    assert_eq!(state.timing_permissions.len(), 1);
    assert_eq!(
        state.timing_permissions[0].spells.card_types,
        Some(CardTypeSet::CREATURE)
    );
    assert_eq!(state.timing_permissions[0].affected, FilterController::Any);
}

#[test]
fn register_without_set_affected_is_an_error() {
    let (state, result) = run(
        "local p = Game.CreateTimingPermission(c)\np:Register()",
        ZoneKind::Battlefield,
    );
    let err = result.expect_err("SetAffected is required");
    assert!(err.contains("no SetAffected()"), "{err}");
    assert!(state.timing_permissions.is_empty());
}

#[test]
fn the_enchanted_player_and_a_plain_table_filter_are_rejected() {
    let (_, result) = run(
        "local p = Game.CreateTimingPermission(c)\np:SetAffected(Game.Affected.EnchantedPlayer)",
        ZoneKind::Battlefield,
    );
    assert!(result.is_err());
    let (_, result) = run(
        "local p = Game.CreateTimingPermission(c)\np:SetFilter({ card_types = { \"Creature\" } })",
        ZoneKind::Battlefield,
    );
    let err = result.expect_err("SetFilter takes Game.Filter only");
    assert!(err.contains("Game.Filter"), "{err}");
}

// CR 113.6
#[test]
fn a_permission_functions_only_from_its_declared_zone() {
    let body =
        "local p = Game.CreateTimingPermission(c)\np:SetAffected(Game.Affected.You)\np:Register()";
    let (state, result) = run(body, ZoneKind::Hand);
    result.expect("the script runs cleanly");
    assert!(state.timing_permissions.is_empty());

    let from_hand = "local p = Game.CreateTimingPermission(c)\np:SetFunctionsFrom(\"Hand\")\np:SetAffected(Game.Affected.You)\np:Register()";
    let (state, result) = run(from_hand, ZoneKind::Hand);
    result.expect("the script runs cleanly");
    assert_eq!(state.timing_permissions.len(), 1);
}
