//! An SBA pass touching several objects must do so in the same order every run, since `GameState::objects` is a `HashMap`. An unsorted collector would write different ledgers for one seed.

use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, minimal_card_definition, two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const COUNT: u64 = 24;

fn equipment_definition(id: u64) -> CardDefinition {
    CardDefinition {
        subtypes: vec![Subtype("Equipment".to_string())],
        ..minimal_card_definition(id, "Sword", "sword", ManaCost { symbols: Vec::new() }, CardTypeSet::ARTIFACT)
    }
}

#[test]
fn equipment_that_falls_off_does_so_in_object_id_order() {
    let mut state = two_player_state(zones(P0, PlayerId(1), &[ZoneKind::Battlefield]));
    let land = ObjectId(5000);
    let mut land_object = card_object(
        land,
        minimal_card_definition(1, "Forest", "forest", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND),
        P0,
        ZoneKind::Battlefield,
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(land);
    for i in 0..COUNT {
        let id = ObjectId(100 + i * 7);
        let mut sword = card_object(id, equipment_definition(10 + i), P0, ZoneKind::Battlefield);
        sword.attached_to = Some(land);
        land_object.attachments.push(id);
        state.objects.insert(id, sword);
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    }
    state.objects.insert(land, land_object);

    let SbaOutcome::Progressed(events, _) = check_state_based_actions(&mut state, &ScriptRuntime::new()) else {
        panic!("expected a pass that finishes");
    };
    let unattached: Vec<ObjectId> = events
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::Unattached { object, .. } => Some(object),
            _ => None,
        })
        .collect();
    assert_eq!(unattached.len(), COUNT as usize);
    let mut sorted = unattached.clone();
    sorted.sort();
    assert_eq!(unattached, sorted, "Unattached events must follow object id order");
}
