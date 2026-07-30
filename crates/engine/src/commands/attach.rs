//! Ability-lookup, retirement/copy-purge, and Aura/Equipment/Fortification
//! attach helpers, shared by `execute_command`'s match arms and called
//! directly by `engine.rs` - split out of the original monolithic
//! `commands.rs`.

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityKind;
use crate::types::ability::RegisteredAbility;
use crate::types::ability::{AbilityEffect, ConditionPrimitive, ConditionSpec, CopyExceptions};
use crate::types::card::{CardDefinition, CardTypeSet};
use crate::types::game::GameState;
use crate::types::ids::{AbilityId, ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::legality::target_is_legal;

/// Looks up the `ability_index`th ability registered to `source` — any
/// kind, in registration order (the order entries were pushed onto
/// `state.abilities`). Used only by `ActivateAbility` (below) —
/// `PlayerActionKind::ActivateAbility::ability_index` is chosen and consumed
/// in the same synchronous step (never spanning a pause), so a position is
/// still safe here; see `AbilityId`'s own doc comment for why
/// `triggers::PendingTrigger` needed a real stable id instead of this same
/// convention (`phase5/ability-stable-id`).
pub(crate) fn find_ability(
    state: &GameState,
    source: ObjectId,
    ability_index: u32,
) -> Option<&RegisteredAbility> {
    state.abilities.iter().filter(|a| a.source == source).nth(ability_index as usize)
}

/// Looks up a `RegisteredAbility` by its stable `AbilityId` — the
/// `PendingTrigger`-facing sibling of `find_ability` above. Used by
/// `PutTriggersApnap` (engine.rs) to resolve a collected trigger back into
/// its real ability at push-onto-stack time.
pub(crate) fn find_ability_by_id(state: &GameState, id: AbilityId) -> Option<&RegisteredAbility> {
    state.abilities.iter().find(|a| a.id == id)
}

/// CR 400.7: abilities belong to their object and travel with it across zone
/// changes. Every zone change mints a new `ObjectId`; call this to keep
/// `state.abilities` in sync so ability lookups by source id don't go stale.
pub(crate) fn remap_abilities(state: &mut GameState, old_id: ObjectId, new_id: ObjectId) {
    for ability in state.abilities.iter_mut() {
        if ability.source == old_id {
            ability.source = new_id;
        }
    }
}

/// Removes `old_id` from `state.objects` and purges any `RegisteredAbility`,
/// `ContinuousEffect`, or `ReplacementEffect` still pointing at it
/// (`state.abilities.retain`/`state.continuous_effects.retain`/
/// `state.replacement_effects.retain`). Use at a zone-change site that does
/// **not** also call `remap_abilities` for `old_id` — calling both would be
/// wrong (retire would delete what remap just carried forward); calling
/// neither leaves a dead entry sitting in `state.abilities`/
/// `state.continuous_effects`/`state.replacement_effects` forever (harmless
/// per `collect_triggers`'s/`layers::applicable_effects_in_layer`'s existing
/// "skip if source doesn't resolve" checks, but pure pileup — the same gap
/// this function was originally built to close for abilities, extended here
/// to continuous effects now that `Game.CreateStaticAbility` (script.rs) can
/// produce one that outlives its source, and again to replacement effects
/// now that `Game.CreateReplacementEffect` can too). Today that's exactly the
/// sites with no
/// in-play "same card, new id" continuation to carry forward: discard
/// (hand→graveyard), `draw_step` (library→hand), and SBA-driven death
/// (battlefield→graveyard) — the other zone-change sites (`PlayLand`,
/// `CastSpell`, mulligan bottom/redraw, `ResolveStackTop`'s permanent
/// branch) already call `remap_abilities` instead, which is the correct
/// behavior there (e.g. a mana ability registered against a land while
/// still in hand must survive the hand→battlefield move).
///
/// Returns the departing object's own `Triggered`-kind abilities (CR
/// 603.10) — a "when this dies"/"when this leaves the battlefield" ability
/// would otherwise be deleted right here, before `collect_triggers` ever
/// gets a chance to see it. Two call sites now thread this up into
/// `Engine`'s transient last-known-information stash so trigger collection
/// can still find it: `sba::commit_death` (SBA-driven death) and
/// `move_to_graveyard_for_cost` (Sacrifice/Discard cost payment, via
/// `commit_payment` → `Engine::submit_payment`) — a "when this dies"
/// ability now fires off a sacrificed permanent the same way it does off a
/// combat death. The remaining two call sites (`submit_discard`'s CR 514.1
/// hand-size discard, `draw_step`'s library-side retirement) still discard
/// the return value: neither can ever produce a Battlefield-departure LKI
/// (`Dies`/`LeavesBattlefield` require `from == Battlefield`), so stashing
/// there would be inert until a hand/library-departure condition shape
/// exists to consume it — deliberately left, not an oversight (see
/// docs/implementation.md's backlog item 7).

/// CR 707.2a (layer 1, "becomes a copy of"): removes every `RegisteredAbility`,
/// `ContinuousEffect`, and `ReplacementEffect` still sourced from `id`,
/// *without* removing `id` itself from `state.objects` or purging any zone —
/// unlike `retire_object`,
/// the object being copied stays exactly where it is (CR 707.4: it remains
/// on the battlefield, same identity, same `ObjectId`). Callers ordinarily
/// call `ScriptRuntime::run_card_script` against the copy's new `CardDefinition`
/// right after this to re-register whatever the *new* copiable text grants -
/// this function only clears out what the object had self-registered under
/// its pre-copy identity, so the two don't end up stacked (CR 707.2a: "it
/// doesn't wind up with two values of each ability").
pub(crate) fn purge_granted_abilities(state: &mut GameState, id: ObjectId) {
    state.abilities.retain(|a| a.source != id);
    state.continuous_effects.retain(|e| e.source != id);
    state.replacement_effects.retain(|r| r.source != id);
}

/// CR 707.9a: Vesuvan Doppelganger's own "except it has 'at the beginning of
/// your upkeep, you may have this creature become a copy of target
/// creature'" — the one concrete shape this engine supports for "a copy
/// effect grants a whole new ability" (as opposed to `add_keywords`, which
/// only grants a keyword). A hardcoded Rust `RegisteredAbility` rather than
/// something `run_card_script` produces — no Lua-authorable "ability
/// blueprint" type exists to describe an arbitrary granted ability, and this
/// is the only real card that needs one.
///
/// Called by both real copy-application call sites
/// (`resolve_become_copy_effect`/`submit_copy_choice`), right after they
/// call `purge_granted_abilities` + `run_card_script` + bump `object`'s own
/// `copy_generation` — a no-op unless `exceptions.grant_recurring_copy_
/// trigger` is set. The granted ability's own `effect` carries `exceptions`
/// forward unchanged, so triggering it again re-grants the same ability
/// again (Vesuvan keeps flipping indefinitely) — CR 601.2c's "may" isn't
/// modeled as a genuine opt-out pause here (this engine has no "declining a
/// triggered ability" mechanism at all yet); the ability simply requires a
/// real target the same way any other targeted trigger does, and fizzles at
/// resolution (CR 608.2b) with none available, a narrowing named rather
/// than silently assumed.
///
/// `generation` tags the new ability in `linked_ability_generation` (CR
/// 707.9e/g's own hook) — see `CopyExceptions`'s own doc comment for why
/// this tag's supersession check stays structurally unreached through this
/// engine's two purge-based copy producers regardless.
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
        id,
        source: object,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
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

/// CR 707.9e: Spark Double's own "if it's a creature, it enters with an
/// additional +1/+1 counter on it" - the "additional effect" exception shape
/// (as opposed to `CopyExceptions::apply`'s characteristic-modification
/// shapes) - see `CopyExceptions`'s own doc comment for why this can't just
/// be folded into `apply`. Returns the `GameEventKind::CounterAdded` to push
/// alongside the caller's own `BecameCopy` event, rather than mutating
/// `state` directly - `CommitEvents` already knows how to apply a
/// `CounterAdded` generically, and pushing straight to `GameObject.counters`
/// here would double it, the same "pick exactly one" mistake this backlog's
/// own mistakes-caught section already names for `EntersWithCounters`.
///
/// No-op unless `exceptions.enters_with_additional_counters` is set. Called
/// by both real copy-application call sites
/// (`resolve_become_copy_effect`/`submit_copy_choice`), right alongside
/// `grant_linked_copy_ability` - CR 707.9e's own invalidation clause (this
/// exception's effect doesn't happen if a *second* copy effect supersedes
/// the first before it applies) is out of scope here, same as it is on
/// `CopyExceptions` itself - see that type's own doc comment.
pub(crate) fn apply_copy_additional_counters(
    object: ObjectId,
    exceptions: &CopyExceptions,
) -> Option<crate::types::event::GameEventKind> {
    let (kind, amount) = exceptions.enters_with_additional_counters.clone()?;
    Some(crate::types::event::GameEventKind::CounterAdded { object, kind, amount: amount as i32 })
}

/// CR 303.4: `definition` is an Aura - an enchantment (card type) with the
/// "Aura" subtype. No `Subtype`-vocabulary parser exists (`Subtype` is a
/// bare string newtype, `types/card.rs`) - checked by direct string
/// comparison, same as every other ad hoc subtype check in this codebase
/// (`cost.rs`'s `FilterSpec::matches` subtype filter does the identical
/// `object.definition.subtypes.contains(s)` shape). Card-type-gated rather
/// than subtype-alone, matching CR 303.4's own "some enchantments have the
/// subtype Aura" framing - a non-enchantment permanent could theoretically
/// carry a stray "Aura" subtype string without this treating it as one.
pub(crate) fn is_aura(definition: &CardDefinition) -> bool {
    definition.card_types.contains(CardTypeSet::ENCHANTMENT)
        && definition.subtypes.iter().any(|s| s.0 == "Aura")
}

/// CR 301.5: `definition` is an Equipment - an artifact (card type) with
/// the "Equipment" subtype. Same ad hoc string-comparison shape `is_aura`
/// already established, for the same reason (`Subtype` has no parsed
/// vocabulary).
pub(crate) fn is_equipment(definition: &CardDefinition) -> bool {
    definition.card_types.contains(CardTypeSet::ARTIFACT)
        && definition.subtypes.iter().any(|s| s.0 == "Equipment")
}

/// CR 301.6: `definition` is a Fortification - an artifact (card type) with
/// the "Fortification" subtype. Same ad hoc string-comparison shape
/// `is_equipment` already establishes.
pub(crate) fn is_fortification(definition: &CardDefinition) -> bool {
    definition.card_types.contains(CardTypeSet::ARTIFACT)
        && definition.subtypes.iter().any(|s| s.0 == "Fortification")
}

/// CR 303.4f: every object or player currently a legal enchant target for
/// `aura`'s own enchant ability (`aura_definition.target_requirement),
/// evaluated fresh against `state` right now - unlike the normal cast-time
/// path, no target has been chosen yet, so this scans every object and
/// player in `state` rather than rechecking one. Reuses `target_is_legal`
/// (same filter/legality logic the cast-time path already consults) once
/// per candidate - `target_is_legal` is already fully generic over
/// `Target::Object`/`Target::Player` (see its own match arms for
/// `TargetFilter::Player`/`PlayerScript`/`AnyTarget`), so this function only
/// needed to start constructing `Target::Player` candidates too
/// (`phase5/aura-enters-attached-player` - player candidates were
/// deliberately deferred by the original `phase5/aura-enters-attached` slice,
/// named as out of scope there since `phase5/aura-enchants-a-player` had only
/// closed the cast-and-resolve path at the time). `aura_source` both
/// supplies CR 115.4's "another target" check (`exclude_source`) and is
/// excluded from the object scan implicitly by `aura_source` not yet
/// existing in `state.objects` at the point this is called (see
/// `SearchDestination::Battlefield`'s call site - the Aura's own
/// not-yet-inserted `ObjectId` is passed here before any zone move happens,
/// so it can never appear as a candidate of itself; CR 303.4d self-enchant
/// only matters once the Aura already exists on the battlefield, which this
/// narrow entry path doesn't produce). No `Aura` carrying a
/// `target_requirement` other than `TargetSpec::Primitive` (i.e. none, this
/// engine never builds one) returns no candidates rather than panicking -
/// same "skip rather than guess" stance every other unresolvable-requirement
/// case in this codebase already takes.
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
    let object_candidates = state.objects.values().map(|obj| Target::Object(obj.object_id));
    let player_candidates = non_eliminated_players(state).into_iter().map(Target::Player);
    object_candidates
        .chain(player_candidates)
        .filter(|&target| target_is_legal(state, script_runtime, target, &req.filter, controller, aura_source, req.exclude_source))
        .collect()
}

