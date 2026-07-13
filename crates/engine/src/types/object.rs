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
    /// CR 702.2: whether any of the damage currently in `damage_marked` was
    /// dealt by a source with Deathtouch — a plain `u32` total can't answer
    /// that on its own (1 damage from a deathtouch source is just as lethal
    /// as 1 damage from a vanilla source's 20). Reset to `false` alongside
    /// `damage_marked` at Cleanup (CR 514.2).
    pub deathtouched: bool,
    pub counters: CounterSet,
    pub attachments: Vec<ObjectId>,
    pub attached_to: Option<ObjectId>,
    /// For layer/dependency timestamp ordering (CR 613). Deliberately
    /// separate from `controlled_since_turn` below — that field answers a
    /// different question (summoning sickness), and conflating the two
    /// would be the same "one field doing two jobs" mistake `ZoneChange`'s
    /// `Zone`-vs-`ZoneKind` bug already taught this codebase to avoid (see
    /// docs/implementation.md).
    pub timestamp: SequenceNo,
    /// CR 302.6/602.5a (summoning sickness): the global `GameState.
    /// turn_number` at the moment this object's current controller most
    /// recently gained control of it — set at creation, and (not modeled
    /// yet — no "gain control" effect exists in this engine) whenever
    /// `controller` changes. Consulted via `commands::has_summoning_sickness`
    /// rather than stored as a bool, since a bool would need imperative
    /// upkeep at every untap-step/control-change site to avoid drifting.
    pub controlled_since_turn: u32,
    /// True for token permanents (CR 111). Tokens that leave the battlefield
    /// should cease to exist (CR 111.7), but enforcement is a TODO — they
    /// currently move to zones like normal cards until that SBA hook exists.
    pub is_token: bool,
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
        // 0; deathtouched: false; counters: CounterSet::new(); attachments: empty;
        // attached_to: None; controlled_since_turn: needs the current
        // GameState.turn_number, which this signature has no way to read -
        // this constructor will need a turn_number param (or a &GameState)
        // before it can set this field correctly; visibility: needs a
        // sensible default tied to `zone`'s default (e.g. fully public if
        // zone == Battlefield, owner-only otherwise) - VisibilityPolicy
        // doesn't have a zone-aware constructor yet, may need one.
        todo!()
    }

    /// `zone == Battlefield`. A permanent is the only state a `GameObject`
    /// can have continuous effects/static abilities active from (CR 110.1).
    pub fn is_permanent(&self) -> bool {
        // TODO: self.zone == ZoneKind::Battlefield.
        todo!()
    }
}
