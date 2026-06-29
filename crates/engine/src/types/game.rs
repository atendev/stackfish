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
    /// CR 505.1: `Main` happens twice per turn (precombat/postcombat) as the
    /// same `Phase::Main` value with different successors - `Phase` alone
    /// can't tell which one this is (see turn.rs's `Phase` doc). Added for
    /// the Phase 5 AdvanceTurnStep slice (docs/implementation.md) since the
    /// deterministic transition table needs to disambiguate. Reset to
    /// `false` whenever a new turn begins (Beginning/Untap).
    pub postcombat_main: bool,
    /// CR 305.2: a player can normally play one land per turn (continuous
    /// effects raising that limit aren't modeled yet — see `PlayLand`'s
    /// TODO in commands.rs). Keyed by player rather than a single counter
    /// since CR 305.2a's comparison is per-player ("the number of lands
    /// they have already played this turn"). Reset to all-zero whenever a
    /// new turn begins (Beginning/Untap) — same trigger as
    /// `postcombat_main` above.
    pub lands_played_this_turn: HashMap<PlayerId, u32>,
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
        self.objects.get(&id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut GameObject> {
        self.objects.get_mut(&id)
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
        self.zones.iter().find(|z| z.kind == kind && z.owner == owner)
    }

    pub fn zone_mut(&mut self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&mut Zone> {
        self.zones.iter_mut().find(|z| z.kind == kind && z.owner == owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::ObjectId;
    use std::collections::VecDeque;

    /// Direct unit test against `Zone::remove`/`push_bottom` and
    /// `GameState::zone_mut`/`object_mut` themselves, isolated from the
    /// command layer (`PlayLand` in commands.rs exercises the same helpers
    /// through a real command, but this pins down the helpers' own
    /// behavior independently).
    #[test]
    fn zone_helpers_roundtrip() {
        let hand = PlayerId(0);
        let mut state = GameState {
            players: Vec::new(),
            turn_order: vec![hand],
            active_player: hand,
            turn_number: 1,
            phase: crate::types::turn::Phase::PreGame,
            step: crate::types::turn::Step::Untap,
            priority_player: hand,
            consecutive_passes: 0,
            postcombat_main: false,
            lands_played_this_turn: HashMap::new(),
            objects: HashMap::new(),
            zones: vec![
                Zone { kind: ZoneKind::Hand, owner: Some(hand), contents: VecDeque::new() },
                Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            ],
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
        };

        let card = ObjectId(0);
        state.zone_mut(ZoneKind::Hand, Some(hand)).unwrap().push_bottom(card);
        assert!(state.zone(ZoneKind::Hand, Some(hand)).unwrap().contents.contains(&card));

        let removed = state.zone_mut(ZoneKind::Hand, Some(hand)).unwrap().remove(card);
        assert_eq!(removed, Some(card));
        assert!(!state.zone(ZoneKind::Hand, Some(hand)).unwrap().contents.contains(&card));

        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(card);
        assert!(state.zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&card));

        // Nonexistent zone (no command zone registered here) -> None, not a panic.
        assert!(state.zone_mut(ZoneKind::Command, Some(hand)).is_none());

        // object()/object_mut() round-trip against an absent id.
        assert!(state.object(card).is_none());
        assert!(state.object_mut(card).is_none());
    }
}
