use stackfish_engine::*;

/// A fresh `GameObject` for `definition`, owned and controlled by `owner`, in `zone`.
/// Skips `GameObject::new` because that needs a `&GameState` and derives visibility from
/// the zone; this always sets `controlled_since_turn: 0` and public visibility.
pub fn card_object(
    id: ObjectId,
    definition: CardDefinition,
    owner: PlayerId,
    zone: ZoneKind,
) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
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
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy {
            public: true,
            visible_to: Vec::new(),
        },
        copy_generation: 0,
        entered_via_escape: false,
    }
}
