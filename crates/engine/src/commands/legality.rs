//! Sorcery-speed and legality predicates and target validation, shared by the
//! command bodies. See `DESIGN.md` for the rest.

use std::collections::HashSet;

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::card::{CardTypeSet, KeywordSet, SpellMode};
use crate::types::cost::FilterController;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::object::GameObject;
use crate::types::stack::StackEntryKind;
use crate::types::target::{ManaValueBound, ManaValueComparison, ManaValueRequirement, TargetFilter, TargetRequirement, TargetSpec};
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

use super::{non_eliminated_players, Rejected};

/// CR 117.1a sorcery-speed window: `player` may take a sorcery-speed action
/// (play a land, cast a sorcery, most activated abilities) only when they hold
/// priority, it is their own turn (CR 305.3), it is a main phase (CR 305.1) and
/// the stack is empty (CR 307.1/305.1). A standalone predicate because
/// `PlayLand`, `CastSpell` and `ActivateAbility`'s Equip check (CR 702.6a) share
/// the condition.
///
/// TODO: effects that grant timing permission beyond the base rule ("additional land", "as though it had flash", "as though it were a main phase") are not consulted, since nothing here reads state.continuous_effects or abilities for them (CR 113.6)
pub(in crate::commands) fn is_sorcery_speed(state: &GameState, player: PlayerId) -> bool {
    state.priority_player == player
        && state.active_player == player
        && state.phase == Phase::Main
        && state.stack.is_empty()
}

/// CR 302.6/602.5a summoning sickness: `object` is sick unless it has been under
/// its current controller's control since their most recent turn began. Derived
/// from `GameObject.controlled_since_turn` against
/// `GameState.last_active_turn_number` and not stored (see those fields). Haste
/// bypasses it (CR 702.10b/c). Checks the live controller
/// (`layers::effective_controller`), since CR 302.6 cares who controls the
/// creature now.
/// TODO: `controlled_since_turn` is not bumped by a `GainControl` effect
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
        // No recorded turn start is not evidence of long-standing control, so treat it
        // as sick. PreGame seeds this, so it should not happen past the first turn.
        None => true,
    }
}

/// CR 613 layer 6: `id`'s live keywords (printed plus anything granted by an
/// applicable `ContinuousEffect`), via `layers::effective_characteristics`.
/// Returns `KeywordSet::empty()` for a missing `id` where
/// `effective_characteristics` would panic, since callers reject a missing id
/// separately with `Rejected::InvalidTarget`.
pub(crate) fn effective_keywords(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> KeywordSet {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, script_runtime, id).keywords
    } else {
        KeywordSet::empty()
    }
}

/// CR 613.1f: `id`'s live `IgnoreKeyword` grants (see that primitive), which void
/// a keyword's restriction. A caller enforcing a keyword-driven restriction
/// (only Defender's attack check does) must combine this with
/// `effective_keywords`: the keyword must be present there and absent here. Same
/// missing-`id` fallback as `effective_keywords`.
pub(crate) fn ignored_keywords(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> KeywordSet {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, script_runtime, id).ignored_keywords
    } else {
        KeywordSet::empty()
    }
}

/// CR 510.1a/702.7: whether a creature with `keywords` deals combat damage during
/// `step` (`FirstStrikeDamage` or `CombatDamage`; `AssignCombatDamage`'s step
/// check rules out any other). `FIRST_STRIKE` and `DOUBLE_STRIKE` are
/// independent bits, so a double striker needs only `DOUBLE_STRIKE` to take part
/// in both steps.
pub(in crate::commands) fn participates_in_damage_step(keywords: KeywordSet, step: Step) -> bool {
    match step {
        Step::FirstStrikeDamage => keywords.intersects(KeywordSet::FIRST_STRIKE | KeywordSet::DOUBLE_STRIKE),
        _ => !keywords.contains(KeywordSet::FIRST_STRIKE) || keywords.contains(KeywordSet::DOUBLE_STRIKE),
    }
}

