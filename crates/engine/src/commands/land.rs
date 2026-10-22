//! `execute_command`'s `PassPriority`/`MakeChoice`/`PlayLand` arm bodies,
//! split out of the original monolithic `commands.rs`. `PassPriority` and
//! `MakeChoice` are trivial enough to share this file with `PlayLand`
//! rather than each getting their own.

use crate::script::ScriptRuntime;
use crate::types::card::CardTypeSet;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::zone::ZoneKind;

use super::attach::{next_active_player, remap_abilities};
use super::legality::is_sorcery_speed;
use super::{CommandOutcome, Internal, PriorityHandoff, Rejected, Validated};

pub(super) fn validate_pass_priority(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
) -> Result<Validated, Rejected> {
    // CR 117.3c: priority moves to the next player in turn_order
    // after whoever just passed - skipping anyone eliminated (CR
    // 800.4b). Read-only against turn_order - does not touch
    // state.priority_player/consecutive_passes, both of which
    // engine.rs owns (see PriorityHandoff/broke_pass_chain doc
    // comments above).
    if !state.turn_order.contains(&actor) {
        return Err(Rejected::IllegalAction("actor not in turn_order".to_string()));
    }
    // CR 508.1d: passing would skip the declaration of a creature that must attack.
    if super::combat::pass_would_skip_a_required_attack(state, script_runtime, actor) {
        return Err(Rejected::IllegalAction(
            "a creature that attacks each combat if able must be declared as an attacker".to_string(),
        ));
    }
    Ok(Validated::PassPriority { actor })
}

pub(super) fn apply_pass_priority(state: &GameState, actor: PlayerId) -> Result<CommandOutcome, Internal> {
    let next_player = next_active_player(state, actor);
    Ok(CommandOutcome {
        events: vec![GameEventKind::PriorityPassed { player: actor }],
        broke_pass_chain: false,
        priority_handoff: PriorityHandoff::To(next_player),
        requires_payment: None,
        battlefield_entries: Vec::new(),
    })
}

// TODO: `MakeChoice` accepts any call, even with no choice pending. It should be
// rejected here once something raises a choice through a `PlayerAction`.
pub(super) fn apply_make_choice() -> Result<CommandOutcome, Internal> {
    // Real choice application is genuinely generic - "depends
    // entirely on what asked for the choice" (see the TODO block
    // below) - no fixed event shape exists yet, so this produces
    // none. Exists as a minimal non-PassPriority stub so
    // broke_pass_chain's reset wiring (CR 117.3c) has something
    // other than PassPriority to exercise in tests; not a real
    // implementation of MakeChoice itself.
    Ok(CommandOutcome {
        events: Vec::new(),
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    })
}

/// What `apply_play_land` needs, checked by `validate_play_land`.
#[derive(Debug)]
pub(crate) struct LandPlan {
    actor: PlayerId,
    card: ObjectId,
}

pub(super) fn validate_play_land(state: &GameState, actor: PlayerId, card: ObjectId) -> Result<Validated, Rejected> {
    // CR 305.1: lands aren't spells, so this goes straight to the
    // battlefield - no stack object. Playing a land is still
    // restricted to sorcery speed (CR 305.1/305.3, via
    // is_sorcery_speed) and to once per turn (CR 305.2).
    if !is_sorcery_speed(state, actor) {
        return Err(Rejected::IllegalAction(
            "a land can only be played at sorcery speed: actor's turn, a main phase, \
             actor holding priority, and an empty stack"
                .to_string(),
        ));
    }
    // CR 305.2: normally one land per turn, raised by any applicable
    // LandPlayModifier (e.g. "you may play an additional land each turn").
    let already_played = *state.lands_played_this_turn.get(&actor).unwrap_or(&0);
    let limit = crate::types::player::effective_land_play_limit(&state.land_play_modifiers, actor);
    if already_played >= limit {
        return Err(Rejected::IllegalAction("actor has already played their land for this turn".to_string()));
    }

    let object = state.object(card).ok_or(Rejected::InvalidTarget)?;
    // TODO: reads CardDefinition.card_types directly rather than
    // layers::effective_characteristics (CR 613 layer 4, real as of
    // the layer-engine-types-colors slice) - a continuous effect
    // granting Land type to a card still in hand would not make it
    // playable here. Deliberately not wired this slice: this
    // object is in ZoneKind::Hand, and every real consumer in this
    // codebase so far (sba.rs's creature-type check, combat's
    // power/keyword checks) targets a battlefield permanent, not a
    // hand card - flagged rather than silently wired to an
    // untested shape.
    if !object.definition.card_types.contains(CardTypeSet::LAND) {
        return Err(Rejected::IllegalAction("card is not a Land".to_string()));
    }
    if object.zone != ZoneKind::Hand || object.controller != actor {
        return Err(Rejected::IllegalAction("card is not in actor's hand".to_string()));
    }
    // Every zone the move touches is looked up here, so `apply_play_land` has
    // nothing left that can fail (024).
    if state.zone(ZoneKind::Hand, Some(actor)).is_none() {
        return Err(Rejected::IllegalAction("actor has no hand zone".to_string()));
    }
    if state.zone(ZoneKind::Battlefield, None).is_none() {
        return Err(Rejected::IllegalAction("no battlefield zone".to_string()));
    }
    Ok(Validated::PlayLand(LandPlan { actor, card }))
}

pub(super) fn apply_play_land(state: &mut GameState, plan: LandPlan) -> Result<CommandOutcome, Internal> {
    let LandPlan { actor, card } = plan;
    let object = state.object(card).ok_or_else(|| Internal::new("play land: the validated card is gone"))?;
    // Snapshot before the move - CR 603.10's last_known_info (see
    // GameEventKind::ZoneChange's doc comment).
    let last_known_info = object.clone();

    // CR 400.7: this zone change creates a new object - mint a fresh
    // ObjectId rather than reusing `card`'s, and link back to it via
    // ObjectLineage. `card`'s entry in state.objects is removed
    // entirely once moved; anything that still needs to know what it
    // looked like in the hand must use `last_known_info` above, not
    // a live lookup by the old id.
    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(card) };
    new_object.zone = ZoneKind::Battlefield;
    new_object.timestamp = state.allocate_sequence_no();

    state
        .zone_mut(ZoneKind::Hand, Some(actor))
        .ok_or_else(|| Internal::new("play land: the validated hand zone is gone"))?
        .remove(card);
    state.objects.remove(&card);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .ok_or_else(|| Internal::new("play land: the validated battlefield zone is gone"))?
        .push_bottom(new_id);
    state.objects.insert(new_id, new_object);
    *state.lands_played_this_turn.entry(actor).or_insert(0) += 1;
    remap_abilities(state, card, new_id);

    Ok(CommandOutcome {
        events: vec![GameEventKind::ZoneChange {
            object: new_id,
            from: ZoneKind::Hand,
            to: ZoneKind::Battlefield,
            last_known_info: Some(last_known_info),
        }],
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: vec![new_id],
    })
}
