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
            visibility: VisibilityPolicy {
                public,
                visible_to: if public { Vec::new() } else { vec![owner] },
            },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    /// On the battlefield (CR 110.1).
    pub fn is_permanent(&self) -> bool {
        self.zone == ZoneKind::Battlefield
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, ModeChoice, PTValue, ScriptReference, SupertypeSet};
    use crate::types::mana::ManaCost;

    fn dummy_definition() -> CardDefinition {
        CardDefinition {
            definition_id: 1,
            name: "Test".to_string(),
            mana_cost: ManaCost {
                symbols: Vec::new(),
            },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: crate::types::card::CardTypeSet::CREATURE,
            subtypes: Vec::new(),
            oracle_text: String::new(),
            power: Some(PTValue::Fixed(1)),
            toughness: Some(PTValue::Fixed(1)),
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::ByKey("test".to_string()),
            keywords: crate::types::card::KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        }
    }

    fn dummy_state() -> GameState {
        GameState {
            players: Vec::new(),
            turn_order: Vec::new(),
            active_player: PlayerId(0),
            turn_number: 1,
            phase: crate::types::turn::Phase::PreGame,
            step: crate::types::turn::Step::Untap,
            priority_player: PlayerId(0),
            consecutive_passes: 0,
            lands_played_this_turn: std::collections::HashMap::new(),
            combat: None,
            last_active_turn_number: std::collections::HashMap::new(),
            objects: std::collections::HashMap::new(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: crate::types::game::RngState {
                seed: 1,
                draws_consumed: 0,
            },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    #[test]
    fn visibility_public_is_visible_to_everyone() {
        let policy = VisibilityPolicy {
            public: true,
            visible_to: Vec::new(),
        };
        assert!(policy.is_visible_to(PlayerId(0)));
        assert!(policy.is_visible_to(PlayerId(1)));
    }

    #[test]
    fn visibility_private_is_visible_only_to_listed_players() {
        let policy = VisibilityPolicy {
            public: false,
            visible_to: vec![PlayerId(0)],
        };
        assert!(policy.is_visible_to(PlayerId(0)));
        assert!(!policy.is_visible_to(PlayerId(1)));
    }

    #[test]
    fn new_object_starts_as_permanent_only_on_battlefield() {
        let state = dummy_state();
        let battlefield = GameObject::new(
            ObjectId(0),
            dummy_definition(),
            PlayerId(0),
            ZoneKind::Battlefield,
            SequenceNo(0),
            &state,
        );
        assert!(battlefield.is_permanent());

        let hand = GameObject::new(
            ObjectId(1),
            dummy_definition(),
            PlayerId(0),
            ZoneKind::Hand,
            SequenceNo(0),
            &state,
        );
        assert!(!hand.is_permanent());
    }

    #[test]
    fn new_object_visibility_is_public_on_battlefield() {
        let state = dummy_state();
        let obj = GameObject::new(
            ObjectId(0),
            dummy_definition(),
            PlayerId(0),
            ZoneKind::Battlefield,
            SequenceNo(0),
            &state,
        );
        assert!(obj.visibility.public);
        assert!(obj.visibility.visible_to.is_empty());
    }

    #[test]
    fn new_object_visibility_is_owner_only_off_battlefield() {
        let state = dummy_state();
        let owner = PlayerId(0);
        let obj = GameObject::new(
            ObjectId(0),
            dummy_definition(),
            owner,
            ZoneKind::Hand,
            SequenceNo(0),
            &state,
        );
        assert!(!obj.visibility.public);
        assert_eq!(obj.visibility.visible_to, vec![owner]);
    }

    #[test]
    fn reset_for_new_object_clears_battlefield_state() {
        let mut obj = GameObject {
            object_id: ObjectId(0),
            lineage: ObjectLineage { previous: None },
            definition: dummy_definition(),
            copiable_override: Some(dummy_definition()),
            owner: PlayerId(0),
            controller: PlayerId(1),
            zone: ZoneKind::Hand,
            tapped: true,
            flipped: true,
            face_down: true,
            phased_out: true,
            damage_marked: 5,
            deathtouched: true,
            counters: CounterSet::new(),
            attachments: vec![ObjectId(99)],
            attached_to: Some(ObjectId(98)),
            attached_to_player: Some(PlayerId(2)),
            timestamp: SequenceNo(0),
            controlled_since_turn: 2,
            is_token: true,
            is_commander: true,
            visibility: VisibilityPolicy {
                public: true,
                visible_to: Vec::new(),
            },
            copy_generation: 3,
            entered_via_escape: true,
        };
        obj.reset_for_new_object();
        assert_eq!(obj.controller, obj.owner);
        assert!(!obj.tapped);
        assert!(!obj.flipped);
        assert!(!obj.face_down);
        assert!(!obj.phased_out);
        assert_eq!(obj.damage_marked, 0);
        assert!(!obj.deathtouched);
        assert!(obj.attachments.is_empty());
        assert!(obj.attached_to.is_none());
        assert!(obj.attached_to_player.is_none());
        assert!(obj.copiable_override.is_none());
        assert_eq!(obj.copy_generation, 0);
        assert!(!obj.entered_via_escape);
        // Identity bits and controlled_since_turn intentionally survive.
        assert!(obj.is_token);
        assert!(obj.is_commander);
    }
}