/// CR 613 layers 7a/7c: `id`'s live power, netted against counters, any
/// resolvable characteristic-defining base and any applicable `ModifyPT` or
/// `SetPT` effect, via `layers::effective_characteristics`. `None` for a missing
/// `id` or an unresolved characteristic-defining base, so callers skip rather
/// than guess and avoid its panic on a missing id.
pub(crate) fn effective_power(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> Option<i32> {
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

/// CR 702.19b: the minimum combat damage `AssignCombatDamage` must give
/// `blocker` before a trampling attacker may send excess to the defending
/// player: effective toughness minus damage already marked (from an earlier step
/// or a non-combat source this turn), floored at 0, and 1 if
/// `attacker_has_deathtouch` (CR 702.2b). An unresolved toughness reads as
/// `u32::MAX` ("needs everything"), so a trampler never claims a lethal
/// requirement it cannot compute.
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

/// CR 702.16c: whether `target_id` cannot be the target of `source` because it
/// has protection from one of: color (702.16e), card type, subtype, name
/// (702.16a), everything (702.16j) or a specific player (702.16k), as an OR over
/// all six. Protection is judged on `source`'s qualities (the card cast or the
/// activated ability's source), never the caster's. Two exceptions:
/// `protection_everything` applies regardless of the source's characteristics
/// (CR 702.16j), so it is checked first; and the player quality (CR 702.16k)
/// compares the source's raw `GameObject.controller` with `target_id`'s
/// controller. An unresolvable source has no qualities, so it cannot target
/// illegally on this basis.
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

/// CR 702.11b/702.18a: whether `id` cannot be chosen as a target by a spell or
/// ability controlled by `caster`, because of hexproof or shroud. Shroud stops
/// every `caster`, including the controller. Hexproof stops only a `caster` who
/// is not the permanent's `layers::effective_controller`. Comparing against
/// `caster` and not a source object's controller keeps it working for a trigger
/// whose source is gone. Keywords come from `effective_characteristics`, so
/// layer 6 grants and removals count.
///
/// Only permanents are covered (CR 702.11b/702.18a): a hexproof card in a
/// graveyard or a hexproof spell on the stack is still targetable.
///
/// TODO: "hexproof from [quality]" is not modeled (CR 702.11d-g)
/// TODO: players cannot have hexproof or shroud (CR 702.11c/702.18a)
/// TODO: nothing consumes `ignored_keywords` here, so "can target as though it didn't have hexproof" is not expressible (CR 702.11e)
/// TODO: no team model, so "opponent" means any other player
fn target_blocked_by_hexproof_or_shroud(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    id: ObjectId,
    caster: PlayerId,
) -> bool {
    let Some(obj) = state.object(id) else { return false };
    if obj.zone != ZoneKind::Battlefield {
        return false;
    }
    let keywords = crate::layers::effective_characteristics(state, script_runtime, id).keywords;
    if !keywords.intersects(KeywordSet::HEXPROOF | KeywordSet::SHROUD) {
        return false;
    }
    keywords.contains(KeywordSet::SHROUD)
        || crate::layers::effective_controller(state, script_runtime, id) != caster
}

/// Full target legality for a spell or ability that targets (cast,
/// activation, trigger candidate scan, CR 608.2b recheck): the filter, the
/// protection check and the hexproof/shroud check. Attachment legality
/// that isn't targeting uses `attachment_target_is_legal` instead.
pub(crate) fn target_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    target: Target,
    filter: &TargetFilter,
    caster: PlayerId,
    source: ObjectId,
    exclude_source: bool,
    caster_x_value: Option<u32>,
) -> bool {
    target_is_legal_impl(state, script_runtime, target, filter, caster, source, exclude_source, caster_x_value, true)
}

/// `target_is_legal` without the hexproof/shroud check, for "is this a legal
/// thing to attach to" and not "may this target it": the CR 704.5m/704.5n
/// rechecks of an attached Aura or Equipment and the CR 303.4f scan for an Aura
/// entering uncast. An Aura permanent does not target (CR 115.1b), so hexproof
/// and shroud do not apply. Protection still does (CR 702.16d).
pub(crate) fn attachment_target_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    target: Target,
    filter: &TargetFilter,
    caster: PlayerId,
    source: ObjectId,
    exclude_source: bool,
    caster_x_value: Option<u32>,
) -> bool {
    target_is_legal_impl(state, script_runtime, target, filter, caster, source, exclude_source, caster_x_value, false)
}

