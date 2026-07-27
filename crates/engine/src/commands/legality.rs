//! Sorcery-speed/legality helper predicates and target validation, shared by
//! `execute_command`'s match arms (`land`/`cast`/`activate`/`combat`
//! submodules) - split out of the original monolithic `commands.rs`.

use std::collections::HashSet;

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::cost::FilterController;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::object::GameObject;
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

use super::CommandError;

/// CR 117.1a's "sorcery speed" window — `player` may take an action that
/// requires it (playing a land, casting a sorcery, most activated
/// abilities) only when all of: they hold priority, it's their own turn
/// (CR 305.3), it's a main phase of that turn (CR 305.1), and the stack is
/// empty (CR 307.1/305.1). Pulled out as a standalone predicate rather than
/// inlined into `PlayLand` alone, since `CastSpell`'s sorcery-timing branch
/// and `ActivateAbility`'s own CR 702.6a check (Equip's "activate only as a
/// sorcery," `phase5/equip-ability`) both need the exact same condition.
///
/// TODO: doesn't account for effects that grant timing permission beyond
/// the base rule (e.g. "you may play an additional land," "you may cast
/// this spell as though it had flash," "you may play lands as though it
/// were a main phase") - those are all CR 113.6-style rules-text
/// permissions layered on top of this base check, and nothing here
/// consults state.continuous_effects/abilities for them yet. Revisit once
/// such an effect needs representing.
pub(in crate::commands) fn is_sorcery_speed(state: &GameState, player: PlayerId) -> bool {
    state.priority_player == player
        && state.active_player == player
        && state.phase == Phase::Main
        && state.stack.is_empty()
}

/// CR 302.6/602.5a's summoning sickness check: `object` is sick unless it's
/// been under its current controller's control since their most recent
/// turn began. Derived from `GameObject.controlled_since_turn` against
/// `GameState.last_active_turn_number` rather than a stored bool - see
/// those fields' doc comments for why. CR 702.10b/c: Haste bypasses this
/// check entirely. Checks against `object`'s *live* controller
/// (`layers::effective_controller`), not the raw field - CR 302.6 cares who
/// controls the creature right now, which matters the moment a
/// `GainControl` effect exists (`controlled_since_turn` itself still isn't
/// bumped by one - see this slice's own UNRESOLVED note).
pub(crate) fn has_summoning_sickness(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: &GameObject,
) -> bool {
    if effective_keywords(state, script_runtime, object.object_id).contains(KeywordSet::HASTE) {
        return false;
    }
    let controller = crate::layers::effective_controller(state, script_runtime, object.object_id);
    match state.last_active_turn_number.get(&controller) {
        Some(&since) => object.controlled_since_turn >= since,
        // The controller has no recorded turn start at all - shouldn't
        // happen past the very first turn (PreGame seeds this), but "no
        // record" isn't evidence of long-standing control either, so treat
        // it as sick rather than silently allowing it.
        None => true,
    }
}

/// CR 613 layer 6: `id`'s live keywords (printed plus anything granted by an
/// applicable `ContinuousEffect`), via `layers::effective_characteristics`.
/// Tolerates a missing `id` (returns `KeywordSet::empty()`) rather than
/// panicking the way `effective_characteristics` itself does — several call
/// sites below already separately raise `CommandError::InvalidTarget` for a
/// missing id and just want "no keywords" as the fallback otherwise (mirrors
/// the pre-existing `Option<&GameObject>` + `is_some_and` shape this
/// replaces).
pub(crate) fn effective_keywords(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> KeywordSet {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, script_runtime, id).keywords
    } else {
        KeywordSet::empty()
    }
}

