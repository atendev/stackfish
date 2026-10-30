//! Mechanical check that `docs/LUA_API.md` stays in sync with the bound `Game.*`
//! constructors in `crates/engine/src/script/globals.rs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[test]
fn lua_api_doc_covers_all_bound_names() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = root.parent().unwrap().parent().unwrap();
    let doc = fs::read_to_string(repo.join("docs/LUA_API.md")).unwrap();
    let source = fs::read_to_string(repo.join("crates/engine/src/script/globals.rs")).unwrap();

    let mut bound = BTreeSet::new();

    // Match `.set("Name", ...)` and `lua.create_table()?; table.set("Name", ...)` style bindings.
    for cap in regex_bare_bindings(&source) {
        bound.insert(cap);
    }

    // Add the table names themselves: Game.Ops, Game.Conditions, etc.
    let tables = [
        "Conditions", "Counter", "Affected", "Filter", "Ops", "Costs", "Layers", "Durations",
        "Effects", "EventFilters", "ReplacementPayloads", "Consumption", "Tokens",
    ];
    for t in tables {
        bound.insert(t.to_string());
    }

    // Builder constructors and helpers.
    let builders = [
        "CreateTriggeredAbility",
        "CreateStaticAbility",
        "CreateActivatedAbility",
        "CreateManaAbility",
        "CreateReplacementEffect",
        "CreateCostModifier",
        "CreateHandSizeModifier",
        "CreateHandRevealModifier",
        "CreateDrawLimitModifier",
        "CreateLandPlayModifier",
        "RegisterKeyword",
        "RegisterWard",
        "SelectTargets",
        "MayChoose",
        "ChooseN",
        "HandSizeEffects",
    ];
    for b in builders {
        bound.insert(b.to_string());
    }

    // Filter out names that are intentionally internal or documented only as a table family.
    let skip: BTreeSet<String> = [
        // Lua-only helper functions not meant for authors.
        "select_targets", "may_choose", "choose_n", "register_keyword", "register_ward",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let mut missing = Vec::new();
    for name in bound {
        if skip.contains(&name) {
            continue;
        }
        if !doc.contains(&name) {
            missing.push(name);
        }
    }

    if !missing.is_empty() {
        panic!(
            "docs/LUA_API.md is missing these bound Game.* names:\n{}\n\
             Add them, or add them to the skip list in lua_api_doc_sync.rs if intentionally undocumented.",
            missing.join("\n")
        );
    }
}

fn regex_bare_bindings(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Simplified regex-free scan for "Name" appearing as a string literal key in a `.set(` call.
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.contains(".set(") {
            continue;
        }
        if let Some(start) = trimmed.find("\"") {
            let after = &trimmed[start + 1..];
            if let Some(end) = after.find("\"") {
                let name = &after[..end];
                // Only PascalCase constructor names are public API entries; skip comments and string error messages.
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !name.starts_with("Game")
                {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}