/// CR 303.4f/301.5e/301.6: every object currently a legal attach target for
/// `definition` entering the battlefield already attached, dispatched by
/// permanent kind - the three don't share a legality shape. An Aura's
/// legality is packed into the card's own `target_requirement`
/// (`legal_aura_entry_candidates`), but CR 702.6c already establishes an
/// Equip ability's own quality restrictions ("Equip [quality]") don't
/// restrict what the Equipment may be attached to at all - so Equipment/
/// Fortification legality is just "every creature/land on the battlefield,"
/// no filter to evaluate, checked against `effective_characteristics` (not
/// the raw printed `card_types`) so something that became a creature/land
/// only through a continuous effect still counts. Returns `None` for any
/// other card kind - not attach-capable at all (CR 303.4h) - so the caller
/// can tell that apart from "attach-capable but zero legal candidates right
/// now" (`Some(vec![])`), since the two are handled differently by CR
/// 303.4g (Aura: stays put) vs CR 301.5e/301.6 (Equipment/Fortification:
/// enters unattached).
pub(crate) fn legal_entry_attach_candidates(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    definition: &CardDefinition,
    controller: PlayerId,
    source: ObjectId,
) -> Option<Vec<Target>> {
    if is_aura(definition) {
        return Some(legal_aura_entry_candidates(state, script_runtime, definition, controller, source));
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
                    && crate::layers::effective_characteristics(state, script_runtime, obj.object_id).card_types.intersects(wanted)
            })
            .map(|obj| Target::Object(obj.object_id))
            .collect(),
    )
}