/// CR 510.1a/702.7: whether a creature with `keywords` assigns/deals combat
/// damage during `step` (`FirstStrikeDamage` or `CombatDamage` - any other
/// `Step` value is unreachable, gated by `AssignCombatDamage`'s own
/// phase/step check above). `FIRST_STRIKE`/`DOUBLE_STRIKE` are independent
/// bits - a double striker only needs `DOUBLE_STRIKE` set to take part in
/// both steps, it doesn't also need `FIRST_STRIKE`.
pub(in crate::commands) fn participates_in_damage_step(keywords: KeywordSet, step: Step) -> bool {
    match step {
        Step::FirstStrikeDamage => keywords.intersects(KeywordSet::FIRST_STRIKE | KeywordSet::DOUBLE_STRIKE),
        _ => !keywords.contains(KeywordSet::FIRST_STRIKE) || keywords.contains(KeywordSet::DOUBLE_STRIKE),
    }
}

/// CR 613 layers 7a/7c: `id`'s live power (already netted against counters,
/// any resolvable characteristic-defining base, and any applicable
/// `ModifyPT`/`SetPT` continuous effect), via `layers::effective_characteristics`.
/// `None` for a missing `id` or an unresolved characteristic-defining base
/// value — same "skip rather than guess" contract `effective_characteristics`
/// itself already documents, just reachable without risking its
/// panic-on-missing-id path.
pub(in crate::commands) fn effective_power(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> Option<i32> {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, script_runtime, id).power
    } else {
        None
    }
}

/// Same as `effective_power` above, for toughness — used by trample's
/// lethal-damage check (CR 702.19b).
fn effective_toughness(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> Option<i32> {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, script_runtime, id).toughness
    } else {
        None
    }
}

/// CR 702.19b: the minimum amount of combat damage `AssignCombatDamage` must
/// give `blocker` before a trampling attacker may send any excess to the
/// defending player — effective toughness minus damage already marked (from
/// an earlier step, e.g. first strike, or a non-combat source this turn),
/// floored at 0, netted down to 1 if `attacker_has_deathtouch` (CR 702.2b:
/// any nonzero deathtouch damage is lethal regardless of toughness). An
/// unresolved toughness (missing CDA script, same "skip rather than guess"
/// stance as `effective_power`) reads as "needs everything" (`u32::MAX`) so
/// a trampler can never claim to have satisfied a lethal requirement it
/// can't actually compute — safer than silently treating unknown as 0.
pub(in crate::commands) fn lethal_damage_needed(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    blocker: ObjectId,
    attacker_has_deathtouch: bool,
) -> u32 {
    let Some(toughness) = effective_toughness(state, script_runtime, blocker) else {
        return u32::MAX;
    };
    let already_marked = state.object(blocker).map(|o| o.damage_marked).unwrap_or(0);
    let remaining = (toughness as i64 - already_marked as i64).max(0);
    if remaining == 0 {
        0
    } else if attacker_has_deathtouch {
        1
    } else {
        remaining as u32
    }
}

