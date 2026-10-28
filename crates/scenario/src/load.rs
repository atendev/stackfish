use crate::schema::*;
use crate::{card_templates, inline_setup};

/// Parses scenario TOML into a `ScenarioFile`, expanding inline `[[setup]]` card fields and
/// then `[[cards]]` templates. Callers use this and not `toml::from_str` so both expansions run.
pub fn parse(text: &str) -> ScenarioFile {
    let mut root: toml::Table =
        toml::from_str(text).unwrap_or_else(|e| panic!("failed to parse scenario TOML: {e}"));
    // Inline fields first, so a moved `template` still expands.
    inline_setup::expand(&mut root);
    card_templates::expand(&mut root);
    // Round-trips through a string to keep the single `toml::from_str` deserialization path.
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
    // A bare filename gives `Some("")`, which joins to the relative path unchanged.
    file.base_dir = path.parent().map(|p| p.to_path_buf());
    file
}