/// CR 303.4b/301.5a: attaches `attaching_id` (an Aura or an Equipment) to
/// `target_id` - sets the attaching object's own `attached_to` and pushes
/// into the target's `attachments`, kept in sync in both directions. The
/// only writer of either field anywhere in this engine - see `GameObject`'s
/// own doc comment for why they were inert placeholder fields before
/// `phase5/aura-attach`. Silently no-ops if either id doesn't resolve to a
/// live object (shouldn't happen - callers only ever pass ids they just
/// confirmed exist), same "skip rather than guess" stance every other
/// unresolvable-id case in this codebase already takes.
///
/// CR 301.5c/702.6a: unlike an Aura (which is never re-targeted after it
/// attaches), an Equip activation can retarget an already-attached
/// Equipment to a different creature - if `attaching_id` is already
/// attached elsewhere, it's detached from the *old* target's own
/// `attachments` list first, so re-equipping doesn't leave a stale entry
/// behind (the same bidirectional-consistency concern `retire_object`'s own
/// cleanup step already takes seriously, just on the "still alive, just
/// moved" path instead of "departed").
pub(crate) fn attach_permanent(state: &mut GameState, attaching_id: ObjectId, target_id: ObjectId) {
    let previous_target = state.object(attaching_id).and_then(|o| o.attached_to);
    if let Some(old_target_id) = previous_target {
        if old_target_id != target_id {
            if let Some(old_target) = state.object_mut(old_target_id) {
                old_target.attachments.retain(|&id| id != attaching_id);
            }
        }
    }
    if let Some(attaching) = state.object_mut(attaching_id) {
        attaching.attached_to = Some(target_id);
        attaching.attached_to_player = None;
    }
    if let Some(target) = state.object_mut(target_id) {
        if !target.attachments.contains(&attaching_id) {
            target.attachments.push(attaching_id);
        }
    }
}

