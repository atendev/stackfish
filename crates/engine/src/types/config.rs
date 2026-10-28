use crate::types::card::CardDefinition;
use crate::types::ids::PlayerId;

/// Automated behaviors applied during game setup. Defaults match standard
/// two-player rules.
#[derive(Clone, PartialEq, Debug)]
pub struct GameSettings {
    /// Shuffle each library before the opening draw. Set `false` for
    /// deterministic scenarios where input order matters.
    pub shuffle_libraries: bool,

    /// Hand size for the opening draw and every redraw.
    pub starting_hand_size: u8,
    /// Every player keeps the opening hand with no mulligan and no bottoming.
    pub skip_mulligan: bool,
    /// Mulligans that put nothing on the bottom. Two-player uses 0;
    /// multiplayer and Commander use 1 (CR 103.5c).
    pub free_mulligans: u32,
    /// `None` is unlimited, `Some(0)` allows no mulligans.
    pub max_mulligans: Option<u32>,

    /// Default 20 (CR 103.4). Commander uses 40.
    pub starting_life: u32,
    /// Poison counters that make a player lose (CR 704.5c).
    pub poison_loss_threshold: u32,
    /// Enforced during cleanup (CR 514.1).
    pub max_hand_size: u32,
}

impl Default for GameSettings {
    fn default() -> Self {
        GameSettings {
            shuffle_libraries: true,
            starting_hand_size: 7,
            skip_mulligan: false,
            free_mulligans: 0,
            max_mulligans: None,
            starting_life: 20,
            poison_loss_threshold: 10,
            max_hand_size: 7,
        }
    }
}

/// One player's starting state, before any game rules apply.
#[derive(Clone, PartialEq, Debug)]
pub struct PlayerConfig {
    pub player_id: PlayerId,
    /// Top to bottom. Shuffled before any draw when
    /// `GameSettings::shuffle_libraries` is set, otherwise kept as given.
    pub library: Vec<CardDefinition>,
    /// CR 903.6: placed in the command zone at setup, not shuffled into
    /// `library`. A `Vec` because Partner allows several. Empty outside
    /// Commander.
    pub commanders: Vec<CardDefinition>,
    /// Overrides `GameSettings::starting_life` for this player only.
    pub life_override: Option<u32>,
}

/// Input to `Engine::new_game`, which builds the initial `GameState` and runs
/// pregame setup.
#[derive(Clone, PartialEq, Debug)]
pub struct GameConfig {
    pub players: Vec<PlayerConfig>,
    /// Must list every id in `players` exactly once. The first entry takes the
    /// first turn (CR 103.1).
    pub turn_order: Vec<PlayerId>,
    pub settings: GameSettings,
    /// Seed for shuffling and any other random pregame step.
    pub rng_seed: u64,
}
