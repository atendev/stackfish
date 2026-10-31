//! Ability lookup, retirement and copy-purge, and the Aura, Equipment and
//! Fortification attach helpers. See `DESIGN.md` for the rest.

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityKind;
use crate::types::ability::RegisteredAbility;
use crate::types::ability::{AbilityEffect, ConditionPrimitive, ConditionSpec, CopyExceptions};
use crate::types::card::{CardDefinition, CardTypeSet, ModeChoice};
use crate::types::game::GameState;
use crate::types::ids::{AbilityId, ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::legality::attachment_target_is_legal;

/// The `ability_index`th ability registered to `source`, of any kind, in
/// registration order (the order pushed onto `state.abilities`). Only
/// `ActivateAbility` uses it: its index is chosen and consumed in one step with
/// no pause, so a position is safe. Triggers use `AbilityId` instead (see its
/// doc comment).
pub(crate) fn find_ability(
    state: &GameState,
    source: ObjectId,
    ability_index: u32,
) -> Option<&RegisteredAbility> {
    state
        .abilities
        .iter()
        .filter(|a| a.source == source)
        .nth(ability_index as usize)
}

/// Looks up a `RegisteredAbility` by its stable `AbilityId`, the
/// `PendingTrigger`-facing sibling of `find_ability`. `PutTriggersApnap` uses it
/// to resolve a collected trigger back to its ability when it goes on the stack.
pub(crate) fn find_ability_by_id(state: &GameState, id: AbilityId) -> Option<&RegisteredAbility> {
    state.abilities.iter().find(|a| a.id == id)
}

/// CR 400.7: abilities belong to their object and travel with it across zone
/// changes. Every zone change mints a new `ObjectId`, so call this to keep
/// `state.abilities` in sync.
///
/// Abilities registered by `run_card_script` (see `GameState::script_ability_ids`)
/// are dropped, not carried over: the script re-runs for the zone the object
/// lands in, and carrying the old registration would leave two copies of
/// everything the script declares.
pub(crate) fn remap_abilities(state: &mut GameState, old_id: ObjectId, new_id: ObjectId) {
    let script_ids = &mut state.script_ability_ids;
    let function_zones = &mut state.ability_function_zones;
    state.abilities.retain_mut(|ability| {
        if ability.source != old_id {
            return true;
        }
        if script_ids.remove(&ability.id) {
            function_zones.remove(&ability.id);
            return false;
        }
        ability.source = new_id;
        true
    });
}

/// CR 707.2a: removes every `RegisteredAbility`, `ContinuousEffect` and
/// `ReplacementEffect` sourced from `id` without removing `id` from
/// `state.objects` or purging any zone, unlike `retire_object`. The copied object
/// stays where it is (CR 707.4) with the same `ObjectId`. Callers then run
/// `ScriptRuntime::run_card_script` against the copy's new `CardDefinition`, so
/// the object does not end up with two values of each ability.
pub(crate) fn purge_granted_abilities(state: &mut GameState, id: ObjectId) {
    let script_ids = &mut state.script_ability_ids;
    let function_zones = &mut state.ability_function_zones;
    state.abilities.retain(|a| {
        if a.source != id {
            return true;
        }
        script_ids.remove(&a.id);
        function_zones.remove(&a.id);
        false
    });
    state.continuous_effects.retain(|e| e.source != id);
    state.replacement_effects.retain(|r| r.source != id);
}

/// CR 707.9a: the copy exception that grants a recurring become-a-copy upkeep
/// trigger (Vesuvan Doppelganger), the one supported shape for a copy effect that
/// grants a whole new ability (`add_keywords` only grants a keyword). It is a
/// hardcoded `RegisteredAbility` because no Lua-authorable ability blueprint
/// exists and no other card needs one.
///
/// Both copy-application sites (`resolve_become_copy_effect`,
/// `submit_copy_choice`) call it after `purge_granted_abilities`,
/// `run_card_script` and the `copy_generation` bump. A no-op unless
/// `exceptions.grant_recurring_copy_trigger` is set. The granted ability's
/// `effect` carries `exceptions` forward, so each trigger re-grants it.
///
/// `generation` tags the ability in `linked_ability_generation` (CR 707.9e/g).
/// The supersession check stays unreached through the two purge-based copy
/// producers; see `CopyExceptions`.
/// TODO: "may" (CR 601.2c) is not an opt-out pause, so the ability needs a real target like any targeted trigger and fizzles (CR 608.2b) with none
pub(crate) fn grant_linked_copy_ability(
    state: &mut GameState,
    object: ObjectId,
    exceptions: &CopyExceptions,
    generation: u32,
) {
    if !exceptions.grant_recurring_copy_trigger {
        return;
    }
    let id = state.allocate_ability_id();
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id,
        source: object,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
                mana_value: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::BecomeCopy(exceptions.clone())),
        intervening_if: None,
    });
    state.linked_ability_generation.insert(id, generation);
}

