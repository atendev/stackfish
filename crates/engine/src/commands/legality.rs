//! Sorcery-speed/legality helper predicates and target validation, shared by
//! `execute_command`'s match arms (`land`/`cast`/`activate`/`combat`
//! submodules) - split out of the original monolithic `commands.rs`.

use std::collections::HashSet;

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::card::{CardTypeSet, KeywordSet, SpellMode};
use crate::types::cost::FilterController;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::object::GameObject;
use crate::types::target::{ManaValueBound, ManaValueComparison, TargetFilter, TargetRequirement, TargetSpec};
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

use super::{non_eliminated_players, CommandError};

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

/// CR 613.1f "as though it didn't have [keyword]" text: `id`'s live
/// `IgnoreKeyword` grants (see that primitive's own doc comment).
/// A caller enforcing a keyword-driven restriction (today, only the
/// Defender attack-declaration check) must combine this with
/// `effective_keywords` above — the keyword still has to be present in
/// `effective_keywords` (this doesn't imply the keyword itself), just with
/// its restriction voided if it's also present here. Same missing-`id`
/// fallback as `effective_keywords`.
pub(crate) fn ignored_keywords(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> KeywordSet {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, script_runtime, id).ignored_keywords
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
    // CR 202.3/107.3a: the *casting/resolving* spell's own announced X
    // (`StackObject::x_value`/`CastSpell::x_value`, not the candidate's own)
    // — only consumed by `TargetFilter::Object::mana_value`'s `X` bound.
    // `None` from every caller with no live X context (an ability, an SBA
    // recheck, a triggered-ability candidate scan) resolves as `0`, same as
    // "no X announced" — see `ManaValueBound::X`'s own doc comment.
    caster_x_value: Option<u32>,
) -> bool {
    // CR 115.4/"another target": the source is never a legal candidate for
    // a requirement that says so, regardless of what the rest of `filter`
    // would otherwise allow - checked once, ahead of every filter variant,
    // rather than duplicated in each match arm below.
    if exclude_source && target == Target::Object(source) {
        return false;
    }
    match filter {
        TargetFilter::Player { controller } => {
            let Target::Player(pid) = target else { return false };
            if state.player(pid).is_none() {
                return false;
            }
            match controller {
                None => true,
                Some(FilterController::You) => pid == caster,
                Some(FilterController::Opponent) => pid != caster,
                Some(FilterController::Any) => true,
            }
        }
        TargetFilter::Object { card_types, excluded_card_types, subtypes, excluded_subtypes, supertypes, excluded_supertypes, controller, zone, mana_value } => {
            let Target::Object(id) = target else { return false };
            let Some(obj) = state.object(id) else { return false };
            if obj.zone != zone.unwrap_or(ZoneKind::Battlefield) {
                return false;
            }
            if let Some(req) = mana_value {
                // CR 202.3b: a candidate's own mana value only uses a real X
                // while it's on the stack (`StackObject::x_value`, resolved
                // at that spell's own cast time) - every other zone treats
                // its `{X}` as 0, same off-the-stack default `ManaCost::
                // mana_value`'s own doc comment already establishes.
                let candidate_x =
                    state.stack.iter().find(|so| so.stack_id == id).and_then(|so| so.x_value).unwrap_or(0);
                let candidate_mv = obj.definition.mana_cost.resolve_x(candidate_x).mana_value();
                let bound = match req.bound {
                    ManaValueBound::Fixed(n) => n,
                    ManaValueBound::X => caster_x_value.unwrap_or(0),
                };
                let matches_mana_value = match req.comparison {
                    ManaValueComparison::Equal => candidate_mv == bound,
                    ManaValueComparison::AtMost => candidate_mv <= bound,
                    ManaValueComparison::AtLeast => candidate_mv >= bound,
                };
                if !matches_mana_value {
                    return false;
                }
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

/// CR 603.3d: every currently-legal candidate for `requirement`, scanned
/// fresh - the same "walk every object/player, filter through
/// `target_is_legal`" shape `attach::legal_aura_entry_candidates` already
/// established, generalized off any `TargetRequirement` instead of just an
/// Aura's own `target_requirement`. Used by `engine::triggers_pipeline`'s CR
/// 603.3d pause, which has no player-submitted `targets` to validate at all
/// (nothing initiated the trigger the way a cast/activation does) - it has
/// to present real candidates instead of guessing blind.
pub(crate) fn legal_target_candidates(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    requirement: &TargetRequirement,
    caster: PlayerId,
    source: ObjectId,
) -> Vec<Target> {
    let object_candidates = state.objects.values().map(|obj| Target::Object(obj.object_id));
    let player_candidates = non_eliminated_players(state).into_iter().map(Target::Player);
    object_candidates
        .chain(player_candidates)
        .filter(|&target| {
            // CR 603.3d candidate scan for a *triggered* ability - no
            // PlayerActionKind::CastSpell exists here to have announced an X
            // at all, so `None` (a `mana_value` filter's own `X` bound
            // resolves as 0 - see `target_is_legal`'s own doc comment).
            target_is_legal(state, script_runtime, target, &requirement.filter, caster, source, requirement.exclude_source, None)
        })
        .collect()
}

/// CR 603.3d, generalized to a full `TargetSpec` (one candidate list per
/// target word - `Vec<Vec<Target>>`, length 1 for `Primitive`): every
/// currently-legal candidate for each word in `spec`, via
/// `legal_target_candidates` per word. `engine::triggers_pipeline`'s own CR
/// 603.3d pause uses this (rather than the single-`TargetRequirement`
/// version) since a triggered ability's own `target` field is a full
/// `TargetSpec` just like a spell/activated ability's, and CR 601.2c/603.3d
/// (via 601.2c-d) already establish "a word with zero legal candidates for
/// its own minimum makes the whole cast/trigger illegal" for the
/// multi-word case too, not just the single-word one.
pub(crate) fn legal_target_candidates_for_spec(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    spec: &TargetSpec,
    caster: PlayerId,
    source: ObjectId,
) -> Vec<Vec<Target>> {
    let requirements: Vec<&TargetRequirement> = match spec {
        TargetSpec::Primitive(req) => vec![req],
        TargetSpec::Multiple(reqs) => reqs.iter().collect(),
    };
    requirements.into_iter().map(|req| legal_target_candidates(state, script_runtime, req, caster, source)).collect()
}

/// One minimum-legal-candidate-count per word in `spec` (`0` for `up_to`,
/// `count` otherwise - `at_least`'s own lower bound is still just `count`,
/// same as `up_to`/exact) - paired positionally with
/// `legal_target_candidates_for_spec`'s own per-word output so
/// `engine::triggers_pipeline::push_ordered_triggers` can check "does every
/// word have enough real candidates" before deciding to pause at all versus
/// applying CR 603.3d's "the ability is simply removed from the stack."
pub(crate) fn target_spec_minimums(spec: &TargetSpec) -> Vec<u32> {
    let requirements: Vec<&TargetRequirement> = match spec {
        TargetSpec::Primitive(req) => vec![req],
        TargetSpec::Multiple(reqs) => reqs.iter().collect(),
    };
    requirements.into_iter().map(|req| if req.up_to { 0 } else { req.count }).collect()
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
///
/// `pub(crate)`, not `pub(in crate::commands)`: `engine::triggers_pipeline`'s
/// own CR 603.3d pause (`Engine::submit_trigger_targets`) needs this too, and
/// is outside the `commands` module tree - a triggered ability's target
/// choice has no `CastSpell`/`ActivateAbility`-style player-submitted
/// `targets` to validate at the point it's initiated, so it validates here
/// instead, at answer time.
pub(crate) fn validate_targets(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    requirement: Option<&TargetSpec>,
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
    // CR 202.3/107.3a: this cast/activation's own announced X, if any - see
    // `target_is_legal`'s own doc comment. `None` from every caller with no
    // X concept (a triggered ability's own CR 603.3d target choice never
    // has one).
    caster_x_value: Option<u32>,
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
            validate_one_target_requirement(state, script_runtime, req, targets, caster, source, caster_x_value)
        }
        Some(TargetSpec::Multiple(reqs)) => {
            // CR 601.2c: a flat submitted-targets list, split into one
            // chunk per requirement - positionally for 0/1 variable words,
            // by filter-assignment for 2+ (see `split_targets_by_
            // requirement`'s own doc comment).
            let Some(chunks) = split_targets_by_requirement(state, script_runtime, reqs, targets, caster, source, caster_x_value) else {
                return Err(CommandError::InvalidTarget);
            };
            // CR 115.3/601.2c: dedup is checked independently per word by
            // `validate_one_target_requirement` below (its own `seen` set is
            // fresh each call) - the same object/player legally can, and
            // often will, appear in two different words' chunks.
            for (req, chunk) in reqs.iter().zip(chunks) {
                validate_one_target_requirement(state, script_runtime, req, &chunk, caster, source, caster_x_value)?;
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
    caster_x_value: Option<u32>,
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
        if !target_is_legal(state, script_runtime, *t, &req.filter, caster, source, req.exclude_source, caster_x_value) {
            return Err(CommandError::InvalidTarget);
        }
    }
    Ok(())
}

/// CR 601.2c/608.2b: splits `targets` into one chunk per entry of `reqs`.
/// Every entry with an exact count (`up_to`/`at_least` both `false`) always
/// claims exactly `req.count`. At most one entry may be variable (`up_to` or
/// `at_least`) for the fast positional path below - its own count isn't
/// announced separately anywhere in this engine's `PlayerAction` shape (just
/// one flat `targets: Vec<Target>`), so it's derived as "however many are
/// left after every exact-count entry has claimed its own share":
/// `submitted.len() - sum(exact counts)`, wherever the variable entry sits in
/// word order (the derivation doesn't depend on position, only on the
/// total). `None` if `targets` is too short to cover every exact entry, if
/// the derived variable count falls outside that entry's own `up_to`/
/// `at_least` bound, or if there's a leftover remainder after every entry
/// has claimed its chunk (a caller/count mismatch, same "reject rather than
/// guess" stance every other validation failure here takes).
///
/// Two or more variable entries (`phase5/multi-word-variable-count-filter-
/// assignment`) fall through to `assign_targets_by_filter` instead - a flat
/// list has no positional signal to derive a *shared* remainder's split at
/// all (e.g. "up to two target creatures and up to two target players" with
/// 2 targets submitted could mean 2+0, 1+1, or 0+2 by position alone), so
/// this needs `state`/`script_runtime`/`caster`/`source` to resolve it by
/// filter-matching instead - unused by the fast positional path, which
/// remains a pure function of `reqs`/`targets` for the common case.
pub(crate) fn split_targets_by_requirement(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    reqs: &[TargetRequirement],
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
    caster_x_value: Option<u32>,
) -> Option<Vec<Vec<Target>>> {
    let variable_index = {
        let mut found = None;
        for (i, req) in reqs.iter().enumerate() {
            if req.up_to || req.at_least {
                if found.is_some() {
                    // A second variable entry - hand off to the
                    // filter-assignment path instead of guessing.
                    return assign_targets_by_filter(state, script_runtime, reqs, targets, caster, source, caster_x_value);
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
        chunks.push(chunk.to_vec());
        rest = remaining;
    }
    if rest.is_empty() {
        Some(chunks)
    } else {
        None
    }
}

/// CR 601.2c, for 2+ variable-count words (`phase5/multi-word-variable-
/// count-filter-assignment`): assigns each entry of `targets` to a word
/// whose own `TargetFilter` it satisfies (the same `target_is_legal` check
/// every single-word/exact-word validation already uses), backtracking over
/// the small search space until every word's own count bound (exact/up_to/
/// at_least) is met and every submitted entry is used exactly once.
///
/// Deliberately accepts *any* valid assignment rather than requiring a
/// *unique* one. Real CR 601.2c has the player state the word-target
/// association explicitly as part of announcing targets, but every
/// multi-word spell/ability `AbilityEffect` in this engine applies its
/// resolution uniformly across the *flattened concatenation* of every
/// word's own legal targets (see `TargetSpec::Multiple`'s own doc comment,
/// CR 608.2b's Plague Spores example) - which word a target is filed under
/// has no resolution-time consequence here, only a legality-checking one.
/// So "some valid partition exists" is the right bar for this engine's own
/// model, not "the partition is uniquely determined" - there is nothing
/// downstream that could ever observe the difference between two otherwise-
/// identical casts that differ only in which word a shared-eligible target
/// was filed under, so rejecting one as "ambiguous" would only be pickier
/// than the engine's own semantics actually require.
///
/// Plain backtracking, not a real max-flow/bipartite-matching
/// implementation - simple on purpose. Every real and test-fixture target
/// count in this engine is tiny (a handful of words, a handful of targets);
/// this is not the right place to optimize for scale that doesn't exist
/// here.
fn assign_targets_by_filter(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    reqs: &[TargetRequirement],
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
    caster_x_value: Option<u32>,
) -> Option<Vec<Vec<Target>>> {
    // Precompute, once, which words each submitted target is even legal
    // for - the backtracking search below only ever branches over these.
    let legal_words: Vec<Vec<usize>> = targets
        .iter()
        .map(|t| {
            reqs.iter()
                .enumerate()
                .filter(|(_, req)| target_is_legal(state, script_runtime, *t, &req.filter, caster, source, req.exclude_source, caster_x_value))
                .map(|(i, _)| i)
                .collect()
        })
        .collect();

    fn backtrack(idx: usize, targets: &[Target], reqs: &[TargetRequirement], legal_words: &[Vec<usize>], buckets: &mut [Vec<Target>]) -> bool {
        if idx == targets.len() {
            return reqs.iter().enumerate().all(|(w, req)| {
                let n = buckets[w].len();
                if req.up_to {
                    n <= req.count as usize
                } else if req.at_least {
                    n >= req.count as usize
                } else {
                    n == req.count as usize
                }
            });
        }
        for &w in &legal_words[idx] {
            // Prune: an exact/up_to word can never exceed its own max -
            // no point recursing into a branch that's already overfull.
            let max = if reqs[w].at_least { usize::MAX } else { reqs[w].count as usize };
            if buckets[w].len() >= max {
                continue;
            }
            buckets[w].push(targets[idx]);
            if backtrack(idx + 1, targets, reqs, legal_words, buckets) {
                return true;
            }
            buckets[w].pop();
        }
        false
    }

    let mut buckets: Vec<Vec<Target>> = vec![Vec::new(); reqs.len()];
    if backtrack(0, targets, reqs, &legal_words, &mut buckets) {
        Some(buckets)
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

/// CR 700.2c/d: combines `modes[chosen_indices]` (already validated for
/// count/duplicates by the caller) into the one `(target_requirement,
/// effect)` pair `ResolveStackTop` resolves — a single chosen mode passes its
/// own `target_requirement`/`effect` through bare (the pre-existing "Choose
/// one —" shape), 2+ chosen modes combine into `TargetSpec::Multiple` (one
/// `TargetRequirement` per chosen mode, a synthetic zero-count placeholder
/// for a mode with none of its own, CR 700.2c) + `AbilityEffect::Modal` (one
/// slot per chosen mode, same order) — see `AbilityEffect::Modal`'s own doc
/// comment for the shape invariant `ResolveStackTop`'s own `Modal` arm relies
/// on.
///
/// Extracted out of `commands::cast::execute_command`'s `CastSpell` caster-
/// chosen branch so a modal *triggered* ability (`Engine::
/// submit_trigger_mode_choice`, CR 603.3c) can reuse the exact same
/// combination rules instead of a third hand-written copy — `chosen_indices`
/// is assumed already sorted into card-print order by the caller, same
/// convention `CastSpell`'s own caller already followed. Doesn't collect CR
/// 700.2h additional costs (`SpellMode::additional_cost`) — that's a
/// casting-cost concept tied to CR 601.2b/f-h, meaningless for a triggered
/// ability going on the stack (CR 603.3c has no equivalent clause), so the
/// caster-chosen `CastSpell` branch still collects it separately around this
/// call rather than folding it in here.
pub(crate) fn combine_chosen_modes(
    modes: &[SpellMode],
    chosen_indices: &[u32],
) -> Result<(Option<TargetSpec>, Option<AbilityEffect>), String> {
    let mut chosen_modes = Vec::with_capacity(chosen_indices.len());
    for &idx in chosen_indices {
        let Some(mode) = modes.get(idx as usize) else {
            return Err("no such mode".to_string());
        };
        chosen_modes.push(mode);
    }

    if let [only] = chosen_modes.as_slice() {
        return Ok((only.target_requirement.clone(), only.effect.clone()));
    }

    let mut reqs = Vec::with_capacity(chosen_modes.len());
    for mode in &chosen_modes {
        match &mode.target_requirement {
            None => reqs.push(TargetRequirement {
                filter: TargetFilter::AnyTarget,
                count: 0,
                up_to: false,
                at_least: false,
                exclude_source: false,
            }),
            Some(TargetSpec::Primitive(req)) => reqs.push(req.clone()),
            Some(TargetSpec::Multiple(_)) => {
                return Err(
                    "a mode with its own multi-target requirement can't be combined with other \
                     chosen modes"
                        .to_string(),
                );
            }
        }
    }
    for mode in &chosen_modes {
        if let Some(effect) = &mode.effect {
            if !is_modal_resolvable(effect) {
                return Err(
                    "this mode's effect can't be combined with other chosen modes yet - only \
                     GainLife/Damage/Counters/Pump/GainControl/Shuffle are supported inside a \
                     multi-mode choice"
                        .to_string(),
                );
            }
        }
    }
    let effects: Vec<Option<AbilityEffect>> = chosen_modes.iter().map(|m| m.effect.clone()).collect();
    Ok((Some(TargetSpec::Multiple(reqs)), Some(AbilityEffect::Modal(effects))))
}