/// CR 115/601.2c-e: whether `target` satisfies `filter`, evaluated against
/// current `state` (used both at cast/activation time and, unchanged, again
/// at `ResolveStackTop` for the CR 608.2b recheck — legality is defined the
/// same way both times, only *when* it's asked differs). `caster` is the
/// spell/ability's controller, since `FilterController::You`/`Opponent` are
/// relative to whoever is casting/activating, not the target's own
/// perspective.
///
/// TODO: `CR 115.2`'s "no longer in the zone it was in" half of 608.2b isn't
/// checked here — this only asks "is it legal *now*", not "is it in the
/// zone it was originally targeted in". The two coincide for every filter
/// this slice builds (an `Object` filter's `zone` is fixed, so a target that
/// changed zones already fails the zone check below), but a future filter
/// that doesn't pin a zone would need the original zone threaded through
/// separately to catch that case.
/// CR 702.16c: `target_id` can't be the target of a spell/ability matching
/// a quality it has protection from — color (702.16e), card type (702.16),
/// subtype (702.16), name (702.16a), "everything" (702.16j), or a specific
/// player (702.16k), checked as an OR across all six. `source` is the
/// spell/ability's own object id (the card being cast, or the source of an
/// activated ability) — its color/card types/subtypes/name, not the
/// caster's, is what protection cares about (CR 702.16's "of the stated
/// quality" always describes the source, never the controller) — except
/// `protection_everything`, which CR 702.16j is explicit applies
/// "regardless of that object's characteristic values," so it's checked
/// before the source's own characteristics are even looked up. The player
/// quality is the one exception to "source, not caster": CR 702.16k cares
/// about the source's *controller* (raw `GameObject.controller`, resolved
/// relative to `target_id`'s own controller, same "You"/"Opponent"
/// resolution `EffectFilter` already does), not any characteristic. An
/// unresolvable source has no qualities to check against and can't
/// illegally target anything on this basis — same "skip rather than guess"
/// stance every other unresolvable-source case in this codebase already
/// takes.
fn target_has_protection_from(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    target_id: ObjectId,
    source: ObjectId,
) -> bool {
    let Some(source_obj) = state.object(source) else {
        return false;
    };
    let target_characteristics = crate::layers::effective_characteristics(state, script_runtime, target_id);
    if target_characteristics.protection_everything {
        return true;
    }
    if !target_characteristics.protection_players.is_empty() {
        let target_controller = state.object(target_id).map(|o| o.controller);
        if target_controller.is_some_and(|target_controller| {
            target_characteristics.protection_players.iter().any(|who| match who {
                FilterController::You => source_obj.controller == target_controller,
                FilterController::Opponent => source_obj.controller != target_controller,
                FilterController::Any => false,
            })
        }) {
            return true;
        }
    }
    if target_characteristics.protection_colors.is_empty()
        && target_characteristics.protection_card_types.is_empty()
        && target_characteristics.protection_subtypes.is_empty()
        && target_characteristics.protection_names.is_empty()
    {
        return false;
    }
    let source_characteristics = crate::layers::effective_characteristics(state, script_runtime, source);
    target_characteristics.protection_colors.intersects(source_characteristics.colors)
        || target_characteristics.protection_card_types.intersects(source_characteristics.card_types)
        || target_characteristics
            .protection_subtypes
            .iter()
            .any(|s| source_characteristics.subtypes.contains(s))
        || target_characteristics.protection_names.iter().any(|n| *n == source_characteristics.name)
}

