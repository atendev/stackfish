use crate::types::card::CardDefinition;
use crate::types::counter::CounterSet;
use crate::types::game::GameState;
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
    /// This struct only covers the override layer (see doc comment above) -
    /// callers needing the *effective* visibility (zone default plus this
    /// override) need to combine both, probably in GameState/layers.rs, not
    /// here.
    pub fn is_visible_to(&self, player: PlayerId) -> bool {
        self.public || self.visible_to.contains(&player)
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
    /// cease to exist (CR 111.7) via `sba::collect_ceasing_tokens`.
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
    /// CR 707.9e/g: how many times this specific object (same `object_id`)
    /// has become a copy of something — bumped by `resolve_become_copy_effect`
    /// (CR 707.2's `BecomeCopy`) and `submit_copy_choice` (CR 707.6's
    /// `EntersAsCopy`) each time either applies, right alongside the
    /// `copiable_override` write itself. Starts at `0` for every object that
    /// has never been a copy (the overwhelming majority) and for a freshly
    /// minted `TokenSpec::CopyOf` token — a brand-new `object_id` has no
    /// prior "own identity" to have copied *from* yet, so its first copy
    /// application isn't a *second* one.
    ///
    /// Exists to answer CR 707.9e/g's shared question — "has *another* copy
    /// effect applied to this object since a given earlier one did?" — for
    /// an exception/linked-ability granted by that earlier copy effect that
    /// needs to stop applying once superseded. `GameState::
    /// linked_ability_generation` tags an `AbilityId` with the
    /// `copy_generation` it was granted under; `triggers::collect_triggers`
    /// checks a tagged ability's own source object's *current`
    /// `copy_generation` against that tag before it's allowed to match at
    /// all (CR 707.9g). Real, tested enforcement — no producer sets that tag
    /// yet, since `CopyExceptions` doesn't have a "this copy effect also
    /// grants a whole new ability" shape (only characteristic-modification
    /// exceptions exist so far) - same "real mechanism, no producer wired in
    /// yet" posture `HandRevealModifier`'s own doc comment already takes.
    /// CR 707.9e's "additional effect" exception variant is structurally the
    /// same question against the same counter, just with no consumer at all
    /// in this engine (no "additional effect" copy exception exists either).
    pub copy_generation: u32,
    /// CR 702.138b: `true` only if this object's own current incarnation
    /// entered the battlefield as a spell cast via an Escape ability's
    /// alternative cost (`StackObject::escaped`, carried across by
    /// `Engine::move_resolving_spell`) - `false` for every other object,
    /// including a card still sitting in its graveyard waiting to be
    /// escaped (this describes *this incarnation's own* provenance, not a
    /// standing permission). Consulted by `InterveningIfPrimitive::Escaped`
    /// (CR 603.4's "if it escaped" clause, Kroxa/Uro's real card pattern)
    /// and exposed on the Lua object-state snapshot for CR 702.138c/d's own
    /// literal "escapes with [counters/ability]" templates
    /// (`phase5/escape-provenance`). Never reset once true - a permanent
    /// only changes `object_id` (CR 400.7) by leaving and re-entering,
    /// which always re-derives this fresh from whatever cast it *that*
    /// time.
    pub entered_via_escape: bool,
}

impl GameObject {
    /// Builds a brand-new object (`lineage: None` — a zone-change successor
    /// that needs to carry lineage forward doesn't go through this
    /// constructor, it clones/struct-updates the prior `GameObject`
    /// instead). `controller` defaults to `owner` (CR 110.2).
    /// `controlled_since_turn` reads `state.turn_number` live rather than
    /// taking a bare `u32`, so a caller can't pass a stale value.
    /// `visibility` defaults to fully public for a battlefield object, and
    /// visible only to `owner` for anything else — the ordinary zone-default
    /// case; a caller with a real per-object exception (e.g. a face-down
    /// permanent) overrides `visibility` on the result.
    pub fn new(
        object_id: ObjectId,
        definition: CardDefinition,
        owner: PlayerId,
        zone: ZoneKind,
        timestamp: SequenceNo,
        state: &GameState,
    ) -> Self {
        let public = zone == ZoneKind::Battlefield;
        GameObject {
            object_id,
            lineage: ObjectLineage { previous: None },
            definition,
            copiable_override: None,
            owner,
            controller: owner,
            zone,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            attached_to_player: None,
            timestamp,
            controlled_since_turn: state.turn_number,
            is_token: false,
            is_commander: false,
            visibility: VisibilityPolicy { public, visible_to: if public { Vec::new() } else { vec![owner] } },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    /// `zone == Battlefield`. A permanent is the only state a `GameObject`
    /// can have continuous effects/static abilities active from (CR 110.1).
    pub fn is_permanent(&self) -> bool {
        self.zone == ZoneKind::Battlefield
    }
}
