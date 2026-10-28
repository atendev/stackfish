use crate::schema::*;
use crate::{card_templates, inline_setup};

/// Parses scenario TOML text into a `ScenarioFile`, expanding any inline
/// `[[setup]]` card fields into synthesized `[[cards]]` entries
/// (`inline_setup::expand`) and then any `[[cards]]` entry's `template`
/// field (`card_templates::expand` - see that module for what a template is
/// and where they're defined), in that order. The canonical entry point
/// every text-based caller (`load` below, `run_db_tests`'s per-row parse)
/// should use instead of calling `toml::from_str::<ScenarioFile>` directly,
/// so neither expansion is something each caller has to remember to opt
/// into separately.
pub fn parse(text: &str) -> ScenarioFile {
    let mut root: toml::Table =
        toml::from_str(text).unwrap_or_else(|e| panic!("failed to parse scenario TOML: {e}"));
    // Inline `[[setup]]` card fields first, so a `template` moved over into
    // a synthesized `[[cards]]` entry still gets expanded normally right
    // after.
    inline_setup::expand(&mut root);
    card_templates::expand(&mut root);
    // Round-trips through a string rather than a direct `Table -> ScenarioFile`
    // conversion - keeps this on the same well-worn `toml::from_str` path
    // every other TOML parse in this crate already uses, instead of a second,
    // less-exercised deserialization route.
    let expanded_text = toml::to_string(&root).unwrap_or_else(|e| {
        panic!("failed to re-serialize scenario TOML after template expansion: {e}")
    });
    toml::from_str(&expanded_text)
        .unwrap_or_else(|e| panic!("failed to parse scenario file after template expansion: {e}"))
}

pub fn load(path: &std::path::Path) -> ScenarioFile {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read scenario file {}: {e}", path.display()));
    let mut file = parse(&text);
    // `path.parent()` is `Some("")` for a bare filename with no directory
    // component (e.g. `load(Path::new("foo.toml"))`) - `PathBuf::from("")`
    // joined against a relative `card_db` below is a harmless no-op
    // (`"".join("cards.sqlite")` is just `"cards.sqlite"`), so this doesn't
    // need special-casing.
    file.base_dir = path.parent().map(|p| p.to_path_buf());
    file
}