pub(crate) fn target_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    target: Target,
    filter: &TargetFilter,
    caster: PlayerId,
    source: ObjectId,
    exclude_source: bool,
) -> bool {
    // CR 115.4/"another target": the source is never a legal candidate for
    // a requirement that says so, regardless of what the rest of `filter`
    // would otherwise allow - checked once, ahead of every filter variant,
    // rather than duplicated in each match arm below.
    if exclude_source && target == Target::Object(source) {
        return false;
    }
    match filter {
        TargetFilter::Player => matches!(target, Target::Player(pid) if state.player(pid).is_some()),
        TargetFilter::Object { card_types, excluded_card_types, subtypes, excluded_subtypes, supertypes, excluded_supertypes, controller, zone } => {
            let Target::Object(id) = target else { return false };
            let Some(obj) = state.object(id) else { return false };
            if obj.zone != zone.unwrap_or(ZoneKind::Battlefield) {
                return false;
            }
            if card_types.is_some()
                || excluded_card_types.is_some()
                || subtypes.is_some()
                || excluded_subtypes.is_some()
                || supertypes.is_some()
                || excluded_supertypes.is_some()
            {
                let effective = crate::layers::effective_characteristics(state, script_runtime, id);
                if let Some(types) = card_types {
                    if !effective.card_types.intersects(*types) {
                        return false;
                    }
                }
                // CR 601.2c/115's negated form - "target nonland permanent."
                // Independent of `card_types` above (a filter can combine a
                // positive card_types with an excluded_subtypes, e.g.
                // "target creature that isn't a Wall").
                if let Some(types) = excluded_card_types {
                    if effective.card_types.intersects(*types) {
                        return false;
                    }
                }
                // CR 205.4b: "target Human"/"Equip Dragons" - matches if the
                // target has *any* subtype in the list, same "or, not and"
                // convention cost.rs's FilterSpec::subtypes already
                // documents. Checked against effective_characteristics
                // (post layer-4 AddCardType-adjacent subtype grants, if any
                // ever exist), same treatment card_types just above already
                // gets - not the raw CardDefinition.
                if let Some(subtypes) = subtypes {
                    if !subtypes.iter().any(|s| effective.subtypes.contains(s)) {
                        return false;
                    }
                }
                // CR 601.2c/115/205.4b's negated form - "target creature
                // that isn't a Wall." Rejects if the target has *any*
                // subtype in the list (the negation of subtypes' "or"
                // match above).
                if let Some(excluded) = excluded_subtypes {
                    if excluded.iter().any(|s| effective.subtypes.contains(s)) {
                        return false;
                    }
                }
                // CR 205.4a: "target legendary creature" - same
                // intersection-against-effective-characteristics treatment
                // card_types gets, mirroring cost.rs's FilterSpec::matches.
                if let Some(types) = supertypes {
                    if !effective.supertypes.intersects(*types) {
                        return false;
                    }
                }
                // CR 601.2c/115/205.4a's negated form - "target nonlegendary
                // creature." Rejects if the target's supertypes intersect
                // this set, the negation of supertypes' match above, checked
                // independently of it (same "can combine positive and
                // negative" stance excluded_subtypes already documents).
                if let Some(types) = excluded_supertypes {
                    if effective.supertypes.intersects(*types) {
                        return false;
                    }
                }
            }
            if let Some(fc) = controller {
                // CR 108.4: "you"/"opponent" resolve against the object's
                // live controller (layers::effective_controller), not the
                // raw GameObject.controller field - a control-changed
                // permanent must be targetable/untargetable per its current
                // controller, not whoever it started under.
                let effective_controller = crate::layers::effective_controller(state, script_runtime, id);
                let matches_controller = match fc {
                    FilterController::You => effective_controller == caster,
                    FilterController::Opponent => effective_controller != caster,
                    FilterController::Any => true,
                };
                if !matches_controller {
                    return false;
                }
            }
            if target_has_protection_from(state, script_runtime, id, source) {
                return false;
            }
            true
        }
        // CR 115.4: creature, player, planeswalker, or battle only - no
        // noncreature artifacts, no spells/abilities on the stack.
        TargetFilter::AnyTarget => match target {
            Target::Player(pid) => state.player(pid).is_some(),
            Target::Object(id) => {
                let Some(obj) = state.object(id) else { return false };
                if obj.zone != ZoneKind::Battlefield {
                    return false;
                }
                let effective = crate::layers::effective_characteristics(state, script_runtime, id);
                if !effective.card_types.intersects(
                    CardTypeSet::CREATURE | CardTypeSet::PLANESWALKER | CardTypeSet::BATTLE,
                ) {
                    return false;
                }
                !target_has_protection_from(state, script_runtime, id, source)
            }
        },
        // CR 601.2c/602.2b: `invoke_target_legal`'s real body (script.rs)
        // is the same per-candidate predicate shape `invoke_continuous_affects`
        // established for `AffectedFilter::Script` — evaluated once per
        // already-chosen candidate, not a `Game.SelectTargets` coroutine
        // suspension (that's a different, unrelated concept - see
        // `TargetFilter::Script`'s own doc comment). Object-only per that
        // doc comment; a nonexistent object is rejected before the script
        // ever runs, same "don't invoke Lua on a dead id" guard the other
        // arms get via `state.object(id)`.
        TargetFilter::Script(handle) => {
            let Target::Object(id) = target else { return false };
            if state.object(id).is_none() {
                return false;
            }
            if target_has_protection_from(state, script_runtime, id, source) {
                return false;
            }
            script_runtime.invoke_target_legal(*handle, state, id, source, caster).unwrap_or(false)
        }
        // `Script`'s player-candidate twin - same treatment `Player` above
        // gets (no protection check, this engine doesn't model player-side
        // protection), plus the script predicate.
        TargetFilter::PlayerScript(handle) => {
            let Target::Player(pid) = target else { return false };
            if state.player(pid).is_none() {
                return false;
            }
            script_runtime.invoke_player_target_legal(*handle, state, pid, source, caster).unwrap_or(false)
        }
    }
}