/// CR 303.4b's "or player" half of `attach_permanent` above — an Aura
/// enchanting a player instead of an object. No `attachments`-style
/// bidirectional list on `PlayerState`: nothing in this engine needs to
/// enumerate "every Aura attached to this player" yet (CR 303.4m's
/// "enchanted player" reference for continuous/replacement effects is a
/// separate, larger, deliberately deferred gap — this engine's continuous
/// effects only ever affect objects, not player-level static abilities), so
/// a reverse index would be pure unused plumbing. Only ever called for an
/// Aura (`commands::is_aura`) — Equipment/Fortification can never attach to
/// a player (CR 301.5a/301.6), so there's no re-target-away-from-a-player
/// case to handle here the way `attach_permanent`'s own reattach logic
/// does.
pub(crate) fn attach_permanent_to_player(state: &mut GameState, attaching_id: ObjectId, player: PlayerId) {
    if let Some(attaching) = state.object_mut(attaching_id) {
        attaching.attached_to_player = Some(player);
        attaching.attached_to = None;
    }
}

/// `propose_attach`'s result: what to do with `candidates` for one Aura's
/// attachment.
#[derive(Debug, PartialEq, Eq)]
pub enum AttachProposal {
    /// No candidate at all - nothing to attach (CR 303.4g/i: an Aura with no
    /// legal object/player stays wherever it is, or goes to the graveyard if
    /// it was on the stack - `propose_attach` itself doesn't perform either,
    /// the caller still owns that decision).
    None,
    /// Exactly one candidate - attach immediately, no choice needed.
    Attach(ObjectId),
    /// CR 303.4d: more than one simultaneous candidate for the same Aura -
    /// its controller must choose which one it becomes attached to.
    NeedsChoice(Vec<ObjectId>),
}

