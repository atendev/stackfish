use crate::types::card::CardDefinition;
use crate::types::counter::CounterSet;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, SequenceNo};
use crate::types::zone::ZoneKind;

/// Per-object reveal scope, layered over the zone's default visibility. It
/// covers exceptions a zone-only model cannot express, such as a face-down
/// permanent whose owner knows its identity or a library card revealed to
/// one player.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityPolicy {
    /// Visible to all players regardless of zone default (e.g. battlefield objects).
    pub public: bool,
    pub visible_to: Vec<PlayerId>,
}

impl VisibilityPolicy {
    /// Override layer only. Effective visibility also needs the zone default.
    pub fn is_visible_to(&self, player: PlayerId) -> bool {
        self.public || self.visible_to.contains(&player)
    }
}

/// Runtime instance of a card, token or emblem. Copiable values come from
/// `definition`, and the layer engine applies modifications at query time.
/// `copiable_override` is the one stored exception.
///
/// Zone order lives in the zone container, not here.
#[derive(Clone, Debug, PartialEq)]
pub struct GameObject {
    pub object_id: ObjectId,
    pub lineage: ObjectLineage,
    pub definition: CardDefinition,
    /// CR 707.2b, 707.2c, 613.1a: frozen snapshot of the copied object's
    /// copiable values, taken when this object became a copy. Layer 1 cannot
    /// be a live query, because later changes to the original must not change
    /// the copy. `layers::effective_characteristics` reads it ahead of
    /// `definition`. Zone-change sites that clone the prior object carry it
    /// forward, and a fresh identity sets `None`.
    pub copiable_override: Option<CardDefinition>,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub zone: ZoneKind,
    pub tapped: bool,
    pub flipped: bool,
    pub face_down: bool,
    pub phased_out: bool,
    pub damage_marked: u32,
    /// CR 702.2: some of `damage_marked` came from a deathtouch source, which
    /// the total cannot show. Reset with it at cleanup (CR 514.2).
    pub deathtouched: bool,
    pub counters: CounterSet,
    pub attachments: Vec<ObjectId>,
    pub attached_to: Option<ObjectId>,
    /// CR 303.4b: an Aura enchanting a player, which `attached_to` cannot
    /// hold. Equipment never attaches to a player (CR 301.5a, 301.6).
    /// Exclusive with `attached_to`; only `commands::attach_permanent` and
    /// `attach_permanent_to_player` write either.
    pub attached_to_player: Option<PlayerId>,
    /// CR 613 timestamp for layer and dependency ordering. Separate from
    /// `controlled_since_turn`, which answers summoning sickness.
    pub timestamp: SequenceNo,
    /// CR 302.6, 602.5a: `GameState.turn_number` when the current controller
    /// last gained control. Read through `commands::has_summoning_sickness`
    /// instead of kept as a flag, so no site has to update a flag.
    pub controlled_since_turn: u32,
    /// True for token permanents (CR 111). Tokens that leave the battlefield
    /// cease to exist (CR 111.7) via `sba::collect_ceasing_tokens`.
    pub is_token: bool,
    /// CR 903.3: this card is a player's commander. The designation belongs
    /// to the object, not the shareable `CardDefinition`, and survives zone
    /// changes. Sites that clone the prior object carry it, and a fresh token
    /// or copy sets `false`.
    pub is_commander: bool,
    pub visibility: VisibilityPolicy,
    /// CR 707.9e, 707.9g: times this object has become a copy, bumped beside
    /// each `copiable_override` write by `resolve_become_copy_effect` and
    /// `submit_copy_choice`. `GameState::linked_ability_generation` tags an
    /// `AbilityId` with the generation it was granted under, and
    /// `triggers::collect_triggers` drops a tagged ability once the
    /// generation moves on.
    // TODO: no copy exception grants an ability yet, so nothing sets that tag (CR 707.9g)
    pub copy_generation: u32,
    /// CR 702.138b: this incarnation entered the battlefield as a spell cast
    /// with an Escape alternative cost (`StackObject::escaped`). A card still
    /// in a graveyard is `false`. Read by `InterveningIfPrimitive::Escaped`
    /// and the Lua object snapshot (CR 702.138c, 702.138d). A new object
    /// re-derives it (CR 400.7).
    pub entered_via_escape: bool,
}

impl GameObject {
    /// CR 400.7: a new object keeps no battlefield state, and a card off the
    /// battlefield or stack is controlled by its owner (CR 108.4). Call it on
    /// the new object only. The event's last-known info is a separate clone.
    pub(crate) fn reset_for_new_object(&mut self) {
        self.controller = self.owner;
        self.tapped = false;
        self.flipped = false;
        self.face_down = false;
        self.phased_out = false;
        self.damage_marked = 0;
        self.deathtouched = false;
        self.counters = crate::types::counter::CounterSet::new();
        self.attachments = Vec::new();
        self.attached_to = None;
        self.attached_to_player = None;
        self.copiable_override = None;
        self.copy_generation = 0;
        self.entered_via_escape = false;
    }

    /// A brand-new object. Zone-change successors clone the prior object
    /// instead, to keep lineage. `controller` starts as `owner` (CR 110.2).
    /// `controlled_since_turn` reads `state.turn_number` so a caller cannot
    /// pass a stale value. Visibility is public on the battlefield and
    /// owner-only elsewhere; override it for a per-object exception.
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

    /// On the battlefield (CR 110.1).
    pub fn is_permanent(&self) -> bool {
        self.zone == ZoneKind::Battlefield
    }
}
