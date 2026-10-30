//! Every active script row in the sibling `stackfish-db` database loads and self-registers
//! without a Lua error under the strict API. Load-only: mechanics are not checked.
//! The tests that read the database skip when it is absent or unpulled.

use rusqlite::{Connection, OpenFlags};
use stackfish_engine::{
    ManaCost, ObjectId, PlayerId, ScriptReference, ZoneKind, script::ScriptRuntime,
};
use stackfish_testutil::{
    card_object, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state,
};
use std::path::{Path, PathBuf};

struct ActiveScript {
    script_id: String,
    oracle_id: String,
    source: String,
}

fn stackfish_db_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../stackfish-db")
}

/// Active rows only: an inactive version is history and may still use an older API.
/// `None` when the database is absent or is a Git LFS pointer that was never pulled.
fn active_scripts(dir: &Path) -> Option<Vec<ActiveScript>> {
    let path = dir.join("stackfish.db");
    let mut header = [0u8; 16];
    let mut file = std::fs::File::open(&path).ok()?;
    std::io::Read::read_exact(&mut file, &mut header).ok()?;
    if &header != b"SQLite format 3\0" {
        return None;
    }
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open stackfish.db read-only");
    // LEFT JOIN so an active row with no script id fails here and is not hidden from the load test.
    let mut stmt = conn
        .prepare(
            "SELECT ids.script_id, s.oracle_id, s.script_source \
             FROM card_scripts s \
             LEFT JOIN card_script_ids ids ON ids.oracle_id = s.oracle_id \
             WHERE s.is_active = 1 AND s.language = 'lua' ORDER BY ids.script_id",
        )
        .expect("prepare active script query");
    let rows = stmt
        .query_map([], |row| {
            let oracle_id: String = row.get(1)?;
            let script_id: Option<String> = row.get(0)?;
            Ok(ActiveScript {
                script_id: script_id.unwrap_or_else(|| {
                    panic!("active card_scripts row for oracle_id {oracle_id} has no card_script_ids entry")
                }),
                oracle_id,
                source: row.get(2)?,
            })
        })
        .expect("query active scripts")
        .collect::<Result<Vec<_>, _>>()
        .expect("read active script rows");
    Some(rows)
}

#[test]
fn all_stackfish_db_active_scripts_load() {
    let dir = stackfish_db_dir();
    let Some(scripts) = active_scripts(&dir) else {
        eprintln!("skipping all_stackfish_db_active_scripts_load: database not in {dir:?}");
        return;
    };
    assert!(!scripts.is_empty(), "stackfish.db has no active scripts");

    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    for script in scripts {
        let mut definition = minimal_card_definition(
            0,
            &script.script_id,
            "",
            ManaCost {
                symbols: Vec::new(),
            },
            stackfish_engine::CardTypeSet::empty(),
        );
        definition.script_ref = ScriptReference::Inline(script.source.clone());

        let mut state = two_player_state(hand_battlefield_graveyard_zones(p0, p1));
        let source_id = ObjectId(0);
        state.objects.insert(
            source_id,
            card_object(source_id, definition.clone(), p0, ZoneKind::Battlefield),
        );
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(source_id);

        let mut runtime = ScriptRuntime::new();
        runtime
            .run_card_script(&mut state, &definition, source_id)
            .unwrap_or_else(|e| {
                panic!(
                    "script {} (oracle_id {}) failed to load: {e:?}",
                    script.script_id, script.oracle_id
                )
            });
    }
}

// Trimmed and LF-normalized so an editor newline or a CRLF checkout is not reported as drift.
fn normalized(source: &str) -> String {
    source.replace("\r\n", "\n").trim().to_string()
}

// The engine loads database rows, but scripts are edited as files, so a row that differs
// from its file is an edit that was never synced.
#[test]
fn active_db_scripts_match_their_files() {
    let dir = stackfish_db_dir();
    let scripts_dir = dir.join("scripts");
    let Some(scripts) = active_scripts(&dir) else {
        eprintln!("skipping active_db_scripts_match_their_files: database not in {dir:?}");
        return;
    };
    assert!(
        scripts_dir.is_dir(),
        "database found but {scripts_dir:?} is missing"
    );

    assert!(!scripts.is_empty(), "stackfish.db has no active scripts");

    let mut missing = Vec::new();
    let mut drifted = Vec::new();
    for script in &scripts {
        let file = scripts_dir.join(format!("m{}.lua", script.script_id));
        match std::fs::read_to_string(&file) {
            Ok(on_disk) if normalized(&on_disk) != normalized(&script.source) => {
                drifted.push(script.script_id.clone());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(script.script_id.clone());
            }
            Err(e) => panic!("cannot read {file:?}: {e}"),
        }
    }
    assert!(
        missing.is_empty(),
        "no scripts/m<script_id>.lua file for: {missing:?}"
    );
    assert!(
        drifted.is_empty(),
        "database rows differ from scripts/: {drifted:?}"
    );
}

// A checkout without the database, or with an unpulled LFS pointer, must skip and not panic.
#[test]
fn unusable_database_is_treated_as_absent() {
    let dir = std::env::temp_dir().join(format!("stackfish-db-skip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(active_scripts(&dir).is_none());

    std::fs::write(
        dir.join("stackfish.db"),
        "version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 1\n",
    )
    .unwrap();
    assert!(active_scripts(&dir).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}
