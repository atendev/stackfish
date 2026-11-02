//! The bot pays sacrifice, discard, tap-permanents and graveyard-exile costs in whole games
//! over bytes. Each paying ability makes a player lose a distinct amount of life, so events show it.

// TODO: madness and cascade have no Lua constructor a fixture card can use, so no byte-level game offers a may_cast Decision

use serde_json::json;
use stackfish_engine::{
    AdditionalCost, AlternativeCost, CardDefinition, CardTypeSet, CostSpec, FilterSpec, ManaCost,
    PTValue, ScriptReference, Subtype, SupertypeSet, ZoneKind,
};
use stackfish_host::{Bot, FixtureCardSource, PROTOCOL_VERSION, Session};
use stackfish_testutil::minimal_card_definition;

const ANSWER_LIMIT: usize = 50_000;

fn mana(cost: &str) -> ManaCost {
    ManaCost::parse(cost).unwrap_or_else(|e| panic!("{cost}: {e:?}"))
}

fn land(id: u64, name: &str, letter: &str) -> CardDefinition {
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
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        subtypes: vec![Subtype(name.to_string())],
        script_ref: ScriptReference::Inline(lua),
        ..minimal_card_definition(id, name, name, mana(""), CardTypeSet::LAND)
    }
}

/// A 2/2 whose activated ability has `costs` and makes the target player lose `loss` life.
fn outlet(id: u64, name: &str, cost: &str, costs: &str, loss: u32) -> CardDefinition {
    let lua = format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
{costs}
    e:SetTarget({{ type = "player", count = 1 }})
    e:SetOperation(Game.Ops.LoseLife({loss}))
    e:Register()
end
return s
"#
    );
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: ScriptReference::Inline(lua),
        ..minimal_card_definition(id, name, name, mana(cost), CardTypeSet::CREATURE)
    }
}

fn bear(id: u64) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: ScriptReference::Inline("local s = {}\nreturn s".to_string()),
        ..minimal_card_definition(id, "Bear", "Bear", mana("{1}{G}"), CardTypeSet::CREATURE)
    }
}

// CR 702.138a
fn escape_bolt(id: u64) -> CardDefinition {
    let lua = r#"
local s = {}
function s.target_requirement()
    return { type = "player", count = 1 }
end
function s.operation()
    return Game.Ops.LoseLife(4)
end
return s
"#;
    let any = FilterSpec {
        card_types: None,
        supertypes: None,
        subtypes: None,
        controller: None,
        random: false,
        extra: None,
    };
    CardDefinition {
        script_ref: ScriptReference::Inline(lua.to_string()),
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Escape {R}, exile two other cards".to_string(),
            cost: CostSpec {
                mana: Some(mana("{R}")),
                additional: vec![AdditionalCost::ExileFromGraveyard {
                    filter: any,
                    count: 2,
                }],
                alternative: Vec::new(),
            },
            from_zone: ZoneKind::Graveyard,
            exile_on_leaving_stack: false,
            escape: true,
        }],
        ..minimal_card_definition(
            id,
            "Escape Bolt",
            "Escape Bolt",
            mana("{1}{R}"),
            CardTypeSet::SORCERY,
        )
    }
}

const DECK: [(&str, u32); 7] = [
    ("Mountain", 9),
    ("Forest", 8),
    ("Bear", 6),
    ("Sac Outlet", 4),
    ("Discard Outlet", 4),
    ("Tapper", 4),
    ("Escape Bolt", 5),
];

fn cards() -> FixtureCardSource {
    FixtureCardSource::new()
        .with("Mountain", land(1, "Mountain", "R"))
        .with("Forest", land(2, "Forest", "G"))
        .with("Bear", bear(3))
        .with(
            "Sac Outlet",
            outlet(
                4,
                "Sac Outlet",
                "{1}{R}",
                r#"    e:AddCost(Game.Costs.Sacrifice({ card_types = { "Creature" } }))"#,
                1,
            ),
        )
        .with(
            "Discard Outlet",
            outlet(
                5,
                "Discard Outlet",
                "{1}{G}",
                "    e:AddCost(Game.Costs.Mana(\"{1}\"))\n    e:AddCost(Game.Costs.Discard({}))",
                2,
            ),
        )
        .with(
            "Tapper",
            outlet(
                6,
                "Tapper",
                "{1}{G}",
                r#"    e:AddCost(Game.Costs.TapPermanents({ card_types = { "Creature" }, controller = "you" }, 2))"#,
                3,
            ),
        )
        .with("Escape Bolt", escape_bolt(7))
}

fn setup(seed: u64) -> Vec<u8> {
    let deck: Vec<_> = DECK
        .iter()
        .map(|(card, count)| json!({ "card": card, "count": count }))
        .collect();
    let body = json!({ "players": [{ "deck": deck }, { "deck": deck }], "format": "standard", "seed": seed.to_string() });
    serde_json::to_vec(&json!({ "v": PROTOCOL_VERSION, "kind": "setup", "body": body }))
        .expect("setup is JSON")
}

#[test]
fn the_deck_holds_forty_cards() {
    assert_eq!(DECK.iter().map(|(_, n)| n).sum::<u32>(), 40);
}

#[test]
fn a_bot_pays_object_costs_through_whole_games() {
    let source = cards();
    // Life lost by amount: 1 sacrifice, 2 discard, 3 tap, 4 the escape bolt.
    let mut losses = [0usize; 5];
    let mut exiled_from_graveyard = 0;

    for seed in 0..30 {
        let (mut session, _) = Session::create(&setup(seed), &source)
            .unwrap_or_else(|e| panic!("the setup is valid: {}", String::from_utf8_lossy(&e)));
        let mut bot = Bot::new(seed).recording(&[]);
        let outcome = bot
            .play(&mut session, ANSWER_LIMIT)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(
            outcome.rejected, 0,
            "seed {seed}: the Session rejected a bot Answer"
        );

        for event in bot.events(0) {
            assert_ne!(event["kind"], "script_error", "seed {seed}: {event}");
            match event["kind"].as_str() {
                Some("life_changed") => {
                    if let Some(loss) = event["delta"].as_i64().map(|d| -d)
                        && (1..=4).contains(&loss)
                    {
                        losses[loss as usize] += 1;
                    }
                }
                Some("zone_change") if event["from"] == "graveyard" && event["to"] == "exile" => {
                    exiled_from_graveyard += 1;
                }
                _ => {}
            }
        }
    }

    for (loss, cost) in [(1, "sacrifice"), (2, "discard"), (3, "tap"), (4, "escape")] {
        assert!(
            losses[loss] > 0,
            "no {cost} ability resolved in 30 games: {losses:?}"
        );
    }
    assert!(
        exiled_from_graveyard > 0,
        "no escape cost exiled a graveyard card"
    );
}

#[test]
fn the_same_seed_gives_the_same_game() {
    let source = cards();
    let hash = |seed: u64| {
        let (mut session, _) = Session::create(&setup(seed), &source).expect("valid setup");
        Bot::new(seed)
            .play(&mut session, ANSWER_LIMIT)
            .expect("the game ends")
            .transcript_hash
    };
    assert_eq!(hash(5), hash(5));
}
