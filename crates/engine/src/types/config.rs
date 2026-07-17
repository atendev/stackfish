use crate::types::card::CardDefinition;
use crate::types::ids::PlayerId;

/// Controls which automated behaviors the engine applies during game setup.
/// All settings have defaults matching standard two-player rules; override
/// individual fields for testing or variant formats.
pub struct GameSettings {
    // --- Library setup ---
    /// Shuffle each player's library before drawing opening hands.
    /// Set `false` for deterministic test scenarios where input order matters.
    pub shuffle_libraries: bool,

    // --- Mulligan ---
    /// Target hand size for the opening draw and every redraw.
    pub starting_hand_size: u8,
    /// Skip mulligan decisions entirely — all players auto-keep their opening
    /// hand with zero mulligans taken (implies no bottoming either).
    pub skip_mulligan: bool,
    /// Number of mulligans that cost nothing (no card goes on the bottom when
    /// the player eventually keeps). CR 103.5c grants 1 free mulligan in
    /// multiplayer/Commander; standard two-player grants 0.
    pub free_mulligans: u32,
    /// Hard cap on how many times a player may mulligan. `None` = unlimited
    /// (standard rules); `Some(0)` = no mulligans allowed.
    pub max_mulligans: Option<u32>,

    // --- Life / loss thresholds ---
    /// Starting life total for every player. Default 20 (CR 103.4).
    /// Override to 40 for Commander, 30 for Two-Headed Giant, etc.
    pub starting_life: u32,
    /// Number of poison counters that causes a player to lose (CR 704.5c).
    pub poison_loss_threshold: u32,
    /// Maximum hand size enforced at end of turn cleanup (CR 514.1).
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

/// Per-player input for `GameConfig`. Describes one player's starting state
/// before any game rules are applied.
pub struct PlayerConfig {
    pub player_id: PlayerId,
    /// Cards in library order, top-to-bottom. If `GameSettings::shuffle_libraries`
    /// is true, the engine shuffles this before any draws occur; otherwise the
    /// order is preserved exactly as given (deterministic for testing).
    pub library: Vec<CardDefinition>,
    /// CR 903.6: this player's designated commander(s), placed in the
    /// command zone at game setup instead of being shuffled into `library`.
    /// A `Vec` (not `Option`) since Partner commanders mean a player can
    /// have more than one (DESIGN.md's Multiplayer/Commander section: "Command
    /// zone holds commander(s)"). Empty for a non-Commander game — no
    /// Command zone-specific behavior is reachable unless a `GameObject`
    /// actually has `is_commander: true`, so an empty `Vec` here is a
    /// complete no-op, not a special case.
    pub commanders: Vec<CardDefinition>,
    /// Starting life total. If `None`, `GameSettings::starting_life` is used;
    /// if `Some`, this overrides the setting for this player only (e.g. for
    /// life-modifier vanguard cards or individual test overrides).
    pub life_override: Option<u32>,
}

/// Full input bundle passed to `Engine::new_game`. The engine builds the
/// initial `GameState` from this and runs the pregame setup sequence.
pub struct GameConfig {
    pub players: Vec<PlayerConfig>,
    /// Turn order by `PlayerId`. Must reference every id in `players` exactly
    /// once. The first entry is the starting player (CR 103.1).
    pub turn_order: Vec<PlayerId>,
    pub settings: GameSettings,
    /// RNG seed for shuffling and any other random pregame operations.
    pub rng_seed: u64,
}
