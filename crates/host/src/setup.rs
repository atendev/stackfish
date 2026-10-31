//! The `setup` message and the checks that turn it into a `GameConfig`. Only structure is
//! validated; deck legality stays with the deck builder. See `docs/DESIGN.md`.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};
use stackfish_engine::{CardDefinition, GameConfig, GameSettings, PlayerConfig, PlayerId};

use crate::card_source::CardSource;
use crate::wire::U64;

/// Engine steps one `advance` or `answer` call may run when the setup sets no `step_cap`.
/// It only stops a runaway from freezing the host.
// TODO: loop detection; the cap does not detect a draw. CR 104.4b
pub const DEFAULT_STEP_CAP: u64 = 100_000;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeckEntry {
    /// The card's `oracle_id` UUID.
    pub card: String,
    pub count: u32,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlayerSetup {
    pub deck: Vec<DeckEntry>,
    /// `oracle_id` UUIDs for the command zone. Empty outside Commander.
    #[serde(default)]
    pub commanders: Vec<String>,
}

fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Individual `GameSettings` fields that replace the format preset's value.
/// `max_mulligans` takes `null` for "no limit".
#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingOverrides {
    pub shuffle_libraries: Option<bool>,
    pub starting_hand_size: Option<u8>,
    pub skip_mulligan: Option<bool>,
    pub free_mulligans: Option<u32>,
    #[serde(default, deserialize_with = "double_option")]
    pub max_mulligans: Option<Option<u32>>,
    pub starting_life: Option<u32>,
    pub poison_loss_threshold: Option<u32>,
    pub max_hand_size: Option<u32>,
}

impl SettingOverrides {
    fn apply(&self, settings: &mut GameSettings) {
        if let Some(v) = self.shuffle_libraries {
            settings.shuffle_libraries = v;
        }
        if let Some(v) = self.starting_hand_size {
            settings.starting_hand_size = v;
        }
        if let Some(v) = self.skip_mulligan {
            settings.skip_mulligan = v;
        }
        if let Some(v) = self.free_mulligans {
            settings.free_mulligans = v;
        }
        if let Some(v) = self.max_mulligans {
            settings.max_mulligans = v;
        }
        if let Some(v) = self.starting_life {
            settings.starting_life = v;
        }
        if let Some(v) = self.poison_loss_threshold {
            settings.poison_loss_threshold = v;
        }
        if let Some(v) = self.max_hand_size {
            settings.max_hand_size = v;
        }
    }
}

/// A Seat tied to no player. `omniscient` shows every hand and library, as a replay viewer
/// needs; only the host sets it, and a player Seat never has it.
#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpectatorSetup {
    #[serde(default)]
    pub omniscient: bool,
}

/// The body of a `setup` request. `players` is in turn order, and player `i` gets
/// `PlayerId(i)` and Seat `i`. Spectator Seats follow the player Seats. CR 103.1
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetupBody {
    pub players: Vec<PlayerSetup>,
    #[serde(default)]
    pub spectators: Vec<SpectatorSetup>,
    pub format: String,
    #[serde(default)]
    pub overrides: SettingOverrides,
    pub seed: U64,
    pub step_cap: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct SetupError(pub String);

struct Format {
    settings: GameSettings,
    commander_slots: bool,
}

fn format_preset(name: &str) -> Option<Format> {
    match name {
        // CR 103.4
        "standard" => Some(Format {
            settings: GameSettings::default(),
            commander_slots: false,
        }),
        // CR 903.7, CR 103.5c
        "commander" => Some(Format {
            settings: GameSettings {
                starting_life: 40,
                free_mulligans: 1,
                ..GameSettings::default()
            },
            commander_slots: true,
        }),
        _ => None,
    }
}

pub(crate) struct Built {
    pub config: GameConfig,
    pub step_cap: u64,
}

struct Resolver<'a> {
    source: &'a dyn CardSource,
    loaded: HashMap<String, CardDefinition>,
}