/// CR 115/601.2c-e: whether `target` satisfies `filter` against current `state`,
/// at cast or activation time and again at `ResolveStackTop` for the CR 608.2b
/// recheck. `caster` is the spell or ability's controller, since
/// `FilterController::You` and `Opponent` are relative to who casts or activates
/// and not to the target.
/// TODO: the zone-change half of CR 608.2b (CR 115.2) is not checked, because this asks only whether the target is legal now; the two coincide for every current filter since an `Object` filter's `zone` is fixed
fn target_is_legal_impl(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    target: Target,
    filter: &TargetFilter,
    caster: PlayerId,
    source: ObjectId,
    exclude_source: bool,
    // CR 202.3/107.3a
    // The casting or resolving spell's announced X (not the candidate's), consumed
    // only by `TargetFilter::Object::mana_value`'s `X` bound. `None` from callers
    // with no X context (an ability, an SBA recheck, a trigger candidate scan)
    // resolves as 0 (see `ManaValueBound::X`).
    caster_x_value: Option<u32>,
    // `false` skips CR 702.11b/702.18a; see `attachment_target_is_legal`.
    enforce_targetability: bool,
) -> bool {
    // CR 115.4
    // The source is never a legal candidate for a requirement that excludes it.
    // Checked once here and not in each match arm.
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
                // CR 202.3b
                // A candidate's mana value uses a real X only on the stack
                // (`StackObject::x_value`, set at cast time). Every other zone treats `{X}` as
                // 0, as in `ManaCost::mana_value`.
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
                // CR 601.2c/115
                // The negated form (a nonland target). Independent of `card_types` above, so a
                // filter can combine a positive card_types with an excluded_subtypes.
                if let Some(types) = excluded_card_types {
                    if effective.card_types.intersects(*types) {
                        return false;
                    }
                }
                // CR 205.4b
                // Matches if the target has any subtype in the list (or, not and, as in
                // cost.rs's FilterSpec::subtypes). Checked against effective_characteristics
                // like card_types above, not the raw CardDefinition.
                if let Some(subtypes) = subtypes {
                    if !subtypes.iter().any(|s| effective.subtypes.contains(s)) {
                        return false;
                    }
                }
                // CR 601.2c/115/205.4b
                // The negated form: rejects if the target has any subtype in the list.
                if let Some(excluded) = excluded_subtypes {
                    if excluded.iter().any(|s| effective.subtypes.contains(s)) {
                        return false;
                    }
                }
                // CR 205.4a
                // The same intersection against effective_characteristics as card_types,
                // mirroring cost.rs's FilterSpec::matches.
                if let Some(types) = supertypes {
                    if !effective.supertypes.intersects(*types) {
                        return false;
                    }
                }
                // CR 601.2c/115/205.4a
                // The negated form: rejects if the target's supertypes intersect this set.
                // Checked independently of `supertypes`, so a filter can combine both.
                if let Some(types) = excluded_supertypes {
                    if effective.supertypes.intersects(*types) {
                        return false;
                    }
                }
            }
            if let Some(fc) = controller {
                // CR 108.4
                // `You` and `Opponent` resolve against the live controller
                // (layers::effective_controller), not the raw GameObject.controller, so a
                // control-changed permanent follows its current controller.
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
            if enforce_targetability && target_blocked_by_hexproof_or_shroud(state, script_runtime, id, caster) {
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
                    && !(enforce_targetability
                        && target_blocked_by_hexproof_or_shroud(state, script_runtime, id, caster))
            }
            Target::Ability(_) => false,
        },
        // CR 601.2c/602.2b
        // `invoke_target_legal` (script.rs) is a per-candidate predicate like
        // `invoke_continuous_affects` for `AffectedFilter::Script`, run once per chosen
        // candidate. It is not a `Game.SelectTargets` coroutine suspension (see
        // `TargetFilter::Script`). Object-only; a nonexistent object is rejected before
        // the script runs, so Lua is never invoked on a dead id.
        TargetFilter::Script(handle) => {
            let Target::Object(id) = target else { return false };
            if state.object(id).is_none() {
                return false;
            }
            if target_has_protection_from(state, script_runtime, id, source) {
                return false;
            }
            if enforce_targetability && target_blocked_by_hexproof_or_shroud(state, script_runtime, id, caster) {
                return false;
            }
            script_runtime.invoke_target_legal(*handle, state, id, source, caster).unwrap_or(false)
        }
        // The player-candidate twin of `Script`: no protection check, because
        // player-side protection is not modeled, plus the script predicate.
        TargetFilter::PlayerScript(handle) => {
            let Target::Player(pid) = target else { return false };
            if state.player(pid).is_none() {
                return false;
            }
            script_runtime.invoke_player_target_legal(*handle, state, pid, source, caster).unwrap_or(false)
        }
        // CR 113.9/701.6a
        // Matched against state.stack directly, because an ability's stack_id never
        // appears in state.objects (see TargetFilter::Ability). A Spell entry is never
        // a candidate, since CR 113.9 keeps the categories disjoint.
        TargetFilter::Ability => {
            let Target::Ability(stack_id) = target else { return false };
            state.stack.iter().any(|so| so.stack_id == stack_id && is_ability_stack_entry(&so.kind))
        }
        // CR 702.21a: the triggering spell or ability, whichever kind it is.
        // Which one it is was settled when the trigger was put on the stack;
        // here it only has to still be there.
        TargetFilter::TriggeringStackObject => {
            let (Target::Object(id) | Target::Ability(id)) = target else { return false };
            state.stack.iter().any(|so| so.stack_id == id)
        }
    }
}