/// CR 601.2c/601.2e (spells) and CR 602.2b (activated abilities): validates
/// a full set of chosen `targets` against `requirement` before the
/// spell/ability is allowed onto the stack at all - an illegal proposed
/// cast/activation never happens (601.2e), so this is called before any
/// state mutation in `CastSpell`/`ActivateAbility`, and its `Err` propagates
/// straight out without anything having been touched.
///
/// `None` requirement means the spell/ability takes no targets at all
/// (CR 601.2c's "cast as though it did not require those targets") - any
/// targets submitted anyway are rejected rather than silently ignored,
/// since nothing asked for them.
///
/// CR 601.2c/115.6: "up to N" (`TargetRequirement::up_to`) accepts anywhere
/// from 0 to `count` chosen; "N or more" (`TargetRequirement::at_least`)
/// accepts `count` or more chosen, no upper bound - see `TargetRequirement`'s
/// own doc for the full three-way shape.
/// TODO(CR 601.2d): no division/distribution step for effects that split
/// among several targets.
pub(in crate::commands) fn validate_targets(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    requirement: Option<&TargetSpec>,
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
) -> Result<(), CommandError> {
    match requirement {
        None => {
            if targets.is_empty() {
                Ok(())
            } else {
                Err(CommandError::InvalidTarget)
            }
        }
        Some(TargetSpec::Primitive(req)) => {
            validate_one_target_requirement(state, script_runtime, req, targets, caster, source)
        }
        Some(TargetSpec::Multiple(reqs)) => {
            // CR 601.2c: a flat submitted-targets list, split into one
            // contiguous chunk per requirement in order - see
            // `TargetSpec::Multiple`'s own doc comment for why every
            // requirement here must use an exact count.
            let Some(chunks) = split_targets_by_requirement(reqs, targets) else {
                return Err(CommandError::InvalidTarget);
            };
            // CR 115.3/601.2c: dedup is checked independently per word by
            // `validate_one_target_requirement` below (its own `seen` set is
            // fresh each call) - the same object/player legally can, and
            // often will, appear in two different words' chunks.
            for (req, chunk) in reqs.iter().zip(chunks) {
                validate_one_target_requirement(state, script_runtime, req, chunk, caster, source)?;
            }
            Ok(())
        }
    }
}

/// One target word's worth of validation (CR 601.2c count check, CR
/// 115.3/601.2c same-instance dedup, per-target legality) - shared by
/// `validate_targets`'s `Primitive` arm (the whole submitted list) and its
/// `Multiple` arm (one chunk per requirement).
fn validate_one_target_requirement(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    req: &TargetRequirement,
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
) -> Result<(), CommandError> {
    // CR 601.2c/115.6: "up to N" accepts anywhere from 0 to `count`
    // chosen; "N or more" accepts `count` or more with no upper
    // bound; the common exact-count case still requires exactly
    // `count`.
    let count_ok = if req.up_to {
        targets.len() <= req.count as usize
    } else if req.at_least {
        targets.len() >= req.count as usize
    } else {
        targets.len() == req.count as usize
    };
    if !count_ok {
        return Err(CommandError::InvalidTarget);
    }
    // CR 115.3/601.2c: the same target can't be chosen twice for one
    // instance of the word "target".
    let mut seen = HashSet::new();
    for t in targets {
        if !seen.insert(*t) {
            return Err(CommandError::InvalidTarget);
        }
    }
    for t in targets {
        if !target_is_legal(state, script_runtime, *t, &req.filter, caster, source, req.exclude_source) {
            return Err(CommandError::InvalidTarget);
        }
    }
    Ok(())
}

