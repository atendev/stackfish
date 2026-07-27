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
/// struct directly. `copiable_override` (CR 707/613.1a, layer 1) is the one
/// exception — see its own doc comment below for why layer 1 can't fit the
/// query-only shape every other layer uses.
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
    /// CR 707.2/707.2b/707.2c (layer 1, CR 613.1a): a frozen snapshot of
    /// another object's copiable values, taken at the moment this object
    /// became a copy. `None` (the overwhelming common case) means "use
    /// `definition` as-is" — every other layer (2, 4-7) is a live query
    /// re-derived from current state on every call, but layer 1 can't be:
    /// CR 707.2b says changing the original's copiable values after the
    /// copy exists must NOT change the copy, and CR 707.2c freezes a copy
    /// effect's granted values at the moment it first applies. A live query
    /// re-reading the source every time would violate both, so this is a
    /// real mutation written once (by whatever command implements "becomes
    /// a copy of"), not a `ContinuousEffect` in `state.continuous_effects`.
    /// `layers::effective_characteristics` reads this ahead of `definition`
    /// when present; nothing else consults `definition` directly without
    /// going through that query already gets this for free. Cloned forward
    /// unmodified by every zone-change site that clones the prior
    /// `GameObject`, same as `is_commander`'s own "rides along automatically"
    /// note — a site that mints a genuinely fresh identity (a non-copy
    /// token) sets this `None` instead, same as that field sets `false`.
    pub copiable_override: Option<CardDefinition>,
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
    /// CR 303.4b's "or player" half of `attached_to` — an Aura can enchant
    /// a player instead of an object (an Equipment/Fortification never can,
    /// CR 301.5a/301.6). A sibling field rather than widening `attached_to`
    /// to `Option<Target>`: that would force every existing object-attach
    /// call site (`commands::attach_permanent`, every SBA/layer/replacement
    /// check already reading `attached_to` as a plain `ObjectId`) to handle
    /// a case that can never apply to them, for zero benefit — same
    /// "additive sibling, not a widened payload" precedent `ManaUnit`'s
    /// `RestrictedColored`/`SnowColored` variants already established.
    /// Mutually exclusive with `attached_to` by construction — nothing ever
    /// sets both; `commands::attach_permanent`/`attach_permanent_to_player`
    /// are the only writers of either.
    pub attached_to_player: Option<PlayerId>,
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
    /// CR 903.3: this specific card is a player's designated commander — an
    /// attribute of the card/object itself, not its `CardDefinition` (a
    /// definition can be shared by non-commander copies of the same card),
    /// and one that "retains this designation even when it changes zones."
    /// Every real zone-change site in this crate builds the successor
    /// `GameObject` from an explicit struct literal or a `.clone()` of the
    /// original, so this rides along automatically wherever a site clones
    /// forward — a site that mints a fresh object (a token, a copy) sets
    /// this `false` instead, since neither is ever a commander.
    pub is_commander: bool,
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