/// CR 707.9e: Spark Double's additional +1/+1 counter, an "additional effect"
/// exception (`CopyExceptions::apply` handles characteristic changes; see
/// `CopyExceptions` for why this cannot fold into it). Returns the `CounterAdded`
/// event for the caller to push beside its `BecameCopy` event and does not
/// mutate `state`, because `CommitEvents` applies `CounterAdded` generically and
/// a direct push to `GameObject.counters` would double it.
///
/// A no-op unless `exceptions.enters_with_additional_counters` is set. Both
/// copy-application sites call it beside `grant_linked_copy_ability`. A second
/// copy effect superseding the first before it applies is out of scope here, as
/// on `CopyExceptions`.
///
/// `x_value` (CR 107.3b) resolves `CounterAmount::X` to the entering object's
/// paid X, with no `{X}` in its cost treated as 0. Ignored for
/// `CounterAmount::Fixed`.
pub(crate) fn apply_copy_additional_counters(
    object: ObjectId,
    exceptions: &CopyExceptions,
    x_value: Option<u32>,
) -> Option<crate::types::event::GameEventKind> {
    let (kind, amount) = exceptions.enters_with_additional_counters.clone()?;
    let amount = match amount {
        crate::types::ability::CounterAmount::Fixed(amount) => amount,
        crate::types::ability::CounterAmount::X => x_value.unwrap_or(0),
    };
    Some(crate::types::event::GameEventKind::CounterAdded {
        object,
        kind,
        amount: amount as i32,
    })
}

/// CR 303.4: `definition` is an enchantment with the "Aura" subtype. `Subtype`
/// is a bare string newtype with no parser, so this compares strings like the
/// other subtype checks (`FilterSpec::matches` in cost.rs). Gated on card type
/// so a stray "Aura" subtype on a non-enchantment does not count.
pub(crate) fn is_aura(definition: &CardDefinition) -> bool {
    definition.card_types.contains(CardTypeSet::ENCHANTMENT)
        && definition.subtypes.iter().any(|s| s.0 == "Aura")
}

/// CR 301.5: `definition` is an artifact with the "Equipment" subtype, checked
/// by string comparison like `is_aura`.
pub(crate) fn is_equipment(definition: &CardDefinition) -> bool {
    definition.card_types.contains(CardTypeSet::ARTIFACT)
        && definition.subtypes.iter().any(|s| s.0 == "Equipment")
}

/// CR 301.6: `definition` is an artifact with the "Fortification" subtype,
/// checked like `is_equipment`.
pub(crate) fn is_fortification(definition: &CardDefinition) -> bool {
    definition.card_types.contains(CardTypeSet::ARTIFACT)
        && definition.subtypes.iter().any(|s| s.0 == "Fortification")
}

/// CR 303.4f: every object or player that is a legal enchant target for
/// `aura`'s enchant ability (`aura_definition.target_requirement`), evaluated
/// against `state` now. No target has been chosen yet, so this scans every
/// object and player and runs `target_is_legal` on each. `aura_source` supplies
/// the "another target" check (CR 115.4, `exclude_source`). It is not yet in
/// `state.objects` when called, before any zone move, so it can never be its own
/// candidate. An Aura whose `target_requirement` is not `TargetSpec::Primitive`
/// returns no candidates instead of panicking.
pub(crate) fn legal_aura_entry_candidates(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    aura_definition: &CardDefinition,
    controller: PlayerId,
    aura_source: ObjectId,
) -> Vec<Target> {
    let Some(TargetSpec::Primitive(req)) = &aura_definition.target_requirement else {
        return Vec::new();
    };
    // Sorted by id: `state.objects` is a `HashMap`, and a candidate list that
    // follows its order differs from run to run.
    let mut object_ids: Vec<ObjectId> = state.objects.keys().copied().collect();
    object_ids.sort();
    let object_candidates = object_ids.into_iter().map(Target::Object);
    let player_candidates = non_eliminated_players(state)
        .into_iter()
        .map(Target::Player);
    object_candidates
        .chain(player_candidates)
        // CR 202.3
        // An Aura's enchant restriction is a definition's base filter with no live X,
        // so `None` is correct.
        .filter(|&target| {
            attachment_target_is_legal(
                state,
                script_runtime,
                target,
                &req.filter,
                controller,
                aura_source,
                req.exclude_source,
                None,
            )
        })
        .collect()
}