/// CR 707.10: a `StackEntryKind::Copy` of an activated/triggered ability is
/// that ability for targeting purposes too, so this unwraps through any
/// number of `Copy` layers rather than checking only the outermost kind.
pub(crate) fn is_ability_stack_entry(kind: &StackEntryKind) -> bool {
    match kind {
        StackEntryKind::ActivatedAbility | StackEntryKind::TriggeredAbility => true,
        StackEntryKind::Copy { of } => is_ability_stack_entry(of),
        StackEntryKind::Spell => false,
    }
}

/// CR 603.3d: every currently legal candidate for `requirement`, scanned fresh
/// like `attach::legal_aura_entry_candidates` but for any `TargetRequirement`.
/// `engine::triggers_pipeline`'s CR 603.3d pause uses it: nothing initiated the
/// trigger, so there are no player-submitted `targets` to validate and it must
/// present real candidates.
pub(crate) fn legal_target_candidates(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    requirement: &TargetRequirement,
    caster: PlayerId,
    source: ObjectId,
) -> Vec<Target> {
    legal_target_candidates_for_x(state, script_runtime, requirement, caster, source, false)
}

/// Whether the requirement's filter has a `mana_value` bound of `X`, so its
/// candidates depend on the X the caster has yet to announce.
pub(crate) fn requirement_depends_on_x(requirement: &TargetRequirement) -> bool {
    matches!(
        requirement.filter,
        TargetFilter::Object { mana_value: Some(ManaValueRequirement { bound: ManaValueBound::X, .. }), .. }
    )
}

