use crate::commands;
use crate::script::ScriptRuntime;
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
pub fn check_state_based_actions(state: &mut GameState, script_runtime: &ScriptRuntime) -> Vec<GameEvent> {
    let mut events = Vec::new();

    apply_player_loss(state, &mut events);
    apply_creature_death(state, script_runtime, &mut events);

    // TODO: still unimplemented, no prerequisite machinery exists yet:
    //   - 704.5b: player who attempted to draw from an empty library loses
    //     (nothing tracks a failed draw attempt yet).
    //   - 704.5c: player with 10+ poison counters loses (PlayerState.counters
    //     exists but nothing increments poison yet).
    //   - CDA toughness (PTValue::CharacteristicDefining): now real, via
    //     layers::effective_characteristics's layer 7a resolution (as of the
    //     layer-engine-cda slice) - see apply_creature_death's own doc
    //     comment. Only resolves if the card's CharacteristicDefining value
    //     actually carries a script (script: Some(ScriptReference::Inline(_)));
    //     no script, or a script that errors, still skips rather than
    //     guesses (returns None), same as always.
    //   - continuous-effect P/T modification (CR 613 layer 7c) and
    //     Indestructible-granting (layer 6): now real, via
    //     layers::effective_characteristics - see apply_creature_death's own
    //     doc comment. Combat-damage power and blocking-legality keyword
    //     checks (commands.rs) are NOT wired to the layer engine yet.
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
///
/// Toughness, Indestructible, and (as of the layer-engine-types-colors
/// slice) creature type itself are all read via
/// `layers::effective_characteristics` (CR 613 layers 4/6/7a/7b/7c) rather
/// than straight off `CardDefinition` — first flagged in
/// docs/implementation.md's layer-engine-basics slice. A "+1/+1 to all your
/// creatures"-style continuous effect, one granting Indestructible, one
/// granting the Creature type itself (e.g. an animated land,
/// `AddCardType(CREATURE)` plus a `SetPT` for its 3/3), or a scripted
/// characteristic-defining toughness (layer 7a, as of the layer-engine-cda
/// slice) now actually changes whether this check applies at all.
/// `commands.rs`'s combat-damage power (`effective_power`) and
/// blocking-legality keyword checks (`effective_keywords`) already read
/// through the same query (since layer-engine-consumers) — as of this
/// slice's `&ScriptRuntime` threading into `execute_command`, a scripted
/// CDA power resolves there too, no separate wiring needed.
fn apply_creature_death(state: &mut GameState, script_runtime: &ScriptRuntime, events: &mut Vec<GameEvent>) {
    let candidates: Vec<ObjectId> =
        state.objects.values().filter(|o| o.zone == ZoneKind::Battlefield).map(|o| o.object_id).collect();

    let dying: Vec<ObjectId> = candidates
        .into_iter()
        .filter(|&id| {
            let effective = crate::layers::effective_characteristics(state, script_runtime, id);
            // CR 613 layer 4: a land animated into a creature by
            // AddCardType(CREATURE) is subject to this SBA; a plain land
            // (or any other non-creature permanent) isn't - checked against
            // the effective type, not CardDefinition.card_types, so a
            // granted type actually counts.
            if !effective.card_types.contains(CardTypeSet::CREATURE) {
                return false;
            }
            // CDA toughness (PTValue::CharacteristicDefining) can't be
            // resolved without the script bridge - skip rather than guess,
            // same stance `PTValue::as_fixed`/`layers.rs` already take.
            let Some(toughness) = effective.toughness else { return false };
            let object = state.object(id).expect("candidate id came from state.objects itself");
            // CR 704.5f: 0-or-less toughness applies unconditionally, even
            // to an indestructible creature (702.12b only exempts 704.5g).
            if toughness <= 0 {
                return true;
            }
            if effective.keywords.contains(KeywordSet::INDESTRUCTIBLE) {
                return false;
            }
            // CR 704.5g + 702.2b/c: lethal marked damage, or any nonzero
            // damage at all from a Deathtouch source.
            object.damage_marked >= toughness as u32 || (object.deathtouched && object.damage_marked > 0)
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
        // CR 400.7: this object is gone for good (no remap follows, unlike
        // PlayLand/CastSpell/ResolveStackTop's "same card, new id" moves) -
        // purge any RegisteredAbility still pointing at it (e.g. a self-
        // registered ETB trigger) rather than leaving a dead entry behind.
        commands::retire_object(state, object_id);
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