/// CR 303.4f/301.5e/301.6: every object that is a legal attach target for
/// `definition` entering the battlefield attached, by permanent kind. An Aura's
/// legality is in its `target_requirement` (`legal_aura_entry_candidates`).
/// Equipment and Fortification take every creature or land on the battlefield,
/// since an Equip quality restriction does not limit what it may attach to
/// (CR 702.6c), judged on `effective_characteristics` so a continuous effect
/// that makes something a creature or land counts.
///
/// `None` means the card is not attach-capable (CR 303.4h). `Some(vec![])` means
/// it is attach-capable with no legal candidate, and the two differ: an Aura
/// stays put (CR 303.4g), while Equipment and Fortification enter unattached
/// (CR 301.5e/301.6).
pub(crate) fn legal_entry_attach_candidates(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    definition: &CardDefinition,
    controller: PlayerId,
    source: ObjectId,
) -> Option<Vec<Target>> {
    if is_aura(definition) {
        return Some(legal_aura_entry_candidates(
            state,
            script_runtime,
            definition,
            controller,
            source,
        ));
    }
    let wanted = if is_equipment(definition) {
        CardTypeSet::CREATURE
    } else if is_fortification(definition) {
        CardTypeSet::LAND
    } else {
        return None;
    };
    Some(
        state
            .objects
            .values()
            .filter(|obj| {
                obj.zone == ZoneKind::Battlefield
                    && crate::layers::effective_characteristics(
                        state,
                        script_runtime,
                        obj.object_id,
                    )
                    .card_types
                    .intersects(wanted)
            })
            .map(|obj| Target::Object(obj.object_id))
            .collect(),
    )
}

/// CR 303.4b/301.5a: attaches `attaching_id` (an Aura or Equipment) to
/// `target_id`, setting its `attached_to` and pushing it into the target's
/// `attachments` so both directions agree. This is the only writer of either
/// field. It does nothing if either id has no live object.
///
/// CR 301.5c/702.6a: an Equip activation can move an attached Equipment, so if
/// `attaching_id` is already attached elsewhere it is removed from the old
/// target's `attachments` first, leaving no stale entry.
pub(crate) fn attach_permanent(state: &mut GameState, attaching_id: ObjectId, target_id: ObjectId) {
    let previous_target = state.object(attaching_id).and_then(|o| o.attached_to);
    if let Some(old_target_id) = previous_target
        && old_target_id != target_id
        && let Some(old_target) = state.object_mut(old_target_id)
    {
        old_target.attachments.retain(|&id| id != attaching_id);
    }
    if let Some(attaching) = state.object_mut(attaching_id) {
        attaching.attached_to = Some(target_id);
        attaching.attached_to_player = None;
    }
    if let Some(target) = state.object_mut(target_id)
        && !target.attachments.contains(&attaching_id)
    {
        target.attachments.push(attaching_id);
    }
}

/// CR 303.4b: the "or player" half of `attach_permanent`, for an Aura enchanting
/// a player. `PlayerState` keeps no `attachments` list because nothing
/// enumerates the Auras on a player, and a reverse index would go unused. Only
/// Auras call it: Equipment and Fortification never attach to a player
/// (CR 301.5a/301.6), so there is no move-away-from-a-player case.
/// TODO: "enchanted player" references for continuous and replacement effects, since continuous effects only affect objects (CR 303.4m)
pub(crate) fn attach_permanent_to_player(
    state: &mut GameState,
    attaching_id: ObjectId,
    player: PlayerId,
) {
    if let Some(attaching) = state.object_mut(attaching_id) {
        attaching.attached_to_player = Some(player);
        attaching.attached_to = None;
    }
}

/// `propose_attach`'s result: what to do with `candidates` for one Aura's
/// attachment.
#[derive(Debug, PartialEq, Eq)]
pub enum AttachProposal {
    /// No candidate: nothing to attach. An Aura with no legal object or player
    /// stays where it is, or goes to the graveyard from the stack (CR 303.4g/i). The
    /// caller does that, not `propose_attach`.
    None,
    /// Exactly one candidate - attach immediately, no choice needed.
    Attach(ObjectId),
    /// CR 303.4d: more than one simultaneous candidate for the same Aura -
    /// its controller must choose which one it becomes attached to.
    NeedsChoice(Vec<ObjectId>),
}

/// CR 303.4d: an Aura cannot enchant more than one object or player, so if a
/// spell or ability would attach it to several, its controller chooses. A pure
/// decision over however many candidates the caller computed. `attach_permanent`
/// stays single-target because `GameObject.attached_to` is an `Option<ObjectId>`.
///
/// Nothing produces more than one candidate: both call sites (`ResolveStackTop`'s
/// Aura ETB attach, Equip's resolution) narrow to one legal target first. Tests
/// call it directly with a hand-built list, like `ProcessState::AwaitingAttachChoice`.
/// Candidates are object ids only, since no effect mixes object and player
/// candidates for one Aura.
pub fn propose_attach(candidates: &[ObjectId]) -> AttachProposal {
    match candidates {
        [] => AttachProposal::None,
        [only] => AttachProposal::Attach(*only),
        _ => AttachProposal::NeedsChoice(candidates.to_vec()),
    }
}