/// `legal_target_candidates`, or with `any_x` every candidate that is legal
/// for some announced X: an entry is listed before the caster announces X, so
/// the `X` bound of a `mana_value` filter is dropped (CR 107.3a, 601.2b-c).
/// The engine's own check against the announced X rejects a mismatched pair.
pub(crate) fn legal_target_candidates_for_x(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    requirement: &TargetRequirement,
    caster: PlayerId,
    source: ObjectId,
    any_x: bool,
) -> Vec<Target> {
    let mut relaxed;
    let requirement = if any_x && requirement_depends_on_x(requirement) {
        relaxed = requirement.clone();
        if let TargetFilter::Object { mana_value, .. } = &mut relaxed.filter {
            *mana_value = None;
        }
        &relaxed
    } else {
        requirement
    };
    // Sorted by id: `state.objects` is a `HashMap`, and a candidate list that
    // follows its order differs from run to run.
    let mut object_ids: Vec<ObjectId> = state.objects.keys().copied().collect();
    object_ids.sort();
    let object_candidates = object_ids.into_iter().map(Target::Object);
    let player_candidates = non_eliminated_players(state).into_iter().map(Target::Player);
    // CR 113.9
    // An ability's StackObject has no matching GameObject, so it needs its own
    // candidate source (see TargetFilter::Ability).
    let ability_candidates = state
        .stack
        .iter()
        .filter(|so| is_ability_stack_entry(&so.kind))
        .map(|so| Target::Ability(so.stack_id));
    object_candidates
        .chain(player_candidates)
        .chain(ability_candidates)
        .filter(|&target| {
            // CR 603.3d
            // A triggered ability has no CastSpell to announce an X, so `None` (a
            // `mana_value` filter's X bound resolves as 0; see `target_is_legal`).
            target_is_legal(state, script_runtime, target, &requirement.filter, caster, source, requirement.exclude_source, None)
        })
        .collect()
}

/// CR 603.3d for a full `TargetSpec`: one candidate list per target word
/// (`Vec<Vec<Target>>`, length 1 for `Primitive`), via `legal_target_candidates`
/// per word. `engine::triggers_pipeline`'s pause uses it because a triggered
/// ability's `target` is a full `TargetSpec`, and a word with too few legal
/// candidates for its minimum makes the whole trigger illegal (CR 601.2c-d).
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

/// The minimum legal-candidate count for each word in `spec`: `0` for `up_to`,
/// `count` otherwise (`at_least` has the same lower bound). Paired by position
/// with `legal_target_candidates_for_spec` so
/// `engine::triggers_pipeline::push_ordered_triggers` can check every word has
/// enough candidates before pausing, or else remove the ability from the stack
/// (CR 603.3d).
pub(crate) fn target_spec_minimums(spec: &TargetSpec) -> Vec<u32> {
    let requirements: Vec<&TargetRequirement> = match spec {
        TargetSpec::Primitive(req) => vec![req],
        TargetSpec::Multiple(reqs) => reqs.iter().collect(),
    };
    requirements.into_iter().map(|req| if req.up_to { 0 } else { req.count }).collect()
}

/// CR 601.2c/601.2e (spells) and CR 602.2b (activated abilities): validates the
/// chosen `targets` against `requirement` before the spell or ability goes on
/// the stack. An illegal cast or activation never happens (CR 601.2e), so this
/// runs before any mutation in `CastSpell`/`ActivateAbility` and its `Err`
/// leaves nothing touched.
///
/// A `None` requirement means no targets (CR 601.2c); submitted targets are
/// rejected and not ignored.
///
/// CR 601.2c/115.6: `up_to` accepts 0 to `count`, `at_least` accepts `count` or
/// more. See `TargetRequirement`.
///
/// `pub(crate)` because `engine::triggers_pipeline`'s pause
/// (`Engine::submit_trigger_targets`) validates a triggered ability's chosen
/// targets here at answer time, from outside the `commands` tree.
/// TODO: no division of an effect among several targets (CR 601.2d)
pub(crate) fn validate_targets(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    requirement: Option<&TargetSpec>,
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
    // CR 202.3/107.3a
    // This cast or activation's announced X (see `target_is_legal`). `None` from
    // callers with no X, such as a triggered ability's CR 603.3d target choice.
    caster_x_value: Option<u32>,
) -> Result<(), Rejected> {
    match requirement {
        None => {
            if targets.is_empty() {
                Ok(())
            } else {
                Err(Rejected::InvalidTarget)
            }
        }
        Some(TargetSpec::Primitive(req)) => {
            validate_one_target_requirement(state, script_runtime, req, targets, caster, source, caster_x_value)
        }
        Some(TargetSpec::Multiple(reqs)) => {
            // CR 601.2c
            // A flat submitted-targets list split into one chunk per requirement:
            // positionally for 0 or 1 variable words, by filter assignment for 2 or more
            // (see `split_targets_by_requirement`).
            let Some(chunks) = split_targets_by_requirement(state, script_runtime, reqs, targets, caster, source, caster_x_value) else {
                return Err(Rejected::InvalidTarget);
            };
            // CR 115.3/601.2c
            // Dedup is checked per word by `validate_one_target_requirement` (fresh `seen`
            // set each call), so the same object or player may legally appear in two
            // words' chunks.
            for (req, chunk) in reqs.iter().zip(chunks) {
                validate_one_target_requirement(state, script_runtime, req, &chunk, caster, source, caster_x_value)?;
            }
            Ok(())
        }
    }
}

