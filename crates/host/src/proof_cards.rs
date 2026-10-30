//! The proof card set and two 40-card decks, used to play the protocol without a card
//! database. CI relies on it, so nothing here reads `../stackfish-db`. See `docs/DESIGN.md`.

// TODO: add cards that ask for a sacrifice, discard, tap or graveyard-exile payment; a Payment Decision lists no eligible objects

use serde_json::{Value, json};
use stackfish_engine::{
    AbilityEffect, AtomicManaSymbol, CardDefinition, CardTypeSet, Color, KeywordSet, ManaCost, ManaSymbol,
    ModeChoice, PTValue, ScriptReference, SpellMode, Subtype, SupertypeSet, TargetFilter, TargetRequirement,
    TargetSpec,
};

use crate::card_source::FixtureCardSource;

type Deck = &'static [(&'static str, u32)];

const RED_GREEN: Deck = &[
    ("mountain", 9),
    ("forest", 8),
    ("grizzly_bear", 4),
    ("stone_giant", 2),
    ("cinder_duelist", 4),
    ("ember_pinger", 1),
    ("mirror_sage", 1),
    ("shock", 3),
    ("fork_in_the_road", 1),
    ("rampant_growth", 2),
    ("raise_militia", 2),
    ("giant_growth_aura", 1),
    ("spark_stone", 1),
    ("iron_blade", 1),
];

const BLUE_BLACK: Deck = &[
    ("island", 8),
    ("swamp", 9),
    ("wind_drake", 4),
    ("scholar_imp", 3),
    ("grave_rat", 3),
    ("counterspell", 2),
    ("bounce", 2),
    ("divination", 2),
    ("peek", 1),
    ("drain_touch", 2),
    ("doom_blade", 2),
    ("spark_stone", 1),
    ("iron_blade", 1),
];

// `ByKey` would fail to load on every zone change, because the engine does not resolve keys.
const NO_ABILITIES: &str = "local s = {}\nreturn s";

fn cost(text: &str) -> ManaCost {
    let symbols = text
        .split('{')
        .skip(1)
        .map(|part| {
            let atom = part.trim_end_matches('}');
            ManaSymbol::Atom(match atom {
                "W" => AtomicManaSymbol::Colored(Color::White),
                "U" => AtomicManaSymbol::Colored(Color::Blue),
                "B" => AtomicManaSymbol::Colored(Color::Black),
                "R" => AtomicManaSymbol::Colored(Color::Red),
                "G" => AtomicManaSymbol::Colored(Color::Green),
                "C" => AtomicManaSymbol::Colorless,
                generic => AtomicManaSymbol::Generic(generic.parse().expect("a generic cost is a number")),
            })
        })
        .collect();
    ManaCost { symbols }
}

fn base(id: u64, name: &str, mana_cost: &str, card_types: CardTypeSet, oracle_text: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: cost(mana_cost),
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: oracle_text.to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(NO_ABILITIES.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn scripted(mut card: CardDefinition, lua: &str) -> CardDefinition {
    card.script_ref = ScriptReference::Inline(lua.to_string());
    card
}

fn land(id: u64, name: &str, subtype: &str, letter: char) -> CardDefinition {
    let lua = format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.AddMana("{letter}"))
    e:Register()
end
return s
"#
    );
    let card = CardDefinition {
        supertypes: SupertypeSet::BASIC,
        subtypes: vec![Subtype(subtype.to_string())],
        ..base(id, name, "", CardTypeSet::LAND, &format!("{{T}}: Add {{{letter}}}."))
    };
    scripted(card, &lua)
}

fn creature(id: u64, name: &str, mana_cost: &str, power: i32, toughness: i32, text: &str) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        ..base(id, name, mana_cost, CardTypeSet::CREATURE, text)
    }
}

fn with_ability(card: CardDefinition, body: &str) -> CardDefinition {
    scripted(
        card,
        &format!(
            r#"
local s = {{}}
function s.register_abilities(c)
{body}
end
return s
"#
        ),
    )
}

fn spell(card: CardDefinition, target: Option<&str>, operation: &str) -> CardDefinition {
    let target_fn = target.map_or(String::new(), |t| {
        format!("function s.target_requirement()\n    return {t}\nend\n")
    });
    scripted(
        card,
        &format!(
            r#"
local s = {{}}
{target_fn}function s.operation()
    return {operation}
end
return s
"#
        ),
    )
}

fn creature_filter() -> TargetFilter {
    TargetFilter::Object {
        mana_value: None,
        card_types: Some(CardTypeSet::CREATURE),
        excluded_card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone: None,
    }
}