/// CR 303.4d's third clause: "An Aura can't enchant more than one object or
/// player. If a spell or ability would cause an Aura to become attached to
/// more than one object or player, the Aura's controller chooses which
/// object or player it becomes attached to." A pure decision function over
/// however many candidates the caller computed - `attach_permanent` itself
/// stays single-target (it always did; `GameObject.attached_to` is a plain
/// `Option<ObjectId>`, not a set, so there's no way to represent "attached
/// to two things" even transiently).
///
/// Dormant as of this slice: no effect, spell, or ability in this engine
/// ever computes more than one simultaneous attach candidate for one Aura -
/// both real call sites (`ResolveStackTop`'s Aura-ETB-attach, `Equip`'s own
/// resolution) already narrow to a single legal target via `TargetRequirement
/// { count: 1 }` before this would ever run. This function exists so the
/// rule is genuinely enforced *if* a future effect ever produces multiple
/// candidates, not because anything does today - exercised in
/// `phase5_aura_attach_choice.rs` by calling it directly with a
/// hand-built candidate list, the same "no real producer, tested by direct
/// construction" precedent `ProcessState::AwaitingAttachChoice` documents at
/// its own definition.
///
/// Player candidates (CR 303.4b: an Aura can also enchant a player) stay out
/// of scope for *this* dormant multi-candidate choice specifically (real
/// support for enchanting a player at all is `phase5/aura-enchants-a-player`
/// - see `attach_permanent_to_player`) - no effect in this engine ever
/// produces a mix of object *and* player candidates for one Aura, so
/// `candidates` here is still object ids only, same as before.
pub fn propose_attach(candidates: &[ObjectId]) -> AttachProposal {
    match candidates {
        [] => AttachProposal::None,
        [only] => AttachProposal::Attach(*only),
        _ => AttachProposal::NeedsChoice(candidates.to_vec()),
    }
}

