//! Every active Lua script in the sibling `stackfish-db` project loads and
//! self-registers without a Lua error. This is a smoke test only: it does not
//! check mechanical behavior, only that the strict Lua API accepts the
//! real-world scripts after naming migrations.

use stackfish_engine::{script::ScriptRuntime, ManaCost, ObjectId, PlayerId, ScriptReference, ZoneKind};
use stackfish_testutil::{
    card_object, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state,
};
use std::path::PathBuf;

fn db_scripts_dir() -> PathBuf {
    let manifest = std::env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("../../../stackfish-db/scripts")
        .canonicalize()
        .expect("stackfish-db/scripts exists relative to the engine crate")
}

#[test]
fn all_stackfish_db_active_scripts_load() {
    let scripts_dir = db_scripts_dir();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&scripts_dir)
        .expect("read scripts dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("lua"))
        .collect();
    assert!(!entries.is_empty(), "expected active lua scripts in {scripts_dir:?}");
    entries.sort();

    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    for path in entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).expect("read script");

        let mut definition = minimal_card_definition(0, &name, "", ManaCost { symbols: Vec::new() }, stackfish_engine::CardTypeSet::empty());
        definition.script_ref = ScriptReference::Inline(source);

        let mut state = two_player_state(hand_battlefield_graveyard_zones(p0, p1));
        let source_id = ObjectId(0);
        state
            .objects
            .insert(source_id, card_object(source_id, definition.clone(), p0, ZoneKind::Battlefield));
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source_id);

        let mut runtime = ScriptRuntime::new();
        runtime
            .run_card_script(&mut state, &definition, source_id)
            .unwrap_or_else(|e| panic!("{name} failed to load from {path:?}: {e:?}"));
    }
}