// Declared in Rust because a card script cannot declare modes.
fn fork_in_the_road() -> CardDefinition {
    let damage = SpellMode {
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: creature_filter(),
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        effect: Some(AbilityEffect::Damage(2)),
        additional_cost: None,
        pawprint_weight: 0,
    };
    let life = SpellMode {
        target_requirement: None,
        effect: Some(AbilityEffect::GainLife(3)),
        additional_cost: None,
        pawprint_weight: 0,
    };
    CardDefinition {
        modes: vec![damage, life],
        mode_choice: ModeChoice::Exactly(1),
        ..base(
            17,
            "Fork in the Road",
            "{1}{R}",
            CardTypeSet::INSTANT,
            "Choose one: Fork in the Road deals 2 damage to target creature; or you gain 3 life.",
        )
    }
}

/// Every card in the proof set, keyed by a readable slug rather than an `oracle_id`.
pub fn proof_cards() -> FixtureCardSource {
    let flying = |card: CardDefinition| CardDefinition { keywords: KeywordSet::FLYING, ..card };
    let first_strike = |card: CardDefinition| CardDefinition { keywords: KeywordSet::FIRST_STRIKE, ..card };

    let mountain = land(1, "Mountain", "Mountain", 'R');
    let forest = land(2, "Forest", "Forest", 'G');
    let island = land(3, "Island", "Island", 'U');
    let swamp = land(4, "Swamp", "Swamp", 'B');

    let grizzly_bear = creature(5, "Grizzly Bear", "{1}{G}", 2, 2, "");
    let stone_giant = creature(6, "Stone Giant", "{3}{G}", 4, 4, "");
    let wind_drake = flying(creature(7, "Wind Drake", "{2}{U}", 2, 2, "Flying"));
    let cinder_duelist = first_strike(creature(8, "Cinder Duelist", "{1}{R}", 2, 1, "First strike"));

    let scholar_imp = with_ability(
        creature(9, "Scholar Imp", "{2}{U}", 1, 1, "When Scholar Imp enters the battlefield, draw a card."),
        r#"    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.DrawCards(1))
    e:Register()"#,
    );
    let grave_rat = with_ability(
        creature(10, "Grave Rat", "{1}{B}", 2, 1, "When Grave Rat dies, you gain 3 life."),
        r#"    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Dies)
    e:SetOperation(Game.Ops.GainLife(3))
    e:Register()"#,
    );
    let ember_pinger = with_ability(
        creature(11, "Ember Pinger", "{2}{R}", 1, 1, "{T}: Ember Pinger deals 1 damage to target creature."),
        r#"    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetTarget({ type = "object", card_types = { "Creature" } })
    e:SetOperation(Game.Ops.Damage(1))
    e:Register()"#,
    );
    let mirror_sage = with_ability(
        creature(12, "Mirror Sage", "{2}{G}", 2, 2, "When Mirror Sage enters the battlefield, you may draw a card."),
        r#"    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(function(source, controller)
        if Game.MayChoose() then
            return Game.Ops.DrawCards(1)
        end
        return nil
    end)
    e:Register()"#,
    );

    let instant = CardTypeSet::INSTANT;
    let sorcery = CardTypeSet::SORCERY;
    let shock = spell(
        base(13, "Shock", "{R}", instant, "Shock deals 2 damage to any target."),
        Some(r#"{ type = "any", count = 1 }"#),
        "Game.Ops.Damage(2)",
    );
    let divination = spell(base(14, "Divination", "{2}{U}", sorcery, "Draw two cards."), None, "Game.Ops.DrawCards(2)");
    let counterspell = spell(
        base(15, "Counterspell", "{U}{U}", instant, "Counter target spell."),
        Some(r#"{ zone = "Stack", count = 1 }"#),
        "Game.Ops.Counter",
    );
    let bounce = spell(
        base(16, "Bounce", "{1}{U}", instant, "Return target creature to its owner's hand."),
        Some(r#"{ type = "object", card_types = { "Creature" }, count = 1 }"#),
        "Game.Ops.ReturnToHand",
    );
    let rampant_growth = spell(
        base(18, "Rampant Growth", "{1}{G}", sorcery, "Search your library for a basic land card, put it onto the battlefield tapped, then shuffle."),
        None,
        r#"Game.Ops.Search({ card_types = { "Land" } }, 1, { destination = "Battlefield", tapped = true })"#,
    );
    let raise_militia = spell(
        base(19, "Raise Militia", "{2}{G}", sorcery, "Create a 2/2 green Soldier creature token."),
        None,
        r#"Game.Ops.CreateToken(Game.Tokens.Creature("Soldier", "G", 2, 2))"#,
    );
    let peek = spell(base(20, "Peek", "{U}", sorcery, "Scry 2."), None, "Game.Ops.Scry(2)");
    let drain_touch = spell(
        base(21, "Drain Touch", "{1}{B}", sorcery, "Target player loses 3 life."),
        Some(r#"{ type = "player", count = 1 }"#),
        "Game.Ops.LoseLife(3)",
    );
    let doom_blade = spell(
        base(22, "Doom Blade", "{1}{B}", instant, "Destroy target creature."),
        Some(r#"{ type = "object", card_types = { "Creature" }, count = 1 }"#),
        "Game.Ops.Destroy",
    );

    let giant_growth_aura = scripted(
        CardDefinition {
            subtypes: vec![Subtype("Aura".to_string())],
            ..base(23, "Giant Growth Aura", "{1}{G}", CardTypeSet::ENCHANTMENT, "Enchant creature. Enchanted creature gets +3/+3.")
        },
        r#"
local s = {}
function s.target_requirement()
    return { card_types = { "Creature" }, count = 1 }
end
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(3, 3))
    e:SetAffected(Game.Affected.Enchanted)
    e:Register()
end
return s
"#,
    );
    let spark_stone = with_ability(
        base(24, "Spark Stone", "{2}", CardTypeSet::ARTIFACT, "{T}: Add {C}."),
        r#"    local e = Game.CreateManaAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.AddMana("C"))
    e:Register()"#,
    );
    let iron_blade = with_ability(
        CardDefinition {
            subtypes: vec![Subtype("Equipment".to_string())],
            ..base(25, "Iron Blade", "{1}", CardTypeSet::ARTIFACT, "Equipped creature gets +2/+0. Equip {1}")
        },
        r#"    local bonus = Game.CreateStaticAbility(c)
    bonus:SetLayer(Game.Layers.PowerToughnessModifying)
    bonus:SetEffect(Game.Effects.ModifyPT(2, 0))
    bonus:SetAffected(Game.Affected.Enchanted)
    bonus:Register()

    local equip = Game.CreateActivatedAbility(c)
    equip:AddCost(Game.Costs.Mana("{1}"))
    equip:SetTarget({ type = "object", card_types = { "Creature" }, controller = "you" })
    equip:SetOperation(Game.Ops.Attach)
    equip:Register()"#,
    );

    FixtureCardSource::new()
        .with("mountain", mountain)
        .with("forest", forest)
        .with("island", island)
        .with("swamp", swamp)
        .with("grizzly_bear", grizzly_bear)
        .with("stone_giant", stone_giant)
        .with("wind_drake", wind_drake)
        .with("cinder_duelist", cinder_duelist)
        .with("scholar_imp", scholar_imp)
        .with("grave_rat", grave_rat)
        .with("ember_pinger", ember_pinger)
        .with("mirror_sage", mirror_sage)
        .with("shock", shock)
        .with("divination", divination)
        .with("counterspell", counterspell)
        .with("bounce", bounce)
        .with("fork_in_the_road", fork_in_the_road())
        .with("rampant_growth", rampant_growth)
        .with("raise_militia", raise_militia)
        .with("peek", peek)
        .with("drain_touch", drain_touch)
        .with("doom_blade", doom_blade)
        .with("giant_growth_aura", giant_growth_aura)
        .with("spark_stone", spark_stone)
        .with("iron_blade", iron_blade)
}

fn deck_json(deck: Deck) -> Value {
    let entries: Vec<Value> = deck.iter().map(|(card, count)| json!({ "card": card, "count": count })).collect();
    json!({ "deck": entries })
}

/// A `setup` request for `players` players on the standard preset, alternating the
/// red-green and blue-black decks.
pub fn proof_setup_request(players: usize, seed: u64) -> Vec<u8> {
    let decks: Vec<Value> =
        (0..players).map(|i| deck_json(if i % 2 == 0 { RED_GREEN } else { BLUE_BLACK })).collect();
    let body = json!({ "players": decks, "format": "standard", "seed": seed.to_string() });
    serde_json::to_vec(&json!({ "v": crate::PROTOCOL_VERSION, "kind": "setup", "body": body }))
        .expect("a setup request holds only strings, numbers and lists")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_source::CardSource;

    #[test]
    fn every_deck_entry_is_a_card_in_the_set() {
        let cards = proof_cards();
        for (key, _) in RED_GREEN.iter().chain(BLUE_BLACK) {
            assert!(cards.load(key).is_ok(), "{key} is not in the set");
        }
    }

    #[test]
    fn both_decks_hold_forty_cards() {
        for deck in [RED_GREEN, BLUE_BLACK] {
            assert_eq!(deck.iter().map(|(_, n)| n).sum::<u32>(), 40);
        }
    }
}