pub(crate) fn retire_object(state: &mut GameState, old_id: ObjectId) -> Vec<RegisteredAbility> {
    let removed = state.objects.remove(&old_id);
    // CR 303.4b/301.5a: if old_id was itself attached to something (an Aura/
    // Equipment that just left play), drop it from that object's own
    // `attachments` list too - best-effort bidirectional cleanup, not
    // load-bearing for correctness. An object *attached to* old_id, if any,
    // is deliberately left with a stale `attached_to` here - CR 704.5m/
    // 704.5n's own SBA checks are what notice that lazily on the next pass,
    // same timing a real game has (an Aura doesn't fall off the instant its
    // enchanted creature dies, only once state-based actions are next
    // checked), not something to preempt.
    if let Some(target_id) = removed.and_then(|o| o.attached_to) {
        if let Some(target) = state.object_mut(target_id) {
            target.attachments.retain(|&id| id != old_id);
        }
    }
    let mut departed_triggered = Vec::new();
    state.abilities.retain(|a| {
        if a.source != old_id {
            return true;
        }
        if a.kind == AbilityKind::Triggered {
            departed_triggered.push(a.clone());
        }
        false
    });
    state.continuous_effects.retain(|e| e.source != old_id);
    // `phase5/static-regeneration`: `Game.CreateReplacementEffect` gave
    // `state.replacement_effects` its first-ever durable (`Repeatable`)
    // entry - every prior entry was `OneShot`/`LimitedUses` and
    // self-consumed via `apply_one_replacement`, so this list never needed
    // purging here before. Same pileup-avoidance rationale as
    // `continuous_effects` above (see this function's own doc comment).
    state.replacement_effects.retain(|r| r.source != old_id);
    departed_triggered
}

/// CR 800.4b: an eliminated player's turns and priority are skipped
/// entirely. Walks forward from `after` in `turn_order`, wrapping, until it
/// finds a player who hasn't left the game. Safe to call unconditionally —
/// `Engine` never lets fewer than 2 non-eliminated players remain without
/// constructing `GameOver` first (`finish_sba_loop`/`finish_cleanup_sba_check`
/// in engine.rs), so this is guaranteed to find one before wrapping all the
/// way back to `after` itself.
pub(crate) fn next_active_player(state: &GameState, after: PlayerId) -> PlayerId {
    let start_idx = state.turn_order.iter().position(|p| *p == after).unwrap_or(0);
    for offset in 1..=state.turn_order.len() {
        let idx = (start_idx + offset) % state.turn_order.len();
        let candidate = state.turn_order[idx];
        if !state.player(candidate).is_some_and(|p| p.eliminated) {
            return candidate;
        }
    }
    after
}

/// Every player still in the game, in `turn_order` order. Shared by the
/// "everyone passed" threshold (`consecutive_passes` needs to count only
/// players who can still be asked for priority) and the `GameOver` check
/// (CR 104.2a: last one standing wins; CR 104.4a: zero remaining is a draw).
pub(crate) fn non_eliminated_players(state: &GameState) -> Vec<PlayerId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|p| !state.player(*p).is_some_and(|pl| pl.eliminated))
        .collect()
}
