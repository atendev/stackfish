use stackfish_engine::*;

/// A `CardDefinition` with every field at its most inert default
/// (vanilla, no keywords, no target requirement, no effect). Callers
/// override whichever fields their test actually cares about with struct
/// update syntax, e.g.:
/// `CardDefinition { supertypes: SupertypeSet::BASIC, ..testutil::minimal_card_definition(1, "Forest", "forest", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND) }`
pub fn minimal_card_definition(
    definition_id: u64,
    name: impl Into<String>,
    script_key: impl Into<String>,
    mana_cost: ManaCost,
    card_types: CardTypeSet,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.into(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(script_key.into()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}
