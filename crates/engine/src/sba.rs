use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::zone::ZoneKind;

/// One pass of CR 704's state-based actions: finds every applicable SBA and
/// applies all of them simultaneously, mutating `state` and returning the
/// `GameEvent`s that resulted (destroys, eliminations, etc) for the caller
/// to append to the ledger and feed into trigger collection.
///
/// Implemented this pass: 704.5a (player at 0 or less life loses), 704.5f
/// (creature toughness 0 or less goes to its owner's graveyard), 704.5g
/// (creature with lethal damage marked is destroyed, folding in 702.2b/c's
/// Deathtouch — any nonzero deathtouch damage counts as lethal — and
/// 702.12b's Indestructible — ignores 704.5g/deathtouch entirely, but NOT
/// 704.5f). Every other CR 704.5 SBA is still a no-op — see the TODO below
/// for which and why.
///
/// This is a single pass, not the fixed-point loop — CR 704.3 requires
/// repeating until no SBA applies. The engine loop (`engine.rs`) is
/// responsible for calling this in a loop until it returns empty, not this
/// function itself, so a caller stepping through ledger entries one at a
/// time can observe each pass separately rather than the loop being opaque.
pub fn check_state_based_actions(state: &mut GameState) -> Vec<GameEvent> {
    let mut events = Vec::new();

    apply_player_loss(state, &mut events);
    apply_creature_death(state, &mut events);

    // TODO: still unimplemented, no prerequisite machinery exists yet:
    //   - 704.5b: player who attempted to draw from an empty library loses
    //     (nothing tracks a failed draw attempt yet).
    //   - 704.5c: player with 10+ poison counters loses (PlayerState.counters
    //     exists but nothing increments poison yet).
    //   - CDA toughness (PTValue::CharacteristicDefining): apply_creature_death
    //     below skips any creature whose toughness isn't PTValue::Fixed,
    //     since resolving a CDA needs the script bridge
    //     (layers.rs::effective_characteristics, still todo!()).
    //   - continuous-effect P/T modification (CR 613 layer 7c): toughness is
    //     read straight off CardDefinition's printed value, ignoring any
    //     continuous effect that would raise/lower it - same layers.rs gap.
    //   - regeneration (CR 701.16): would replace 704.5g's destroy, not
    //     modeled, lethal damage always destroys outright here.
    //   - legend rule (704.5j), world rule (704.5k), planeswalker loyalty 0
    //     (704.5i - not v1 scope per DESIGN.md), commander damage >= 21
    //     (PlayerState::commander_damage_from already exposed for this), and
    //     the rest of 704.5 - no prerequisite subsystem exists yet.
    events
}

/// CR 704.5a: a player at 0 or less life loses the game. `eliminated` is
/// the field this flips (already present on `PlayerState`, just unused
/// until now); already-eliminated players are skipped so re-running this
/// pass doesn't re-emit the event every fixed-point iteration.
fn apply_player_loss(state: &mut GameState, events: &mut Vec<GameEvent>) {
    let losing: Vec<PlayerId> =
        state.players.iter().filter(|p| !p.eliminated && p.life <= 0).map(|p| p.player_id).collect();

    for player in losing {
        if let Some(p) = state.player_mut(player) {
            p.eliminated = true;
        }
        let event_id = state.allocate_event_id();
        let sequence_no = state.allocate_sequence_no();
        events.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::PlayerEliminated { player },
            caused_by_command: None,
            parent_event: None,
        });
    }
}

/// CR 704.5f/704.5g: a battlefield creature with non-positive toughness, or
/// with marked damage at or above its toughness, is put into its owner's
/// graveyard. Moves the surviving creature snapshot through a fresh
/// `ObjectId` (CR 400.7), same convention `PlayLand`/`CastSpell` resolution
/// already use for every other zone change in this codebase.
fn apply_creature_death(state: &mut GameState, events: &mut Vec<GameEvent>) {
    let dying: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && o.definition.card_types.contains(CardTypeSet::CREATURE))
        .filter_map(|o| {
            // CDA toughness (PTValue::CharacteristicDefining) can't be
            // resolved without the script bridge - skip rather than guess.
            // net_with_counters folds in +1/+1 / -1/-1 counters (CR 121.3) -
            // otherwise Wither/Infect's counters would have no effect on
            // whether this creature is actually dead.
            let toughness = o.definition.toughness.as_ref()?.net_with_counters(&o.counters)?;
            // CR 704.5f: 0-or-less toughness applies unconditionally, even
            // to an indestructible creature (702.12b only exempts 704.5g).
            if toughness <= 0 {
                return Some(o.object_id);
            }
            if o.definition.keywords.contains(KeywordSet::INDESTRUCTIBLE) {
                return None;
            }
            // CR 704.5g + 702.2b/c: lethal marked damage, or any nonzero
            // damage at all from a Deathtouch source.
            let lethal_damage =
                o.damage_marked >= toughness as u32 || (o.deathtouched && o.damage_marked > 0);
            lethal_damage.then_some(o.object_id)
        })
        .collect();

    for object_id in dying {
        let Some(object) = state.object(object_id) else {
            // Already moved by an earlier iteration of this same pass
            // (shouldn't happen - object_id is from this pass's own
            // snapshot - but no live object to move means nothing to do).
            continue;
        };
        let last_known_info = object.clone();
        let owner = last_known_info.owner;

        let new_id = state.allocate_object_id();
        let mut new_object = last_known_info.clone();
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage { previous: Some(object_id) };
        new_object.zone = ZoneKind::Graveyard;
        new_object.timestamp = state.allocate_sequence_no();

        if let Some(zone) = state.zone_mut(ZoneKind::Battlefield, None) {
            zone.remove(object_id);
        }
        state.objects.remove(&object_id);
        if let Some(zone) = state.zone_mut(ZoneKind::Graveyard, Some(owner)) {
            zone.push_bottom(new_id);
            state.objects.insert(new_id, new_object);
        }
        // else: no such zone registered - defensive no-op, same stance
        // ResolveStackTop/PlayLand already take on a missing zone_mut.

        let event_id = state.allocate_event_id();
        let sequence_no = state.allocate_sequence_no();
        events.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::ZoneChange {
                object: new_id,
                from: ZoneKind::Battlefield,
                to: ZoneKind::Graveyard,
                last_known_info: Some(last_known_info),
            },
            caused_by_command: None,
            parent_event: None,
        });
    }
}