/// Removes `old_id` from `state.objects` and purges every `RegisteredAbility`,
/// `ContinuousEffect` and `ReplacementEffect` still pointing at it. Call it at a
/// zone-change site that does not call `remap_abilities` for `old_id` (discard,
/// `draw_step`, SBA-driven death). Calling both deletes what remap carried
/// forward; calling neither leaves dead entries behind.
///
/// Returns the departing object's `Triggered` abilities (CR 603.10), which would
/// otherwise be deleted before `collect_triggers` sees them. `sba::commit_death`
/// and `move_to_graveyard_for_cost` pass them to the engine's last-known-
/// information stash. `submit_discard` and `draw_step` drop them, because `Dies`
/// and `LeavesBattlefield` need `from == Battlefield`.
/// TODO: stash departed abilities from hand and library once a condition can consume them
pub(crate) fn retire_object(state: &mut GameState, old_id: ObjectId) -> Vec<RegisteredAbility> {
    let removed = state.objects.remove(&old_id);
    // CR 303.4b/301.5a
    // If old_id was attached to something (an Aura or Equipment that left play),
    // drop it from that object's `attachments` too, best effort. An object attached
    // to old_id keeps a stale `attached_to`, which the SBA checks (CR 704.5m/704.5n)
    // notice on their next pass, as in a real game.
    if let Some(target_id) = removed.and_then(|o| o.attached_to)
        && let Some(target) = state.object_mut(target_id)
    {
        target.attachments.retain(|&id| id != old_id);
    }
    let mut departed_triggered = Vec::new();
    state.off_battlefield_scripts_run.remove(&old_id);
    // CR 610.3: the exiled card left exile by another route, so there is
    // nothing left to return. A departing `source` keeps its link; the engine
    // consumes it right after.
    state.linked_exiles.retain(|link| link.exiled != old_id);
    let script_ids = &mut state.script_ability_ids;
    let function_zones = &mut state.ability_function_zones;
    state.abilities.retain(|a| {
        if a.source != old_id {
            return true;
        }
        script_ids.remove(&a.id);
        function_zones.remove(&a.id);
        if a.kind == AbilityKind::Triggered {
            departed_triggered.push(a.clone());
        }
        false
    });
    state.continuous_effects.retain(|e| e.source != old_id);
    // A repeatable replacement effect (a `Game.CreateReplacementEffect` shield)
    // outlives its source, so it is purged like `continuous_effects` above.
    state.replacement_effects.retain(|r| r.source != old_id);
    // CR 611.3b
    // A continuous effect from a static ability applies only while its source is on
    // the battlefield. These four player-level modifier lists have that shape (an
    // extra land drop, an extra draw), so they get the same source-keyed purge as
    // the three lists above.
    state.hand_size_modifiers.retain(|m| m.source != old_id);
    state.hand_reveal_modifiers.retain(|m| m.source != old_id);
    state.draw_limit_modifiers.retain(|m| m.source != old_id);
    state.land_play_modifiers.retain(|m| m.source != old_id);
    departed_triggered
}

/// CR 800.4b: an eliminated player's turns and priority are skipped. Walks
/// forward from `after` in `turn_order`, wrapping, to the first player still in
/// the game. Safe to call unconditionally: the engine builds `GameOver` before
/// fewer than 2 non-eliminated players remain (`finish_sba_loop`,
/// `finish_cleanup_sba_check`), so the walk finds one before wrapping to `after`.
pub(crate) fn next_active_player(state: &GameState, after: PlayerId) -> PlayerId {
    let start_idx = state
        .turn_order
        .iter()
        .position(|p| *p == after)
        .unwrap_or(0);
    for offset in 1..=state.turn_order.len() {
        let idx = (start_idx + offset) % state.turn_order.len();
        let candidate = state.turn_order[idx];
        if !state.player(candidate).is_some_and(|p| p.eliminated) {
            return candidate;
        }
    }
    after
}

/// Every player still in the game, in `turn_order` order. Used for the "everyone
/// passed" threshold and the `GameOver` check (CR 104.2a: last one standing wins;
/// CR 104.4a: none left is a draw).
pub(crate) fn non_eliminated_players(state: &GameState) -> Vec<PlayerId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|p| !state.player(*p).is_some_and(|pl| pl.eliminated))
        .collect()
}
