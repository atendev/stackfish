//! CR 903.4, 903.5c/d, 702.124c: Commander color identity as pure query functions in `crates/engine/src/color_identity.rs`.
//! Nothing is wired into `Engine::new_game` or any command, since the engine trusts the setup (CR 903.3a).

use stackfish_engine::color_identity::{color_identity, commander_color_identity, illegal_cards_for_color_identity};
use stackfish_engine::*;

fn card(name: &str, mana_cost: &str, oracle_text: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost::parse(mana_cost).unwrap(),
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: oracle_text.to_string(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

#[test]
fn colored_symbols_contribute_their_color() {
    let cost = ManaCost::parse("{2}{R}{R}").unwrap();
    assert_eq!(cost.colors(), ColorSet::RED);
}

#[test]
fn hybrid_symbols_contribute_both_colors() {
    let cost = ManaCost::parse("{R/G}").unwrap();
    assert_eq!(cost.colors(), ColorSet::RED | ColorSet::GREEN);
}

#[test]
fn phyrexian_symbols_count_as_colored() {
    let cost = ManaCost::parse("{U/P}").unwrap();
    assert_eq!(cost.colors(), ColorSet::BLUE);
}

#[test]
fn generic_colorless_and_snow_contribute_no_color() {
    let cost = ManaCost::parse("{3}{C}{S}").unwrap();
    assert_eq!(cost.colors(), ColorSet::empty());
}

#[test]
fn card_colors_come_from_mana_cost_by_default() {
    let bear = card("Bear", "{1}{G}", "");
    assert_eq!(bear.colors(), ColorSet::GREEN);
}

#[test]
fn color_indicator_overrides_mana_cost_colors() {
    let mut vanilla = card("Ghostfire Bear", "{4}", "");
    vanilla.color_indicator = Some(ColorSet::RED);
    assert_eq!(vanilla.colors(), ColorSet::RED);
}

#[test]
fn no_colored_symbols_or_indicator_is_colorless() {
    let sol_ring = card("Sol Ring", "{1}", "");
    assert_eq!(sol_ring.colors(), ColorSet::empty());
}

#[test]
fn bosh_iron_golem_color_identity_is_red_per_the_cr_example() {
    // CR 903.4: {8} with "{3}{R}, Sacrifice an artifact: ..." has red identity.
    let bosh = card(
        "Bosh, Iron Golem",
        "{8}",
        "{3}{R}, Sacrifice an artifact: Bosh, Iron Golem deals damage equal to the sacrificed \
         artifact's mana value to any target.",
    );
    assert_eq!(color_identity(&bosh), ColorSet::RED);
}

#[test]
fn reminder_text_is_excluded_from_color_identity() {
    // The only {W} symbol is inside parentheses, so the identity is colorless (CR 903.4c).
    let vanilla =
        card("Silent Wisp", "{1}", "Flying (Reminder: as if it had {W} in its cost, which it doesn't.)");
    assert_eq!(color_identity(&vanilla), ColorSet::empty());
}

#[test]
fn color_identity_includes_both_mana_cost_and_rules_text_colors() {
    let card = card("Two-Color General", "{1}{W}", "{T}: Add {U}.");
    assert_eq!(color_identity(&card), ColorSet::WHITE | ColorSet::BLUE);
}

#[test]
fn single_commander_color_identity_is_just_its_own() {
    let general = card("General", "{2}{R}", "");
    assert_eq!(commander_color_identity(&[general]), ColorSet::RED);
}

#[test]
fn partner_commanders_color_identity_is_the_union() {
    let red_partner = card("Red Partner", "{1}{R}", "Partner");
    let blue_partner = card("Blue Partner", "{1}{U}", "Partner");
    assert_eq!(commander_color_identity(&[red_partner, blue_partner]), ColorSet::RED | ColorSet::BLUE);
}

#[test]
fn wort_deck_legality_matches_the_cr_example() {
    // CR 903.5c: Wort, the Raidmother ({4}{R/G}{R/G}) has red/green identity, so a red-only card is legal and a blue card is not.
    let wort = card("Wort, the Raidmother", "{4}{R/G}{R/G}", "");
    let identity = commander_color_identity(&[wort]);

    let red_card = card("Shock", "{R}", "");
    let blue_card = card("Counterspell", "{U}{U}", "");
    let colorless_card = card("Sol Ring", "{1}", "");

    let cards = [red_card, blue_card, colorless_card];
    let illegal = illegal_cards_for_color_identity(identity, &cards);
    assert_eq!(illegal.len(), 1);
    assert_eq!(illegal[0].name, "Counterspell");
}

#[test]
fn basic_land_legality_falls_out_of_the_general_rule() {
    // CR 903.5d: a basic land's tap ability text carries the color symbol like any rules text, so no land-specific check exists.
    let forest = card("Forest", "{0}", "{T}: Add {G}.");
    let mountain = card("Mountain", "{0}", "{T}: Add {R}.");

    let green_commander = commander_color_identity(&[card("Green General", "{2}{G}", "")]);
    assert!(illegal_cards_for_color_identity(green_commander, &[forest.clone()]).is_empty());
    assert_eq!(illegal_cards_for_color_identity(green_commander, &[mountain]).len(), 1);

    let blue_commander = commander_color_identity(&[card("Blue General", "{2}{U}", "")]);
    assert_eq!(illegal_cards_for_color_identity(blue_commander, &[forest]).len(), 1);
}
