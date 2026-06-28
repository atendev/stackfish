use crate::types::card::CardDefinition;
use crate::types::counter::CounterKind;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, SequenceNo};
use crate::types::zone::ZoneKind;

/// Reveal scope for hidden-zone objects, additive to the zone's default
/// visibility. Right answer is "mostly the zone, plus this for exceptions":
/// the zone gives the default (library/hand hidden, battlefield/graveyard/
/// exile/stack public), but that default has real per-object exceptions a
/// zone-only model can't express — a face-down permanent on the (public)
/// battlefield whose owner secretly knows its identity, or a card on top of
/// a (hidden) library revealed to one player by a scry/surveil-style effect.
/// Keeping this field, but it's an override layer, not the primary source.
pub struct VisibilityPolicy {
    /// Visible to all players regardless of zone default (e.g. battlefield objects).
    pub public: bool,
    pub visible_to: Vec<PlayerId>,
}

/// Mutable runtime instance of a card, token, or emblem. Copiable values are
/// resolved at construction from `definition`; the continuous-effect layer
/// engine applies modifications on top at query time, not by mutating this
/// struct directly.
///
/// No in-zone ordering field here — that's the zone container's job (see
/// `GameState`'s per-zone `VecDeque<ObjectId>`s in `game.rs`), so there's a
/// single place that owns order instead of two places that can drift out of
/// sync.
pub struct GameObject {
    pub object_id: ObjectId,
    pub lineage: ObjectLineage,
    pub definition: CardDefinition,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub zone: ZoneKind,
    pub tapped: bool,
    pub flipped: bool,
    pub face_down: bool,
    pub phased_out: bool,
    pub damage_marked: u32,
    pub counters: Vec<(CounterKind, i32)>,
    pub attachments: Vec<ObjectId>,
    pub attached_to: Option<ObjectId>,
    /// For layer/dependency timestamp ordering (CR 613).
    pub timestamp: SequenceNo,
    pub visibility: VisibilityPolicy,
}
