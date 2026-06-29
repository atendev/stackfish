use std::collections::HashMap;

use crate::types::ability::RegisteredAbility;
use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::GameEvent;
use crate::types::ids::{EventId, ObjectId, PlayerId, SequenceNo};
use crate::types::object::GameObject;
use crate::types::player::PlayerState;
use crate::types::stack::StackObject;
use crate::types::turn::{Phase, Step};
use crate::types::zone::{Zone, ZoneKind};

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
    /// Folded in from the Phase 5 PassPriority dry run (see
    /// docs/implementation.md) — neither field existed in Phase 1.
    /// `turn_order`/`active_player` alone don't tell you who currently has
    /// priority or how many players in a row have passed it (CR 117.3a,
    /// 117.4).
    pub priority_player: PlayerId,
    pub consecutive_passes: u32,
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
    /// Every card's registered triggered/activated/static abilities, flat —
    /// same reasoning as `continuous_effects`/`replacement_effects` above.
    pub abilities: Vec<RegisteredAbility>,
    pub ledger: Vec<GameEvent>,
    pub rng: RngState,
    /// Dedicated monotonic counter for `EventId` allocation — deliberately
    /// not derived from `ledger.len()` (that broke as soon as more than one
    /// event could be in flight before all of them land in the ledger, or
    /// events get generated somewhere other than the exact moment of the
    /// push). See `allocate_event_id`.
    pub next_event_id: u64,
    /// Dedicated monotonic counter for `SequenceNo` allocation. Same
    /// rationale as `next_event_id` — `ledger.len()` at commit time isn't a
    /// stable source: it depends on how many events from the same batch
    /// already landed, so replaying the same command stream against a
    /// different batching of `CommitEvents` calls could reorder/reuse
    /// sequence numbers. See `allocate_sequence_no`.
    pub next_sequence_no: u64,
}

impl GameState {
    pub fn allocate_event_id(&mut self) -> EventId {
        let id = EventId(self.next_event_id);
        self.next_event_id += 1;
        id
    }

    pub fn allocate_sequence_no(&mut self) -> SequenceNo {
        let seq = SequenceNo(self.next_sequence_no);
        self.next_sequence_no += 1;
        seq
    }

    pub fn object(&self, id: ObjectId) -> Option<&GameObject> {
        // TODO: self.objects.get(&id).
        todo!()
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut GameObject> {
        // TODO: self.objects.get_mut(&id).
        todo!()
    }

    pub fn player(&self, id: PlayerId) -> Option<&PlayerState> {
        // TODO: self.players.iter().find(|p| p.player_id == id) - note
        // PlayerId needs PartialEq, which it already derives (ids.rs).
        todo!()
    }

    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut PlayerState> {
        // TODO: same as player() but iter_mut().
        todo!()
    }

    /// Looks up the one `Zone` matching `kind`/`owner` in `zones`. Panics-
    /// worthy if more than one matches — that'd mean `zones` was built
    /// wrong (e.g. two battlefields) — but that invariant isn't enforced
    /// yet (Phase 6).
    pub fn zone(&self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&Zone> {
        // TODO: self.zones.iter().find(|z| z.kind == kind && z.owner ==
        // owner) - note ZoneKind already derives PartialEq (zone.rs); the
        // "more than one matches" invariant check is Phase 6 (Invariants),
        // not this phase - leave find() returning the first match for now.
        todo!()
    }

    pub fn zone_mut(&mut self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&mut Zone> {
        // TODO: same as zone() but iter_mut().
        todo!()
    }
}
