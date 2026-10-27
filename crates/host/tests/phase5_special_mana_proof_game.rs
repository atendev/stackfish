//! Phase 5 vertical slice: the bot pays hybrid, Phyrexian, `{C}`, `{S}` and
//! `{X}` costs in whole games over the byte interface. The deck is vanilla
//! creatures with those costs and lands that produce what they need, so the
//! test needs no card database and no card scripts beyond the lands.

use std::collections::BTreeSet;

use serde_json::json;
use stackfish_engine::{CardDefinition, CardTypeSet, ManaCost, PTValue, ScriptReference, Subtype, SupertypeSet};
use stackfish_host::{Bot, FixtureCardSource, PROTOCOL_VERSION, Session};
use stackfish_testutil::minimal_card_definition;

const ANSWER_LIMIT: usize = 50_000;

const NO_ABILITIES: &str = "local s = {}\nreturn s";

fn creature(id: u64, name: &str, cost: &str) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: ScriptReference::Inline(NO_ABILITIES.to_string()),
        ..minimal_card_definition(
            id,
            name,
            name,
            ManaCost::parse(cost).unwrap_or_else(|e| panic!("{name}: {e:?}")),
            CardTypeSet::CREATURE,
        )
    }
}

fn land(id: u64, name: &str, letter: &str, snow: bool) -> CardDefinition {
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
    let supertypes = if snow { SupertypeSet::SNOW | SupertypeSet::BASIC } else { SupertypeSet::BASIC };
    CardDefinition {
        supertypes,
        subtypes: vec![Subtype(name.to_string())],
        script_ref: ScriptReference::Inline(lua),
        ..minimal_card_definition(id, name, name, ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

/// The nine creatures, by name and cost, and the number of copies of each.
const CREATURES: [(&str, &str, u32); 9] = [
    ("Hybrid Bear", "{1}{R/G}", 3),
    ("Twofold Goblin", "{2/R}{2/G}", 3),
    ("Phyrexian Raider", "{1}{R/P}{R/P}", 3),
    ("Hybrid Phyrexian Elf", "{1}{G/R/P}", 2),
    ("Eldrazi Drone", "{2}{C}", 3),
    ("Wastes Wurm", "{C}{C}", 2),
    ("Snow Wolf", "{S}{S}", 3),
    ("Frost Giant", "{1}{S}{G}", 2),
    ("Growing Hydra", "{X}{G}", 2),
];

fn cards() -> FixtureCardSource {
    let mut source = FixtureCardSource::new()
        .with("Snow Forest", land(1, "Forest", "G", true))
        .with("Snow Mountain", land(2, "Mountain", "R", true))
        .with("Wastes", land(3, "Wastes", "C", false));
    for (i, (name, cost, _)) in CREATURES.iter().enumerate() {
        source.insert(*name, creature(10 + i as u64, name, cost));
    }
    source
}

fn setup(seed: u64) -> Vec<u8> {
    let mut deck = vec![
        json!({ "card": "Snow Forest", "count": 6 }),
        json!({ "card": "Snow Mountain", "count": 6 }),
        json!({ "card": "Wastes", "count": 5 }),
    ];
    deck.extend(CREATURES.iter().map(|(name, _, count)| json!({ "card": name, "count": count })));
    let body = json!({ "players": [{ "deck": deck }, { "deck": deck }], "format": "standard", "seed": seed.to_string() });
    serde_json::to_vec(&json!({ "v": PROTOCOL_VERSION, "kind": "setup", "body": body })).expect("setup is JSON")
}

#[test]
fn the_deck_holds_forty_cards() {
    let creatures: u32 = CREATURES.iter().map(|(_, _, n)| n).sum();
    assert_eq!(6 + 6 + 5 + creatures, 40);
}

#[test]
fn a_bot_pays_special_mana_costs_through_whole_games() {
    let source = cards();
    let mut entered = BTreeSet::new();
    let mut snow_mana = 0;
    let mut paid_with_life = 0;

    for seed in 0..30 {
        let (mut session, _) = Session::create(&setup(seed), &source)
            .unwrap_or_else(|e| panic!("the setup is valid: {}", String::from_utf8_lossy(&e)));
        let mut bot = Bot::new(seed).recording(&[]);
        let outcome = bot.play(&mut session, ANSWER_LIMIT).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(outcome.rejected, 0, "seed {seed}: the Session rejected a bot Answer");

        for seat in 0..2 {
            for event in bot.events(seat) {
                assert_ne!(event["kind"], "script_error", "seed {seed}: {event}");
                match event["kind"].as_str() {
                    Some("zone_change") if event["to"] == "battlefield" => {
                        if let Some(name) = event["identity"]["name"].as_str() {
                            entered.insert(name.to_string());
                        }
                    }
                    Some("mana_added") => {
                        snow_mana += event["units"].as_array().into_iter().flatten().filter(|u| u["kind"] == "snow_colored").count();
                    }
                    Some("life_changed") if event["delta"] == -2 => paid_with_life += 1,
                    _ => {}
                }
            }
        }
    }

    for (name, cost, _) in CREATURES {
        assert!(entered.contains(name), "{name} {cost} never entered the battlefield in 30 games: {entered:?}");
    }
    assert!(snow_mana > 0, "no snow mana was produced");
    assert!(paid_with_life > 0, "no 2 life payment was seen");
}

#[test]
fn the_same_seed_gives_the_same_game() {
    let source = cards();
    let hash = |seed: u64| {
        let (mut session, _) = Session::create(&setup(seed), &source).expect("valid setup");
        Bot::new(seed).play(&mut session, ANSWER_LIMIT).expect("the game ends").transcript_hash
    };
    assert_eq!(hash(5), hash(5));
}
