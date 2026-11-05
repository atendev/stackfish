//! The `PassPriority`, `MakeChoice` and `PlayLand` command bodies. See
//! `DESIGN.md` for the rest.

use crate::script::ScriptRuntime;
use crate::types::card::CardTypeSet;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::zone::ZoneKind;

use super::attach::{next_active_player, remap_abilities};
use super::legality::{can_play_land_from_zone, is_sorcery_speed};
use super::{CommandOutcome, Internal, PriorityHandoff, Rejected, Validated};

pub(super) fn validate_pass_priority(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
) -> Result<Validated, Rejected> {
    // CR 117.3c/800.4b
    // Priority goes to the next non-eliminated player in turn_order. This only reads turn_order, because the engine owns priority_player and consecutive_passes.
    if !state.turn_order.contains(&actor) {
        return Err(Rejected::IllegalAction(
            "actor not in turn_order".to_string(),
        ));
    }
    // CR 508.1d
    // Passing would skip the declaration of a creature that must attack.
    if super::combat::pass_would_skip_a_required_attack(state, script_runtime, actor) {
        return Err(Rejected::IllegalAction(
            "a creature that attacks each combat if able must be declared as an attacker"
                .to_string(),
        ));
    }
    Ok(Validated::PassPriority { actor })
}

pub(super) fn apply_pass_priority(
    state: &GameState,
    actor: PlayerId,
) -> Result<CommandOutcome, Internal> {
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
    // No fixed event shape exists because applying a choice depends on what asked for it (see the TODO above). This stub gives broke_pass_chain's reset (CR 117.3c) a command other than PassPriority to exercise in tests.
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
    from_zone: ZoneKind,
    from_owner: Option<PlayerId>,
}

pub(super) fn validate_play_land(
    state: &GameState,
    actor: PlayerId,
    card: ObjectId,
) -> Result<Validated, Rejected> {
    // CR 305.1/305.3
    // A land is not a spell, so it skips the stack but still needs sorcery speed.
    if !is_sorcery_speed(state, actor) {
        return Err(Rejected::IllegalAction(
            "a land can only be played at sorcery speed: actor's turn, a main phase, \
             actor holding priority, and an empty stack"
                .to_string(),
        ));
    }
    // CR 305.2
    // One land per turn, raised by any applicable LandPlayModifier.
    let already_played = *state.lands_played_this_turn.get(&actor).unwrap_or(&0);
    let limit = crate::types::player::effective_land_play_limit(&state.land_play_modifiers, actor);
    if already_played >= limit {
        return Err(Rejected::IllegalAction(
            "actor has already played their land for this turn".to_string(),
        ));
    }

    let object = state.object(card).ok_or(Rejected::InvalidTarget)?;
    // TODO: reads CardDefinition.card_types instead of layers::effective_characteristics, so a continuous effect that grants Land to a card in hand does not make it playable (CR 613)
    if !object.definition.card_types.contains(CardTypeSet::LAND) {
        return Err(Rejected::IllegalAction("card is not a Land".to_string()));
    }
    // CR 305.1
    // A land comes from the actor's hand unless a zone permission opens another zone.
    if !can_play_land_from_zone(state, actor, object) {
        return Err(Rejected::IllegalAction(
            "card is not in a zone actor may play lands from".to_string(),
        ));
    }
    // CR 400.7
    // Exile is shared, and any other zone belongs to the card's owner.
    let from_zone = object.zone;
    let from_owner = if from_zone == ZoneKind::Exile {
        None
    } else {
        Some(object.owner)
    };
    // Every zone the move touches is looked up here, so `apply_play_land` has nothing left that can fail.
    if state.zone(from_zone, from_owner).is_none() {
        return Err(Rejected::IllegalAction(
            "the card's zone does not exist".to_string(),
        ));
    }
    if state.zone(ZoneKind::Battlefield, None).is_none() {
        return Err(Rejected::IllegalAction("no battlefield zone".to_string()));
    }
    Ok(Validated::PlayLand(LandPlan {
        actor,
        card,
        from_zone,
        from_owner,
    }))
}

pub(super) fn apply_play_land(
    state: &mut GameState,
    plan: LandPlan,
) -> Result<CommandOutcome, Internal> {
    let LandPlan {
        actor,
        card,
        from_zone,
        from_owner,
    } = plan;
    let object = state
        .object(card)
        .ok_or_else(|| Internal::new("play land: the validated card is gone"))?;
    // CR 603.10
    // Snapshot before the move, for last_known_info.
    let last_known_info = object.clone();

    // CR 400.7
    // The move makes a new object: mint a fresh ObjectId linked back through ObjectLineage. `card`'s entry in state.objects is removed, so later reads of the old version must use last_known_info.
    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage {
        previous: Some(card),
    };
    new_object.zone = ZoneKind::Battlefield;
    // CR 110.2
    // A land played from another player's graveyard enters under the player who played it.
    new_object.controller = actor;
    new_object.timestamp = state.allocate_sequence_no();

    state
        .zone_mut(from_zone, from_owner)
        .ok_or_else(|| Internal::new("play land: the validated source zone is gone"))?
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
            from: from_zone,
            to: ZoneKind::Battlefield,
            last_known_info: Some(last_known_info),
        }],
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: vec![new_id],
    })
}