/// CR 601.2c/608.2b: splits `targets` into one contiguous slice per entry of
/// `reqs`, in order. Every entry with an exact count (`up_to`/`at_least`
/// both `false`) always claims exactly `req.count`. At most one entry may be
/// variable (`up_to` or `at_least`) - its own count isn't announced
/// separately anywhere in this engine's `PlayerAction` shape (just one flat
/// `targets: Vec<Target>`), so it's derived as "however many are left after
/// every exact-count entry has claimed its own share": `submitted.len() -
/// sum(exact counts)`, wherever the variable entry sits in word order (the
/// derivation doesn't depend on position, only on the total). Two or more
/// variable entries stay a real, unsupported ambiguity - given only a flat
/// list, nothing says how a shared remainder splits between them (e.g. "up
/// to two target creatures and up to two target players" with 2 targets
/// submitted could mean 2+0, 1+1, or 0+2) - `None` rather than guessing.
/// Also `None` if `targets` is too short to cover every exact entry, if the
/// derived variable count falls outside that entry's own `up_to`/`at_least`
/// bound, or if there's a leftover remainder after every entry has claimed
/// its chunk (a caller/count mismatch, same "reject rather than guess"
/// stance every other validation failure here takes).
pub(crate) fn split_targets_by_requirement<'a>(
    reqs: &[TargetRequirement],
    targets: &'a [Target],
) -> Option<Vec<&'a [Target]>> {
    let variable_index = {
        let mut found = None;
        for (i, req) in reqs.iter().enumerate() {
            if req.up_to || req.at_least {
                if found.is_some() {
                    // A second variable entry - ambiguous, see this
                    // function's own doc comment.
                    return None;
                }
                found = Some(i);
            }
        }
        found
    };
    let exact_total: usize =
        reqs.iter().filter(|r| !(r.up_to || r.at_least)).map(|r| r.count as usize).sum();
    if targets.len() < exact_total {
        return None;
    }
    let remainder = targets.len() - exact_total;
    if let Some(i) = variable_index {
        let req = &reqs[i];
        let bound_ok =
            if req.up_to { remainder <= req.count as usize } else { remainder >= req.count as usize };
        if !bound_ok {
            return None;
        }
    }

    let mut rest = targets;
    let mut chunks = Vec::with_capacity(reqs.len());
    for (i, req) in reqs.iter().enumerate() {
        let n = if Some(i) == variable_index { remainder } else { req.count as usize };
        if rest.len() < n {
            return None;
        }
        let (chunk, remaining) = rest.split_at(n);
        chunks.push(chunk);
        rest = remaining;
    }
    if rest.is_empty() {
        Some(chunks)
    } else {
        None
    }
}

/// CR 700.2d: is `effect` one of the synchronous, never-pausing
/// `AbilityEffect` kinds `ResolveStackTop`'s `AbilityEffect::Modal` arm knows
/// how to resolve? Checked at cast time (`execute_command`'s `CastSpell` arm)
/// so combining an unsupported mode is rejected immediately rather than
/// silently doing nothing at resolution. Narrowed deliberately: every kind
/// excluded here either pauses mid-resolution (`Destroy`/`Sacrifice`/`Exile`/
/// `DrawCards`/`Search`/`BecomeCopy`/`CreateToken`) or has its own bespoke
/// per-target-pairing shape (`ExchangeTextBoxes`/`Attach`) that a generic
/// per-mode chunk loop can't drive - combining 2+ such modes into one
/// resolution would need real coordination between multiple simultaneous
/// pauses, out of scope for this slice. A single chosen mode (the pre-
/// existing "Choose one —" case) is never checked against this at all - it
/// uses its own effect bare, unwrapped from `Modal`, exactly as before.
///
/// `pub(crate)` since CR 700.2e's `Engine::submit_mode_choice` (engine.rs)
/// needs this same check once a deferred chosen-by-opponent choice finally
/// names its modes, mirroring this function's own call site here.
pub(crate) fn is_modal_resolvable(effect: &AbilityEffect) -> bool {
    matches!(
        effect,
        AbilityEffect::GainLife(_)
            | AbilityEffect::Damage(_)
            | AbilityEffect::Counters { .. }
            | AbilityEffect::Pump { .. }
            | AbilityEffect::GainControl(_)
            | AbilityEffect::Shuffle
    )
}