/// One target word's validation (CR 601.2c count, CR 115.3/601.2c same-instance
/// dedup, per-target legality), shared by the `Primitive` arm (whole list) and
/// the `Multiple` arm (one chunk per requirement) of `validate_targets`.
fn validate_one_target_requirement(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    req: &TargetRequirement,
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
    caster_x_value: Option<u32>,
) -> Result<(), Rejected> {
    // CR 601.2c/115.6
    // `up_to` accepts 0 to `count`, `at_least` accepts `count` or more, and the
    // exact case requires `count`.
    let count_ok = if req.up_to {
        targets.len() <= req.count as usize
    } else if req.at_least {
        targets.len() >= req.count as usize
    } else {
        targets.len() == req.count as usize
    };
    if !count_ok {
        return Err(Rejected::InvalidTarget);
    }
    // CR 115.3/601.2c
    // A target cannot repeat within one target word.
    let mut seen = HashSet::new();
    for t in targets {
        if !seen.insert(*t) {
            return Err(Rejected::InvalidTarget);
        }
    }
    for t in targets {
        if !target_is_legal(state, script_runtime, *t, &req.filter, caster, source, req.exclude_source, caster_x_value) {
            return Err(Rejected::InvalidTarget);
        }
    }
    Ok(())
}

/// CR 601.2c/608.2b: splits `targets` into one chunk per entry of `reqs`. An
/// exact-count entry (`up_to` and `at_least` both false) claims exactly
/// `req.count`. At most one variable entry (`up_to` or `at_least`) is allowed on
/// the fast positional path. `PlayerAction` carries one flat `targets` list, so
/// the variable count is derived as `submitted.len() - sum(exact counts)`,
/// wherever that entry sits. Returns `None` if `targets` is too short for the
/// exact entries, the derived count falls outside the entry's bound, or a
/// remainder is left over (a mismatch is rejected, not guessed).
///
/// Two or more variable entries fall through to `assign_targets_by_filter`: a
/// flat list has no positional signal for a shared remainder (up to two target
/// creatures and up to two target players with 2 targets submitted could be 2+0,
/// 1+1 or 0+2), so it resolves by filter matching and needs `state`,
/// `script_runtime`, `caster` and `source`. The fast path stays a pure function
/// of `reqs` and `targets`.
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
                    // A second variable entry: hand off to filter assignment instead of guessing.
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

