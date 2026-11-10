use stackfish_engine::*;

/// A vanilla `CardDefinition` with no keywords, target requirement or effect.
/// Override fields with struct update syntax: `CardDefinition { supertypes: SupertypeSet::BASIC, ..testutil::minimal_card_definition(..) }`
pub fn minimal_card_definition(
    definition_id: u64,
    name: impl Into<String>,
    script_key: impl Into<String>,
    mana_cost: ManaCost,
    card_types: CardTypeSet,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        oracle_id: None,
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// A vanilla 2/2 creature named `name` with definition id 1 and script key `bear`.
/// Tests that only need some creature on the battlefield share it.
pub fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            1,
            name,
            "bear",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}
