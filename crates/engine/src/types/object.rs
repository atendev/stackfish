use crate::types::card::CardDefinition;
use crate::types::counter::CounterSet;
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
#[derive(Clone, Debug)]
pub struct VisibilityPolicy {
    /// Visible to all players regardless of zone default (e.g. battlefield objects).
    pub public: bool,
    pub visible_to: Vec<PlayerId>,
}

impl VisibilityPolicy {
    pub fn is_visible_to(&self, player: PlayerId) -> bool {
        // TODO: self.public || self.visible_to.contains(&player). Note this
        // struct only covers the override layer (see doc comment above) -
        // callers needing the *effective* visibility (zone default plus
        // this override) need to combine both, probably in GameState/
        // layers.rs, not here.
        todo!()
    }
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
#[derive(Clone, Debug)]
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
    pub counters: CounterSet,
    pub attachments: Vec<ObjectId>,
    pub attached_to: Option<ObjectId>,
    /// For layer/dependency timestamp ordering (CR 613).
    pub timestamp: SequenceNo,
    pub visibility: VisibilityPolicy,
}

impl GameObject {
    pub fn new(
        object_id: ObjectId,
        definition: CardDefinition,
        owner: PlayerId,
        zone: ZoneKind,
        timestamp: SequenceNo,
    ) -> Self {
        // TODO: controller defaults to owner at creation (CR 110.2);
        // lineage: ObjectLineage { previous: None } for a brand-new object
        // (vs. a zone-change successor, which needs a different
        // constructor or a lineage param - not covered by this signature
        // yet); tapped/flipped/face_down/phased_out: false; damage_marked:
        // 0; counters: CounterSet::new(); attachments: empty;
        // attached_to: None; visibility: needs a sensible default tied to
        // `zone`'s default (e.g. fully public if zone == Battlefield, owner-
        // only otherwise) - VisibilityPolicy doesn't have a zone-aware
        // constructor yet, may need one.
        todo!()
    }

    /// `zone == Battlefield`. A permanent is the only state a `GameObject`
    /// can have continuous effects/static abilities active from (CR 110.1).
    pub fn is_permanent(&self) -> bool {
        // TODO: self.zone == ZoneKind::Battlefield.
        todo!()
    }
}