/// CR 601.2c, for 2 or more variable-count words: assigns each submitted target
/// to a word whose `TargetFilter` it satisfies (via `target_is_legal`),
/// backtracking until every word's bound (exact, `up_to` or `at_least`) is met
/// and every target is used once.
///
/// It accepts any valid assignment, not only a unique one. CR 601.2c has the
/// player state which word each target fills, but every multi-word
/// `AbilityEffect` here resolves across the flattened concatenation of all
/// words' targets (see `TargetSpec::Multiple`, CR 608.2b), so the filing has no
/// effect at resolution and rejecting an ambiguous one would be pickier than the
/// engine's semantics need.
///
/// Plain backtracking, not bipartite matching, since target counts are tiny (a
/// handful of words and targets).
fn assign_targets_by_filter(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    reqs: &[TargetRequirement],
    targets: &[Target],
    caster: PlayerId,
    source: ObjectId,
    caster_x_value: Option<u32>,
) -> Option<Vec<Vec<Target>>> {
    // Precompute which words each submitted target is legal for; the search below
    // branches only over these.
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
            // Prune: an exact or up_to word can never exceed its max.
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

/// CR 700.2d: can `effect` be one of 2+ chosen modes? `ResolveStackTop`'s
/// `AbilityEffect::Modal` arm (`engine::resolve::modal`) hands each mode to the
/// resolver a single effect uses, so this is every kind that resolver handles
/// against a mode's target requirement and the standard `ReplacementCheck` tail:
/// synchronous effects, scripted zone moves (with their CR 616.1 ordering
/// pauses), draw, discard, scry/surveil, search (with devour/amplify entry
/// choices), token creation (with the Aura attach choice) and "unless paid" (the
/// payer's answer). Checked at cast time so an unsupported mode is rejected and
/// not silently ignored at resolution. A single chosen mode is never checked
/// here, because it uses its own effect unwrapped from `Modal`.
///
/// `ForEachPlayer`, `Cascade`, `MadnessCastOrDiscard`, `ReturnSelfWithCounter`,
/// `AddMana*`, `Script` and a nested `Modal` are not card-text modes and are
/// rejected. A mode may own several target words (a `TargetSpec::Multiple`, such
/// as a two-word `Fight` or `ExchangeTextBoxes`); see `AbilityEffect::Modal`.
///
/// Callers outside this module ask `mode_can_join_others` instead.
pub(crate) fn is_modal_resolvable(effect: &AbilityEffect) -> bool {
    matches!(
        effect,
        // Synchronous.
        AbilityEffect::GainLife(_)
            | AbilityEffect::Damage(_)
            | AbilityEffect::Counters { .. }
            | AbilityEffect::Pump { .. }
            | AbilityEffect::GainControl(_)
            | AbilityEffect::Shuffle
            | AbilityEffect::Regenerate
            | AbilityEffect::LoseLife(_)
            | AbilityEffect::Tap
            | AbilityEffect::Untap
            | AbilityEffect::TapSelf
            | AbilityEffect::UntapSelf
            | AbilityEffect::TapAll { .. }
            | AbilityEffect::UntapAll { .. }
            | AbilityEffect::Counter
            | AbilityEffect::Fight
            | AbilityEffect::Bite
            // Scripted zone moves.
            | AbilityEffect::Sacrifice
            | AbilityEffect::SacrificeSelf
            | AbilityEffect::SacrificeRandom { .. }
            | AbilityEffect::Destroy
            | AbilityEffect::DestroyAll(_)
            | AbilityEffect::Exile
            | AbilityEffect::ExileSelf
            | AbilityEffect::ExileAll { .. }
            | AbilityEffect::ExileUntilSourceLeaves
            | AbilityEffect::ReturnToHand
            | AbilityEffect::ReturnSelfToHand
            | AbilityEffect::ReturnAllToHand { .. }
            | AbilityEffect::ReturnToBattlefield { .. }
            | AbilityEffect::ReturnAllToBattlefield { .. }
            | AbilityEffect::Mill(_)
            // Library and hand effects that pause for a draw replacement or
            // a player choice.
            | AbilityEffect::DrawCards(_)
            | AbilityEffect::Discard { .. }
            | AbilityEffect::DiscardRandom { .. }
            | AbilityEffect::Scry(_)
            | AbilityEffect::Surveil(_)
            | AbilityEffect::Search { .. }
            | AbilityEffect::CreateToken { .. }
            | AbilityEffect::UnlessPaid { .. }
            // A mode with several target words (`TargetSpec::Multiple`).
            | AbilityEffect::ExchangeTextBoxes
            // Act on the ability's own source, so only a modal triggered
            // ability can use them (a spell's source is not a permanent).
            | AbilityEffect::BecomeCopy(_)
            | AbilityEffect::Attach
    )
}

/// Why `mode` can only be chosen alone, or `None` when it can join other chosen
/// modes. Two modes that each pass can always combine: target words are
/// deduplicated per word, so no joint check is needed.
///
/// TODO: `is_modal_resolvable` refuses `ForEachPlayer`, `Cascade`, `MadnessCastOrDiscard`, `ReturnSelfWithCounter`, `AddMana*`, `Script` and a nested `Modal`, and each kind the resolver learns shrinks the set of solo-only modes
/// TODO: an empty `TargetSpec::Multiple` is a card-definition mistake that a load-time check should reject
fn mode_join_refusal(mode: &SpellMode) -> Option<&'static str> {
    if matches!(&mode.target_requirement, Some(TargetSpec::Multiple(own)) if own.is_empty()) {
        return Some("a mode with an empty multi-target requirement can't be combined");
    }
    match &mode.effect {
        Some(effect) if !is_modal_resolvable(effect) => Some(
            "this mode's effect can't be combined with other chosen modes yet - see \
             is_modal_resolvable for the supported kinds",
        ),
        _ => None,
    }
}