impl Resolver<'_> {
    fn resolve(&mut self, key: &str) -> Result<CardDefinition, SetupError> {
        if let Some(card) = self.loaded.get(key) {
            return Ok(card.clone());
        }
        let card = self
            .source
            .load(key)
            .map_err(|e| SetupError(format!("card key {key:?} did not resolve: {e}")))?;
        self.loaded.insert(key.to_string(), card.clone());
        Ok(card)
    }
}

pub(crate) fn build(body: &SetupBody, source: &dyn CardSource) -> Result<Built, SetupError> {
    if body.players.len() < 2 {
        return Err(SetupError(format!(
            "a game needs at least 2 players, got {}",
            body.players.len()
        )));
    }
    let Some(format) = format_preset(&body.format) else {
        return Err(SetupError(format!(
            "format {:?} is unknown, expected \"standard\" or \"commander\"",
            body.format
        )));
    };
    if !format.commander_slots
        && let Some(index) = body.players.iter().position(|p| !p.commanders.is_empty())
    {
        return Err(SetupError(format!(
            "player {index} lists commanders, but format {:?} has no commander slots",
            body.format
        )));
    }
    let step_cap = body.step_cap.unwrap_or(DEFAULT_STEP_CAP);
    if step_cap == 0 {
        return Err(SetupError("step_cap must be at least 1".to_string()));
    }

    let mut settings = format.settings;
    body.overrides.apply(&mut settings);

    let mut resolver = Resolver {
        source,
        loaded: HashMap::new(),
    };
    let mut players = Vec::with_capacity(body.players.len());
    for (index, player) in body.players.iter().enumerate() {
        // Library order is decklist order; the engine shuffles if the settings say so.
        let mut library = Vec::new();
        for entry in &player.deck {
            let card = resolver.resolve(&entry.card)?;
            library.extend(std::iter::repeat_n(card, entry.count as usize));
        }
        let commanders = player
            .commanders
            .iter()
            .map(|key| resolver.resolve(key))
            .collect::<Result<Vec<_>, _>>()?;
        players.push(PlayerConfig {
            player_id: PlayerId(index as u32),
            library,
            commanders,
            life_override: None,
        });
    }

    let turn_order = players.iter().map(|p| p.player_id).collect();
    Ok(Built {
        config: GameConfig {
            players,
            turn_order,
            settings,
            rng_seed: body.seed.0,
        },
        step_cap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_source::FixtureCardSource;
    use serde_json::json;
    use stackfish_engine::{CardTypeSet, ManaCost};
    use stackfish_testutil::minimal_card_definition;

    fn source() -> FixtureCardSource {
        let card = |id: u64, name: &str| {
            minimal_card_definition(
                id,
                name,
                name,
                ManaCost {
                    symbols: Vec::new(),
                },
                CardTypeSet::CREATURE,
            )
        };
        FixtureCardSource::new()
            .with("a", card(1, "A"))
            .with("b", card(2, "B"))
            .with("boss", card(3, "Boss"))
    }

    fn body(value: serde_json::Value) -> SetupBody {
        serde_json::from_value(value).unwrap()
    }

    fn names(cards: &[CardDefinition]) -> Vec<&str> {
        cards.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn the_standard_preset_uses_the_default_settings() {
        let setup = body(json!({
            "players": [{ "deck": [] }, { "deck": [] }], "format": "standard", "seed": "7",
        }));
        let built = build(&setup, &source()).unwrap();
        let settings = &built.config.settings;
        assert_eq!(
            (
                settings.starting_life,
                settings.free_mulligans,
                settings.starting_hand_size
            ),
            (20, 0, 7)
        );
        assert!(settings.shuffle_libraries && !settings.skip_mulligan);
        assert_eq!(built.config.rng_seed, 7);
        assert_eq!(built.step_cap, DEFAULT_STEP_CAP);
    }

    #[test]
    fn the_commander_preset_has_forty_life_one_free_mulligan_and_commander_slots() {
        let setup = body(json!({
            "players": [{ "deck": [{ "card": "a", "count": 1 }], "commanders": ["boss"] }, { "deck": [] }],
            "format": "commander", "seed": "1",
        }));
        let built = build(&setup, &source()).unwrap();
        assert_eq!(
            (
                built.config.settings.starting_life,
                built.config.settings.free_mulligans
            ),
            (40, 1)
        );
        assert_eq!(names(&built.config.players[0].commanders), ["Boss"]);
        assert_eq!(
            names(&built.config.players[0].library),
            ["A"],
            "a commander is not in the library"
        );
    }

    #[test]
    fn overrides_replace_single_preset_fields() {
        let setup = body(json!({
            "players": [{ "deck": [] }, { "deck": [] }], "format": "commander", "seed": "1", "step_cap": 50,
            "overrides": { "starting_life": 25, "skip_mulligan": true, "max_mulligans": 2 },
        }));
        let built = build(&setup, &source()).unwrap();
        let settings = &built.config.settings;
        assert_eq!(
            (settings.starting_life, settings.free_mulligans),
            (25, 1),
            "untouched fields keep the preset"
        );
        assert!(settings.skip_mulligan);
        assert_eq!(settings.max_mulligans, Some(2));
        assert_eq!(built.step_cap, 50);
    }

    #[test]
    fn a_null_max_mulligans_override_means_no_limit() {
        let setup = body(json!({
            "players": [{ "deck": [] }, { "deck": [] }], "format": "standard", "seed": "1",
            "overrides": { "max_mulligans": null },
        }));
        assert_eq!(SettingOverrides::default().max_mulligans, None);
        assert_eq!(setup.overrides.max_mulligans, Some(None));
        let limited = body(json!({
            "players": [{ "deck": [] }, { "deck": [] }], "format": "standard", "seed": "1",
            "overrides": { "max_mulligans": 0 },
        }));
        assert_eq!(
            build(&limited, &source())
                .unwrap()
                .config
                .settings
                .max_mulligans,
            Some(0)
        );
        assert_eq!(
            build(&setup, &source())
                .unwrap()
                .config
                .settings
                .max_mulligans,
            None
        );
    }

    #[test]
    fn a_library_repeats_each_card_by_its_count_in_decklist_order() {
        let setup = body(json!({
            "players": [
                { "deck": [{ "card": "b", "count": 2 }, { "card": "a", "count": 1 }, { "card": "b", "count": 1 }] },
                { "deck": [{ "card": "a", "count": 0 }] },
            ],
            "format": "standard", "seed": "1",
        }));
        let built = build(&setup, &source()).unwrap();
        assert_eq!(
            names(&built.config.players[0].library),
            ["B", "B", "A", "B"]
        );
        assert!(built.config.players[1].library.is_empty());
        let ids: Vec<u32> = built.config.players.iter().map(|p| p.player_id.0).collect();
        assert_eq!(ids, [0, 1]);
        assert_eq!(
            built.config.turn_order,
            built
                .config
                .players
                .iter()
                .map(|p| p.player_id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_source_is_asked_once_per_distinct_key() {
        use std::cell::Cell;
        struct Counting(FixtureCardSource, Cell<u32>);
        impl CardSource for Counting {
            fn load(
                &self,
                key: &str,
            ) -> Result<CardDefinition, crate::card_source::CardSourceError> {
                self.1.set(self.1.get() + 1);
                self.0.load(key)
            }
        }
        let counting = Counting(source(), Cell::new(0));
        let setup = body(json!({
            "players": [{ "deck": [{ "card": "a", "count": 20 }] }, { "deck": [{ "card": "a", "count": 20 }] }],
            "format": "standard", "seed": "1",
        }));
        build(&setup, &counting).unwrap();
        assert_eq!(counting.1.get(), 1);
    }
}
