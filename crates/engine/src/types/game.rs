use std::collections::HashMap;

use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::GameEvent;
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::object::GameObject;
use crate::types::player::PlayerState;
use crate::types::stack::StackObject;
use crate::types::turn::{Phase, Step};
use crate::types::zone::Zone;

/// Seeded once at game start. Every draw is itself a logged `GameEvent`
/// (`RngDrawn`), not an ambient side effect, so replay is exact.
pub struct RngState {
    pub seed: u64,
    pub draws_consumed: u64,
}

/// Owns the full mutable state for one game. Player count is a runtime
/// parameter — not hardcoded to 2 or 4 — to support N-player Commander.
pub struct GameState {
    pub players: Vec<PlayerState>,
    /// Turn order as a list of player indices into `players`; APNAP order is
    /// this list rotated to start at the active player.
    pub turn_order: Vec<PlayerId>,
    pub active_player: PlayerId,
    pub turn_number: u32,
    pub phase: Phase,
    pub step: Step,
    /// Object store — single source of truth for object data, looked up by
    /// id. `zones` below holds only ids in order; that's the single source
    /// of truth for *order*, so order is never duplicated on the object
    /// itself (see the no-`zone_position` note on `GameObject`).
    pub objects: HashMap<ObjectId, GameObject>,
    /// Every library/hand/graveyard/command zone (one set per player) plus
    /// the shared battlefield and exile — one flat list rather than a
    /// per-player struct, since the zone's own `owner: Option<PlayerId>`
    /// already disambiguates whose zone it is. The Stack is the deliberate
    /// exception: it's tracked in `stack` below instead, since it needs
    /// `StackObject` metadata, not just an id list.
    pub zones: Vec<Zone>,
    pub stack: Vec<StackObject>,
    pub continuous_effects: Vec<ContinuousEffect>,
    pub replacement_effects: Vec<ReplacementEffect>,
    pub ledger: Vec<GameEvent>,
    pub rng: RngState,
}

/// Result of one `engine.step()` call. Concrete shape (what callers can
/// observe/act on) is defined in Phase 2 — this is a placeholder marker type
/// so `GameState` ownership boundaries are visible now.
pub struct StepResult;