/// Whether `mode` can be one of two or more chosen modes (CR 700.2). The
/// rules allow any pair, so a `false` here is a gap in the resolver and not a
/// rule. The cast, the modal triggers, the opponent's choice and the entries
/// all ask this one question.
pub(crate) fn mode_can_join_others(mode: &SpellMode) -> bool {
    mode_join_refusal(mode).is_none()
}

/// The ascending indices of the modes that can only be chosen alone.
pub(crate) fn solo_only_modes(modes: &[SpellMode]) -> Vec<u32> {
    modes
        .iter()
        .enumerate()
        .filter(|(_, mode)| !mode_can_join_others(mode))
        .map(|(index, _)| index as u32)
        .collect()
}

/// The `TargetRequirement` `combine_chosen_modes` puts in a `Multiple` slot
/// for a mode that has none of its own (CR 700.2c): zero targets of anything.
fn modal_placeholder_requirement() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 0,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }
}

/// Whether `req` is the CR 700.2c placeholder, i.e. the mode declared no
/// targets, so a resolver should see no target requirement at all.
pub(crate) fn is_modal_placeholder_requirement(req: &TargetRequirement) -> bool {
    req.count == 0
        && !req.up_to
        && !req.at_least
        && !req.exclude_source
        && matches!(req.filter, TargetFilter::AnyTarget)
}

/// CR 700.2c/d: combines `modes[chosen_indices]` (already validated for count and
/// duplicates by the caller) into the one `(target_requirement, effect)` pair
/// `ResolveStackTop` resolves. A single chosen mode passes its own
/// `target_requirement` and `effect` through bare. Two or more combine into
/// `TargetSpec::Multiple` (one `TargetRequirement` per mode, with a zero-count
/// placeholder for a mode that has none, CR 700.2c) and `AbilityEffect::Modal`
/// (one slot per mode, same order); see `AbilityEffect::Modal` for the shape
/// `ResolveStackTop` relies on.
///
/// Shared by the caster-chosen `CastSpell` branch and a modal triggered ability
/// (`Engine::submit_trigger_mode_choice`, CR 603.3c), so both use one set of
/// combination rules. `chosen_indices` must already be in card-print order. It
/// does not collect CR 700.2h additional costs (`SpellMode::additional_cost`):
/// those belong to casting (CR 601.2b/f-h) and mean nothing for a triggered
/// ability, so `CastSpell` collects them around this call.
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
    for mode in &chosen_modes {
        if let Some(reason) = mode_join_refusal(mode) {
            return Err(reason.to_string());
        }
    }

    // One flat list of target words; `words[i]` is how many of them mode i
    // owns (see `AbilityEffect::Modal`).
    let mut reqs = Vec::with_capacity(chosen_modes.len());
    let mut words = Vec::with_capacity(chosen_modes.len());
    for mode in &chosen_modes {
        match &mode.target_requirement {
            None => {
                reqs.push(modal_placeholder_requirement());
                words.push(1);
            }
            Some(TargetSpec::Primitive(req)) => {
                reqs.push(req.clone());
                words.push(1);
            }
            Some(TargetSpec::Multiple(own)) => {
                reqs.extend(own.iter().cloned());
                words.push(own.len());
            }
        }
    }
    let effects: Vec<Option<AbilityEffect>> = chosen_modes.iter().map(|m| m.effect.clone()).collect();
    Ok((Some(TargetSpec::Multiple(reqs)), Some(AbilityEffect::Modal { effects, words })))
}
