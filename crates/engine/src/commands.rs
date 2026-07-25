use std::collections::{HashMap, HashSet};

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::ability::AbilityKind;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ModeChoice};
use crate::types::combat::CombatState;
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::cost::{self, AdditionalCost, AlternativeCost, CostPayment, CostSpec, FilterController};
use crate::types::counter::CounterKind;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{AbilityId, ObjectId, ObjectLineage, PlayerId, ScriptHandle, Target};
use crate::types::mana::{ColorSet, ManaUnit};
use crate::types::object::GameObject;
use crate::types::stack::{PendingModeChoice, StackEntryKind, StackObject};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

/// Carrying out an already-validated `EngineCommand` failed partway —
/// distinct from `ActionError` (engine.rs), which rejects an action before
/// it's even accepted. Reaching a `CommandError` mid-execution should be
/// rare/exceptional once cost/target validation happens up front; this
/// exists mainly for cases validation can't fully rule out ahead of time.
#[derive(Debug)]
pub enum CommandError {
    CostNotPaid,
    InvalidTarget,
    IllegalAction(String),
}

/// Whether/how priority should move as a result of one `EngineCommand`.
/// Returned explicitly by `execute_command` rather than the command
/// mutating `state.priority_player` itself, so `engine.rs` is the one
/// place that actually writes that field — same reasoning as
/// `CommandOutcome::broke_pass_chain` not mutating `consecutive_passes`
/// directly.
pub enum PriorityHandoff {
    /// Most variants: priority stays with whoever currently has it.
    Unchanged,
    /// `PassPriority` (CR 117.3c): priority moves to the next player in
    /// `state.turn_order`. Carries the resolved target player rather than
    /// engine.rs re-deriving "next" from `turn_order`, so `execute_command`
    /// (which already has `state` to hand) stays the single place that
    /// knows the rotation rule.
    To(PlayerId),
}

/// What carrying out one `EngineCommand` produced, for `engine.rs` to act
/// on. Returned explicitly rather than left for `engine.rs` to infer from
/// `state` afterward — see `broke_pass_chain`/`priority_handoff` below.
pub struct CommandOutcome {
    /// Proposed events, not yet final — `engine.rs` runs these through
    /// `ReplacementCheck` -> `CommitEvents` to allocate ids and append to
    /// `state.ledger`. See `execute_command`'s doc comment for why this
    /// function doesn't write the ledger itself.
    pub events: Vec<GameEventKind>,
    /// `false` only for `PassPriority`; `true` for every other variant.
    /// CR 117.3c: taking any action other than passing breaks a
    /// consecutive-pass count. `engine.rs` owns `state.consecutive_passes`
    /// entirely (increment on `false`, reset on `true`) and decides "did
    /// everyone just pass" from that — this flag is the one thing it needs
    /// from execution to do so, rather than relying on every match arm
    /// here remembering to mutate `consecutive_passes` itself correctly.
    pub broke_pass_chain: bool,
    /// See `PriorityHandoff`. `engine.rs` applies this to
    /// `state.priority_player` itself.
    pub priority_handoff: PriorityHandoff,
    /// CR 601.2a puts a spell on the stack *before* its cost is determined/
    /// paid (601.2f-h) — set by `CastSpell` once it's pushed `stack_id` onto
    /// `state.stack`, so `engine.rs` can pause the rest of this pipeline run
    /// on `ProcessState::AwaitingPayment` until `Engine::submit_payment`
    /// answers, rather than this function deducting mana synchronously
    /// (which would assume the cost is always payable/free, and skip the
    /// CR 118.5a "must explicitly acknowledge a {0} cost" requirement).
    /// `None` for every other variant — nothing else here puts something on
    /// the stack pending payment.
    pub requires_payment: Option<PendingPaymentRequest>,
    /// Object ids that just became new battlefield permanents this command
    /// (CR 400.7 mints a fresh id per zone change) — `engine.rs` runs each
    /// one's card script (`ScriptRuntime::run_card_script`) right after this
    /// outcome is applied. Empty for every variant except `PlayLand`, since
    /// `execute_command` (`state: &mut GameState` only) can't reach
    /// `ScriptRuntime` itself — same "one place decides, one place performs
    /// the side effect" split `priority_handoff`/`broke_pass_chain` already
    /// use. `ResolveStackTop`'s permanent-entry branch lives directly in
    /// `engine.rs`, so it calls `run_card_script` itself without needing
    /// this field.
    pub battlefield_entries: Vec<ObjectId>,
}

/// See `CommandOutcome::requires_payment`'s doc comment for why this exists
/// as its own pause rather than a synchronous deduction.
pub struct PendingPaymentRequest {
    pub stack_id: ObjectId,
    pub payer: PlayerId,
    pub cost: CostSpec,
    /// The object whose activation/cast this cost belongs to —
    /// `AdditionalCost::TapSelf` validates against this specifically (CR
    /// 602.5a: an activated ability's own `{T}` always taps *this* object,
    /// never a filter choice). For `CastSpell`, this is the spell's own
    /// `stack_id` — harmless as a placeholder since a spell's `CostSpec`
    /// never contains `TapSelf`.
    pub source: ObjectId,
    /// What to do once this cost fully commits — see `PaymentResolution`'s
    /// own doc comment. Almost always `StackObject` (a real `StackObject`
    /// is already sitting on `state.stack` at `stack_id`, waiting for normal
    /// stack resolution) — `ManaAbility` is the one exception (CR
    /// 605.1a/605.3b: a mana ability with a real cost still needs an
    /// explicit payment step, same as any other ability, but never reaches
    /// the stack at all).
    pub on_paid: PaymentResolution,
}

/// See `PendingPaymentRequest::on_paid`'s doc comment. `stack_id` on the
/// enclosing `PendingPaymentRequest` is only a real, resolvable `StackObject`
/// id for the `StackObject` case — for `ManaAbility` it's an opaque handle
/// (freshly allocated, CR 400.7-style) that nothing on `state.stack` will
/// ever match, since a mana ability with a real cost still pauses for
/// payment exactly like any other ability, but nothing is ever pushed onto
/// the stack for it to resolve later.
pub enum PaymentResolution {
    StackObject,
    /// CR 605's mana units to add once this cost commits — already fully
    /// resolved (a fixed `AddMana` list, or an `AddManaChoice`'s
    /// `chosen_colors` already validated and converted) at command-execution
    /// time, same as the free (no-cost) mana-ability path already did before
    /// this existed; paying a cost doesn't change *which* mana comes out.
    ManaAbility { controller: PlayerId, units: Vec<ManaUnit> },
}

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
fn is_sorcery_speed(state: &GameState, player: PlayerId) -> bool {
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
fn participates_in_damage_step(keywords: KeywordSet, step: Step) -> bool {
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
fn effective_power(state: &GameState, script_runtime: &ScriptRuntime, id: ObjectId) -> Option<i32> {
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
fn lethal_damage_needed(
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
fn validate_targets(
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

/// CR 303.4f: every object currently a legal enchant target for `aura`'s own
/// enchant ability (`aura_definition.target_requirement`), evaluated fresh
/// against `state` right now - unlike the normal cast-time path, no target
/// has been chosen yet, so this scans every object in `state` rather than
/// rechecking one. Reuses `target_is_legal` (same filter/legality logic the
/// cast-time path already consults) once per candidate; player candidates
/// (CR 303.4b) are out of scope, same posture `phase5/aura-attach-choice`
/// already took for the identical reason (this engine has no support for an
/// Aura enchanting a player at all yet). `aura_source` both supplies CR
/// 115.4's "another target" check (`exclude_source`) and is excluded from
/// the scan implicitly by `aura_source` not yet existing in `state.objects`
/// at the point this is called (see `SearchDestination::Battlefield`'s call
/// site - the Aura's own not-yet-inserted `ObjectId` is passed here before
/// any zone move happens, so it can never appear as a candidate of itself;
/// CR 303.4d self-enchant only matters once the Aura already exists on the
/// battlefield, which this narrow entry path doesn't produce). No `Aura`
/// carrying a `target_requirement` other than `TargetSpec::Primitive` (i.e.
/// none, this engine never builds one) returns no candidates rather than
/// panicking - same "skip rather than guess" stance every other
/// unresolvable-requirement case in this codebase already takes.
pub(crate) fn legal_aura_entry_candidates(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    aura_definition: &CardDefinition,
    controller: PlayerId,
    aura_source: ObjectId,
) -> Vec<ObjectId> {
    let Some(TargetSpec::Primitive(req)) = &aura_definition.target_requirement else {
        return Vec::new();
    };
    state
        .objects
        .values()
        .filter(|obj| {
            target_is_legal(state, script_runtime, Target::Object(obj.object_id), &req.filter, controller, aura_source, req.exclude_source)
        })
        .map(|obj| obj.object_id)
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
) -> Option<Vec<ObjectId>> {
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
            .map(|obj| obj.object_id)
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
    }
    if let Some(target) = state.object_mut(target_id) {
        if !target.attachments.contains(&attaching_id) {
            target.attachments.push(attaching_id);
        }
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
/// Player candidates (CR 303.4b: an Aura can also enchant a player) are out
/// of scope - `attached_to`/`attach_permanent` are `ObjectId`-only, since
/// this engine has no support at all yet for an Aura enchanting a player, a
/// separate and larger pre-existing gap. `candidates` here is therefore
/// object ids only.
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

/// CR 601.2h/616.1/514.1: a card moving to a graveyard - whether via a
/// `Sacrifice`/`Discard` cost payment or CR 514.1's cleanup hand-size
/// discard - is itself a replaceable zone-change proposal ("if a permanent
/// you control would be sacrificed, exile it instead" is a real, if
/// unusual, card pattern). This builds the *proposed* event only, mirroring
/// `sba::resolve_deaths`'/`Engine::continue_scripted_zone_move`'s own
/// "propose, then replacement-check, then commit" split rather than
/// mutating directly the way this function's predecessor
/// (`move_to_graveyard_for_cost`, folded into `commit_payment` itself) used
/// to. Two callers: `Engine::continue_cost_zone_moves` (cost payments) and
/// `Engine::continue_discard_moves` (cleanup discard, `phase5/replaceable-
/// cleanup-discard`) - see either's own doc comment for its full pipeline.
pub(crate) fn propose_zone_move(state: &GameState, id: ObjectId, from: ZoneKind) -> Option<GameEventKind> {
    let last_known_info = state.object(id)?.clone();
    Some(GameEventKind::ZoneChange { object: id, from, to: ZoneKind::Graveyard, last_known_info: Some(last_known_info) })
}

/// CR 504.1/616.1: drawing a card is itself a replaceable zone-change
/// proposal (e.g. Notion Thief-style "you draw instead" effects) - same
/// "propose, then replacement-check, then commit" split `propose_zone_move`
/// established, generalized to `to: Hand` (the other real destination a
/// zone-move proposal in this codebase starts with, alongside
/// `propose_zone_move`'s own `Graveyard`). `Engine::commit_zone_move`
/// already handles the commit side correctly with no changes needed - its
/// zone-owner derivation treats Library/Hand as per-player, same as every
/// zone besides Battlefield/Exile.
///
/// CR 704.5b: an empty library still sets `PlayerState.drew_from_empty_library`
/// here (not itself a zone-change) - the same flag the old direct-mutation
/// `Engine::draw_one_card` used to set. `Game.Ops.DrawCards(n)`'s
/// `ResolveStackTop` arm now routes through this function + `commit_zone_move`
/// (see `Engine::continue_draw_cards`). Takes `&mut GameState` rather than
/// `propose_zone_move`'s `&GameState` for exactly this one side effect -
/// nothing else about this function mutates state.
pub(crate) fn propose_draw(state: &mut GameState, player: PlayerId) -> Option<GameEventKind> {
    let card = state.zone(ZoneKind::Library, Some(player)).and_then(|library| library.contents.front().copied());
    let Some(card) = card else {
        if let Some(p) = state.player_mut(player) {
            p.drew_from_empty_library = true;
        }
        return None;
    };
    let last_known_info = state.object(card).expect("card id came from the library zone's own contents").clone();
    Some(GameEventKind::ZoneChange { object: card, from: ZoneKind::Library, to: ZoneKind::Hand, last_known_info: Some(last_known_info) })
}

/// Commits `event` (a `propose_zone_move` proposal, already
/// replacement-checked by the caller — `to` may have been rewritten by a
/// `ReplaceZoneDestination` payload) — CR 400.7 fresh `ObjectId`, same
/// convention every other zone move in this codebase uses, and purges any
/// ability/continuous-effect the old id left behind (`retire_object` - no
/// remap follows a sacrifice/discard, same as a SBA-driven death).
///
/// `from`/`to`'s owner for the `state.zone_mut` lookup is derived from the
/// zone kind, not carried separately - Battlefield/Exile are shared zones
/// (owner: `None`), everything else (Hand, Graveyard, ...) is per-player -
/// same convention `Engine::commit_scripted_zone_move`'s own Exile arm
/// already established for its destination-side lookup, mirrored here for
/// both sides since a source zone can be shared too (Battlefield, for a
/// sacrifice).
///
/// Returns the departed `Triggered`-kind abilities alongside the finished
/// `GameEvent` (CR 603.10) — for a `Sacrifice` (Battlefield→Graveyard, the
/// common case), this is what lets `Dies`/`LeavesBattlefield` fire the same
/// way they do off an SBA-driven death. For a `Discard` (Hand→Graveyard),
/// the departed abilities let `DiscardsCard`/`EachDiscardsCard`
/// (player-relative) and `Discarded` (self-referential,
/// `phase5/discarded-trigger`) fire the same way.
pub(crate) fn commit_zone_move(state: &mut GameState, event: GameEventKind) -> (GameEvent, Vec<RegisteredAbility>) {
    let GameEventKind::ZoneChange { object: old_id, from, to, last_known_info } = event else {
        unreachable!("propose_zone_move only ever proposes a ZoneChange");
    };
    let last_known_info = last_known_info.expect("propose_zone_move always sets last_known_info");
    let owner = last_known_info.owner;

    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(old_id) };
    new_object.zone = to;
    new_object.timestamp = state.allocate_sequence_no();

    let from_owner = match from {
        ZoneKind::Battlefield | ZoneKind::Exile => None,
        _ => Some(owner),
    };
    if let Some(zone) = state.zone_mut(from, from_owner) {
        zone.remove(old_id);
    }
    let departed_triggered = retire_object(state, old_id);
    let to_owner = match to {
        ZoneKind::Battlefield | ZoneKind::Exile => None,
        _ => Some(owner),
    };
    if let Some(zone) = state.zone_mut(to, to_owner) {
        zone.push_bottom(new_id);
    }
    state.objects.insert(new_id, new_object);

    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let committed = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::ZoneChange { object: new_id, from, to, last_known_info: Some(last_known_info) },
        caused_by_command: None,
        parent_event: None,
    };
    (committed, departed_triggered)
}

/// Performs the real side effect of every *immediate* `CostPayment` in an
/// already-validated `PaymentPlan` (`PaymentPlan::validate` must have
/// confirmed the whole plan first — same "trusts validate, doesn't recheck"
/// contract `cost::spend` documents for mana). `Sacrifice`/`Discard` are
/// deliberately not handled here — unlike mana/tap/life, they're zone-change
/// proposals, themselves subject to CR 601.2h/616.1 replacement checking
/// (see `propose_zone_move`'s own doc comment) — `Engine::submit_payment`
/// collects them separately and drives them through `Engine::
/// continue_cost_zone_moves` instead of this function.
///
/// Returns the `GameEventKind`s for the caller (`Engine::submit_payment`) to
/// append to the ledger — same "commands decide, engine.rs writes the
/// ledger" split every other command path in this file follows.
pub(crate) fn commit_payment(state: &mut GameState, payer: PlayerId, payments: &[CostPayment]) -> Vec<GameEventKind> {
    let mut events = Vec::new();
    for payment in payments {
        match payment {
            CostPayment::Mana { units, phyrexian_life } => {
                if let Some(player) = state.player_mut(payer) {
                    cost::spend(units, &mut player.mana_pool);
                    if *phyrexian_life > 0 {
                        player.lose_life(*phyrexian_life as i32);
                    }
                }
                if *phyrexian_life > 0 {
                    events.push(GameEventKind::LifeChanged {
                        player: payer,
                        delta: -(*phyrexian_life as i32),
                    });
                }
            }
            CostPayment::Tap(id) => {
                if let Some(obj) = state.object_mut(*id) {
                    obj.tapped = true;
                }
                events.push(GameEventKind::Tapped { object: *id });
            }
            CostPayment::Sacrifice(_) | CostPayment::Discard(_) => {
                // Handled by Engine::continue_cost_zone_moves instead - see
                // this function's own doc comment.
            }
            CostPayment::Life(amount) => {
                if let Some(player) = state.player_mut(payer) {
                    player.lose_life(*amount as i32);
                }
                events.push(GameEventKind::LifeChanged { player: payer, delta: -(*amount as i32) });
            }
            CostPayment::Other { .. } => {
                // PaymentPlan::validate never accepts a plan containing
                // this variant (see AdditionalCost::Other's doc comment) -
                // unreachable in practice, no-op rather than panicking if
                // it somehow got here anyway.
            }
        }
    }
    events
}

/// `PendingZoneMove` (`Engine::continue_cost_zone_moves`'s own queue
/// entry) - collects every `Sacrifice`/`Discard` payment in `payments` into
/// a `(id, source zone)` pair, ready for `propose_zone_move`. Kept as a
/// standalone pass (not folded into `commit_payment` above) since it must
/// run *before* any replacement-checked commit happens - `PaymentPlan::
/// validate` has already confirmed every id here really is in the zone it
/// claims to be.
pub(crate) fn collect_cost_zone_moves(payments: &[CostPayment]) -> Vec<(ObjectId, ZoneKind)> {
    payments
        .iter()
        .filter_map(|payment| match payment {
            CostPayment::Sacrifice(id) => Some((*id, ZoneKind::Battlefield)),
            CostPayment::Discard(id) => Some((*id, ZoneKind::Hand)),
            _ => None,
        })
        .collect()
}

/// Performs the real effect of one already-committed `GameEventKind::
/// DamageDealt` event (CR 510.2/119/702.15/702.80/702.90) — mark damage on
/// a creature or lose life on a player, folding in Lifelink/Wither/Infect/
/// Deathtouch off the source's effective keywords (CR 613 layer 6, so a
/// granted keyword applies here too, not just the printed `KeywordSet`).
///
/// Called from `engine.rs`'s `CommitEvents` once `event` is in its final,
/// post-replacement form — CR 614-616 says damage is itself a replaceable/
/// preventable event, so this can't run any earlier than that (`amount`
/// might have been reduced to 0, or `target` redirected, by a
/// `ReplacementEffect` in between the moment `AssignCombatDamage` proposed
/// it and the moment it's actually committed). `AssignCombatDamage` itself
/// (this file) only ever *proposes* `DamageDealt` events now — it must not
/// also mutate `damage_marked`/life directly, or a prevention/redirection
/// effect would rewrite the ledger's record of what happened without
/// stopping the mutation that already happened.
pub(crate) fn apply_damage_event(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    source: ObjectId,
    target: Target,
    amount: u32,
    is_combat_damage: bool,
) {
    // CR 702.15/702.80/702.90: looked up once, up front, so the borrow ends
    // before the target mutations below (which may need a mutable borrow of
    // the same GameState).
    let source_keywords = effective_keywords(state, script_runtime, source);
    let has_lifelink = source_keywords.contains(KeywordSet::LIFELINK);
    let replaces_with_counters = source_keywords.intersects(KeywordSet::WITHER | KeywordSet::INFECT);
    let is_infect = source_keywords.contains(KeywordSet::INFECT);
    let has_deathtouch = source_keywords.contains(KeywordSet::DEATHTOUCH);
    // CR 702.15b: lifelink's life gain goes to whoever *currently* controls
    // the damage source (layers::effective_controller), not whoever
    // controlled it when the damage was dealt if a GainControl effect has
    // since changed hands - though in practice these are the same instant.
    let source_controller = state
        .object(source)
        .map(|_| crate::layers::effective_controller(state, script_runtime, source));
    let source_is_commander = state.object(source).map(|o| o.is_commander).unwrap_or(false);

    match target {
        Target::Player(pid) => {
            // CR 903.10a: a commander's *combat* damage to a player
            // accumulates regardless of how that damage otherwise manifests
            // (life loss below, or poison via infect) - a parallel tracking
            // system, not a redirect of the life-loss/poison branching, so
            // this runs unconditionally before that branch rather than
            // inside either arm of it. TODO: keyed by this specific
            // ObjectId, which changes across a CR 903.9a return
            // (cast/die/return/recast) - a recast commander's damage count
            // resets to 0 under this key instead of accumulating against
            // "the same commander" the way CR 903.10a's own text means it.
            // Pre-existing field (item 10's slice), not changed here - see
            // docs/implementation.md's phase5/commander-damage slice notes
            // for why this was deliberately left as-is rather than widened
            // to name-keying like PlayerState::commander_casts.
            if is_combat_damage && source_is_commander && amount > 0 {
                if let Some(player) = state.player_mut(pid) {
                    match player.commander_damage_taken.iter_mut().find(|(s, _)| *s == source) {
                        Some((_, total)) => *total += amount,
                        None => player.commander_damage_taken.push((source, amount)),
                    }
                }
            }
            // CR 702.90b: Infect damage to a player becomes poison
            // counters, not life loss.
            if is_infect {
                let timestamp = state.allocate_sequence_no();
                if let Some(player) = state.player_mut(pid) {
                    player.counters.add(CounterKind::Poison, amount as i32, timestamp);
                }
            } else if let Some(player) = state.player_mut(pid) {
                player.lose_life(amount as i32);
            }
        }
        Target::Object(id) => {
            // TODO(planeswalker/battle damage, CR 120.3c/d): damage to
            // those marks loyalty/defense loss, not damage_marked -
            // GameObject has no loyalty/defense-damage path wired yet, so
            // only creature targets are functional this slice.
            let timestamp = state.allocate_sequence_no();
            if let Some(object) = state.object_mut(id) {
                if object.definition.card_types.contains(CardTypeSet::CREATURE) {
                    // CR 702.80a/702.90c: Wither/Infect damage to a
                    // creature becomes -1/-1 counters instead of marked
                    // damage.
                    if replaces_with_counters {
                        object.counters.add(CounterKind::MinusOneMinusOne, amount as i32, timestamp);
                    } else {
                        object.damage_marked += amount;
                        if has_deathtouch && amount > 0 {
                            object.deathtouched = true;
                        }
                    }
                }
            }
        }
    }

    // CR 702.15b: Lifelink's life gain happens "in addition to any other
    // results that damage causes" - applies to damage dealt to a player or
    // an object alike.
    if has_lifelink && amount > 0 {
        if let Some(controller) = source_controller {
            if let Some(player) = state.player_mut(controller) {
                player.lose_life(-(amount as i32));
            }
        }
    }
}

/// Executes one already-accepted `EngineCommand` against `state`, mutating
/// non-ledger state directly (zones, etc.) and returning a
/// `CommandOutcome`. Per the rationale in `command.rs`: this does not
/// decompose `command` into further sub-commands — the sub-steps of
/// carrying it out are events, not commands.
///
/// Does NOT append to `state.ledger` and does NOT allocate `EventId`s/
/// `SequenceNo`s — those, plus replacement/prevention checks (CR 614-616),
/// are engine.rs's job (`ReplacementCheck` -> `CommitEvents`) so there is
/// exactly one place that writes the ledger. Does NOT mutate
/// `state.consecutive_passes` or `state.priority_player` either — see
/// `CommandOutcome::broke_pass_chain`/`priority_handoff`.
pub fn execute_command(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    match command.action {
        PlayerActionKind::PassPriority => {
            // CR 117.3c: priority moves to the next player in turn_order
            // after whoever just passed - skipping anyone eliminated (CR
            // 800.4b). Read-only against turn_order - does not touch
            // state.priority_player/consecutive_passes, both of which
            // engine.rs owns (see PriorityHandoff/broke_pass_chain doc
            // comments above).
            if !state.turn_order.contains(&command.actor) {
                return Err(CommandError::IllegalAction("actor not in turn_order".to_string()));
            }
            let next_player = next_active_player(state, command.actor);
            return Ok(CommandOutcome {
                events: vec![GameEventKind::PriorityPassed { player: command.actor }],
                broke_pass_chain: false,
                priority_handoff: PriorityHandoff::To(next_player),
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::MakeChoice { .. } => {
            // Real choice application is genuinely generic - "depends
            // entirely on what asked for the choice" (see the TODO block
            // below) - no fixed event shape exists yet, so this produces
            // none. Exists as a minimal non-PassPriority stub so
            // broke_pass_chain's reset wiring (CR 117.3c) has something
            // other than PassPriority to exercise in tests; not a real
            // implementation of MakeChoice itself.
            return Ok(CommandOutcome {
                events: Vec::new(),
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::PlayLand { card } => {
            // CR 305.1: lands aren't spells, so this goes straight to the
            // battlefield - no stack object. Playing a land is still
            // restricted to sorcery speed (CR 305.1/305.3, via
            // is_sorcery_speed) and to once per turn (CR 305.2).
            if !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "a land can only be played at sorcery speed: actor's turn, a main phase, \
                     actor holding priority, and an empty stack"
                        .to_string(),
                ));
            }
            // TODO(CR 305.2): hardcoded against the literal `1` - doesn't
            // read state.continuous_effects for anything that raises a
            // player's land-plays-per-turn limit (e.g. "you may play an
            // additional land each turn"). continuous_effects has no query
            // API yet (layers.rs is still mostly todo!()) - once it does,
            // this needs "limit = 1 + sum of applicable continuous
            // effects" instead of the bare literal below.
            let already_played =
                *state.lands_played_this_turn.get(&command.actor).unwrap_or(&0);
            if already_played >= 1 {
                return Err(CommandError::IllegalAction(
                    "actor has already played their land for this turn".to_string(),
                ));
            }

            let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
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
                return Err(CommandError::IllegalAction("card is not a Land".to_string()));
            }
            if object.zone != ZoneKind::Hand || object.controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "card is not in actor's hand".to_string(),
                ));
            }
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
                .zone_mut(ZoneKind::Hand, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no hand zone".to_string()))?
                .remove(card);
            state.objects.remove(&card);
            state
                .zone_mut(ZoneKind::Battlefield, None)
                .ok_or_else(|| CommandError::IllegalAction("no battlefield zone".to_string()))?
                .push_bottom(new_id);
            state.objects.insert(new_id, new_object);
            *state.lands_played_this_turn.entry(command.actor).or_insert(0) += 1;
            remap_abilities(state, card, new_id);

            return Ok(CommandOutcome {
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
            });
        }
        PlayerActionKind::CastSpell { card, modes, targets, x_value, alternative_cost } => {
            let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
            if object.definition.card_types.contains(CardTypeSet::LAND) {
                return Err(CommandError::IllegalAction(
                    "lands aren't cast as spells - use PlayLand".to_string(),
                ));
            }
            // CR 601.2b: an announced alternative cost (flashback, etc)
            // replaces the mana cost entirely and, for the ones this slice
            // supports, is what grants permission to cast from a zone other
            // than the hand in the first place - so the zone permission and
            // the alternative-cost choice are resolved together, not as two
            // independent checks.
            let alternative: Option<&AlternativeCost> = match alternative_cost {
                Some(idx) => Some(
                    object
                        .definition
                        .alternative_costs
                        .get(idx)
                        .ok_or(CommandError::IllegalAction(
                            "no such alternative cost on this card".to_string(),
                        ))?,
                ),
                None => None,
            };
            // CR 903.8: a commander may be cast from the command zone by its
            // owner (not just controller - matching the rule text) at the
            // normal mana cost, no alternative cost announced. Checked
            // before the general zone-permission rule below rather than
            // folded into `required_zone`, since this path cares about
            // ownership, not control, and only applies with no
            // `alternative_cost` at all.
            let cast_from_command_zone = alternative.is_none()
                && object.is_commander
                && object.zone == ZoneKind::Command
                && object.owner == command.actor;
            // TODO: doesn't require casting from the graveyard specifically
            // for flashback's own zone permission to have been granted at
            // all (e.g. nothing here checks the card actually has a
            // Flashback keyword vs. an ad-hoc alternative_costs entry) -
            // simplest correct rule for now: the chosen alternative's
            // from_zone must match where the card actually is, full stop.
            // Without an alternative_cost declared, the card must be in
            // hand, same as before this slice.
            let required_zone = match alternative {
                Some(AlternativeCost::Replacement { from_zone, .. }) => *from_zone,
                None => ZoneKind::Hand,
            };
            if !cast_from_command_zone
                && (object.zone != required_zone || object.controller != command.actor)
            {
                return Err(CommandError::IllegalAction(
                    "card is not in the zone required to cast it this way".to_string(),
                ));
            }
            let from_zone = object.zone;
            // CR 117.1a/602/702.8b: instants, and anything with flash, may
            // be cast any time the actor holds priority; everything else
            // needs the full sorcery-speed window (is_sorcery_speed already
            // covers "holds priority" as one of its four conditions, so
            // this branch only adds the priority check on its own rather
            // than duplicating it).
            // TODO: effects granting flash (e.g. "you may cast this as
            // though it had flash") aren't represented on
            // continuous_effects yet - same class of gap is_sorcery_speed
            // flags for PlayLand. Only the card's own printed Flash keyword
            // is consulted here.
            let castable_at_instant_speed = object.definition.card_types.contains(CardTypeSet::INSTANT)
                || object.definition.keywords.contains(KeywordSet::FLASH);
            if castable_at_instant_speed {
                if state.priority_player != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not hold priority".to_string(),
                    ));
                }
            } else if !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "this spell can only be cast at sorcery speed: actor's turn, a main \
                     phase, actor holding priority, and an empty stack"
                        .to_string(),
                ));
            }

            // CR 700.2a/d/h: a modal card requires the caster to announce
            // their mode choice(s) here, before targets are chosen (601.2b
            // comes before 601.2c). A non-modal card (definition.modes
            // empty) is untouched: modes must be empty, and the card's own
            // singular target_requirement/effect are used exactly as before
            // this field existed. `mode_choice` names how many of `modes`
            // must be chosen (`ModeChoice::Exactly(1)` is the pre-existing
            // "Choose one —" behavior); its `*Repeatable` variants are CR
            // 700.2d's reminder-text-gated "you may choose the same mode
            // more than once" exception - a repeated index is rejected
            // unless `repeatable` is set. `mode_additional_costs` collects
            // every chosen mode's own `SpellMode::additional_cost` (CR
            // 700.2h) - appended to the spell's overall `CostSpec.additional`
            // once `cost` itself is computed below, so it's paid together
            // with the mana cost through the same already-generic
            // `PaymentPlan` machinery an activated ability's own additional
            // costs already use. A repeated mode with its own additional
            // cost pays that cost once per instance (CR 700.2d "treated as
            // if that mode appeared that many times in sequence" + 700.2h
            // "if more than one such mode is chosen, all additional costs
            // must be paid") - falls out for free below since
            // `mode_additional_costs` is collected per chosen instance, not
            // deduped by mode index.
            let (target_requirement, modal_effect, mode_additional_costs, pending_mode_choice): (
                Option<TargetSpec>,
                Option<AbilityEffect>,
                Vec<AdditionalCost>,
                Option<PendingModeChoice>,
            ) = if object.definition.modes.is_empty() {
                    if !modes.is_empty() {
                        return Err(CommandError::IllegalAction(
                            "this card isn't modal - no modes to choose".to_string(),
                        ));
                    }
                    (object.definition.target_requirement.clone(), None, Vec::new(), None)
                } else if let ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) =
                    object.definition.mode_choice
                {
                    // CR 700.2e: the other player chooses, not the caster -
                    // see `ModeChoice`'s own doc comment for the full
                    // narrowing this variant carries. The choice itself is
                    // deferred to a resolution-time pause rather than
                    // supplied here.
                    if !modes.is_empty() {
                        return Err(CommandError::IllegalAction(
                            "this card's mode is chosen by another player, not the caster - submit \
                             no modes when casting it"
                                .to_string(),
                        ));
                    }
                    if !targets.is_empty() {
                        return Err(CommandError::IllegalAction(
                            "this card's mode (and therefore its targets) isn't chosen yet"
                                .to_string(),
                        ));
                    }
                    if object
                        .definition
                        .modes
                        .iter()
                        .any(|m| m.target_requirement.is_some() || m.additional_cost.is_some())
                    {
                        return Err(CommandError::IllegalAction(
                            "a chosen-by-opponent modal card's modes must have no target \
                             requirement and no additional cost"
                                .to_string(),
                        ));
                    }
                    let others: Vec<PlayerId> =
                        non_eliminated_players(state).into_iter().filter(|&p| p != command.actor).collect();
                    let [chooser] = others.as_slice() else {
                        return Err(CommandError::IllegalAction(
                            "a chosen-by-opponent modal card needs exactly one other player in the \
                             game - CR 700.2e's 3+ player 'controller decides who chooses' \
                             sub-clause isn't supported yet"
                                .to_string(),
                        ));
                    };
                    let (min, max) = match object.definition.mode_choice {
                        ModeChoice::ExactlyChosenByOpponent(n) => (n, n),
                        ModeChoice::UpToChosenByOpponent(n) => (1, n),
                        _ => unreachable!("matched above"),
                    };
                    (
                        None,
                        None,
                        Vec::new(),
                        Some(PendingModeChoice { chooser: *chooser, min, max }),
                    )
                } else {
                    let (min, max, repeatable) = match object.definition.mode_choice {
                        ModeChoice::Exactly(n) => (n, n, false),
                        // CR 700.2a: "choose a number of those options" -
                        // never zero, even for an UpTo spell.
                        ModeChoice::UpTo(n) => (1, n, false),
                        ModeChoice::ExactlyRepeatable(n) => (n, n, true),
                        ModeChoice::UpToRepeatable(n) => (1, n, true),
                        ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) => {
                            unreachable!("handled above")
                        }
                    };
                    let chosen_count = modes.len() as u32;
                    if chosen_count < min || chosen_count > max {
                        return Err(CommandError::IllegalAction(format!(
                            "this modal spell requires choosing between {min} and {max} mode(s), got {chosen_count}"
                        )));
                    }
                    if !repeatable {
                        let mut seen_modes = HashSet::new();
                        for &m in &modes {
                            if !seen_modes.insert(m) {
                                return Err(CommandError::IllegalAction(
                                    "the same mode can't be chosen more than once".to_string(),
                                ));
                            }
                        }
                    }
                    // CR 700.2's own convention (per Wizards' rulings on
                    // e.g. Cryptic Command): chosen modes' effects happen in
                    // the order printed on the card, not the order the
                    // player listed them in `modes`.
                    let mut chosen_indices = modes.clone();
                    chosen_indices.sort_unstable();
                    let mut chosen_modes = Vec::with_capacity(chosen_indices.len());
                    for &idx in &chosen_indices {
                        let mode = object.definition.modes.get(idx as usize).ok_or_else(|| {
                            CommandError::IllegalAction("no such mode on this card".to_string())
                        })?;
                        chosen_modes.push(mode);
                    }
                    // CR 700.2h: every chosen mode's own additional cost (if
                    // any), collected regardless of whether one or several
                    // modes were chosen - "if more than one such mode is
                    // chosen, all additional costs must be paid."
                    let mode_additional_costs: Vec<AdditionalCost> =
                        chosen_modes.iter().filter_map(|m| m.additional_cost.clone()).collect();
                    if let [only] = chosen_modes.as_slice() {
                        // Single mode chosen - unchanged pre-multi-mode
                        // shape, no TargetSpec::Multiple/AbilityEffect::Modal
                        // wrapping.
                        (only.target_requirement.clone(), only.effect.clone(), mode_additional_costs, None)
                    } else {
                        // 2+ modes chosen: combine into one TargetSpec::
                        // Multiple (one TargetRequirement per chosen mode, a
                        // synthetic zero-count placeholder for a mode with
                        // no target_requirement of its own, CR 700.2c) and
                        // one AbilityEffect::Modal (one slot per chosen
                        // mode, same order) - ResolveStackTop zips the two
                        // back together at resolution time via
                        // split_targets_by_requirement, the same helper
                        // multi-target-word spells already use. A mode
                        // whose own target_requirement is itself
                        // TargetSpec::Multiple (a mode that independently
                        // needs several differently-filtered targets) can't
                        // be combined this way - rejected outright rather
                        // than silently flattened, a named narrow gap.
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
                                    return Err(CommandError::IllegalAction(
                                        "a mode with its own multi-target requirement can't be \
                                         combined with other chosen modes"
                                            .to_string(),
                                    ));
                                }
                            }
                        }
                        // Always TargetSpec::Multiple, even if every chosen
                        // mode's requirement is the synthetic zero-count
                        // placeholder - ResolveStackTop's Modal handling
                        // always re-derives per-mode target chunks from
                        // this shape, so it must never collapse to `None`
                        // the way a single chosen mode's bare `None` can.
                        for mode in &chosen_modes {
                            if let Some(effect) = &mode.effect {
                                if !is_modal_resolvable(effect) {
                                    return Err(CommandError::IllegalAction(
                                        "this mode's effect can't be combined with other chosen \
                                         modes yet - only GainLife/Damage/Counters/Pump/\
                                         GainControl/Shuffle are supported inside a multi-mode cast"
                                            .to_string(),
                                    ));
                                }
                            }
                        }
                        let target_requirement = Some(TargetSpec::Multiple(reqs));
                        let effects: Vec<Option<AbilityEffect>> =
                            chosen_modes.iter().map(|m| m.effect.clone()).collect();
                        (target_requirement, Some(AbilityEffect::Modal(effects)), mode_additional_costs, None)
                    }
                };

            // CR 601.2c/601.2e: target count/filter/dedup validated below via
            // validate_targets, rejecting the whole cast before anything is
            // mutated if illegal. TODO(CR 601.2d): no division/distribution
            // step for effects that split among targets. TODO: no splice
            // (CR 601.2b/702.47) - a player revealing other cards in hand to
            // splice their text onto this spell isn't modeled at all;
            // PlayerAction has nowhere to carry "which cards I'm splicing,"
            // and StackObject has nowhere to record it either.
            validate_targets(
                state,
                script_runtime,
                target_requirement.as_ref(),
                &targets,
                command.actor,
                card,
            )?;

            // CR 601.2f: total cost = the card's printed mana_cost, unless
            // an alternative cost was announced (CR 601.2b), in which case
            // it replaces the mana cost entirely - never mixed with it.
            // `cost.additional` is populated below only from chosen modes'
            // own CR 700.2h costs (`mode_additional_costs`, computed above)
            // - a non-modal card's own top-level additional cost has no
            // representation on `CardDefinition` at all yet (only
            // `alternative_costs` exists), a separate, still-open gap.
            //
            // CR 107.3a/601.2b: an {X} cost and an announced x_value must
            // agree - both present or both absent - and once agreed, X is
            // substituted for a generic amount before PaymentPlan::validate
            // ever sees the cost (see ManaCost::resolve_x's own doc comment
            // for why that's enough on its own, no changes needed in
            // cost.rs). TODO: X on an activated ability's own cost isn't
            // supported at all yet - ActivateAbility has no x_value field
            // to announce one with, a real struct gap, not just this check
            // missing. TODO: X combined with an alternative cost (e.g.
            // flashback on an X spell) isn't handled - x_value is simply
            // unchecked in that branch below.
            let (mut cost, exile_on_leaving_stack) = match alternative {
                Some(AlternativeCost::Replacement { cost, exile_on_leaving_stack, .. }) => {
                    (cost.clone(), *exile_on_leaving_stack)
                }
                None => {
                    let has_x = object.definition.mana_cost.has_x();
                    let resolved_mana_cost = match (has_x, x_value) {
                        (true, Some(x)) => object.definition.mana_cost.resolve_x(x),
                        (false, None) => object.definition.mana_cost.clone(),
                        (true, None) => {
                            return Err(CommandError::IllegalAction(
                                "this spell's cost includes {X} - a value must be announced"
                                    .to_string(),
                            ));
                        }
                        (false, Some(_)) => {
                            return Err(CommandError::IllegalAction(
                                "an X value was announced but this spell's cost has no {X}"
                                    .to_string(),
                            ));
                        }
                    };
                    // CR 601.2f/118.9: static cost-modification effects
                    // (Goblin Warchief, Thalia, ...) apply after X is
                    // resolved, before the cost is locked in for payment -
                    // see CostModifier's own doc comment (cost.rs) for why
                    // this only touches the plain mana-cost path (never an
                    // alternative cost, this slice).
                    let resolved_mana_cost =
                        cost::apply_cost_modifiers(&resolved_mana_cost, &state.cost_modifiers, object);
                    // CR 903.8/702.124d: commander tax - {2} generic per
                    // prior time this player has cast *this* commander (by
                    // name, not ObjectId - see PlayerState::commander_casts'
                    // own doc comment for why) from the command zone this
                    // game. Only the 903.8 path taxes; a normal hand cast or
                    // an announced alternative cost never does (`alternative`
                    // being `None` is this whole match arm's own condition,
                    // and `cast_from_command_zone` narrows it further).
                    let resolved_mana_cost = if cast_from_command_zone {
                        let prior_casts = state
                            .player(command.actor)
                            .and_then(|p| p.commander_casts.get(&object.definition.name))
                            .copied()
                            .unwrap_or(0);
                        // A first cast (prior_casts == 0, delta == 0) must
                        // be a true no-op - apply_cost_delta(0) isn't one,
                        // it always drops an explicit Generic(0) symbol
                        // (see cost::apply_cost_modifiers's own doc comment
                        // for the same bug shape, caught by phase5/cost-
                        // modifier-order). An {X} commander with X == 0,
                        // cast from the command zone for the first time,
                        // would otherwise lose that explicit zero marker.
                        if prior_casts == 0 {
                            resolved_mana_cost
                        } else {
                            resolved_mana_cost.apply_cost_delta(2 * prior_casts as i32)
                        }
                    } else {
                        resolved_mana_cost
                    };
                    (CostSpec::mana_only(resolved_mana_cost), false)
                }
            };
            // CR 700.2h: every chosen mode's own additional cost (collected
            // above, regardless of whether an alternative or normal mana
            // cost is being paid - CR 700.2h doesn't exclude either) is
            // paid together with the rest of this cost, through the same
            // already-generic PaymentPlan machinery.
            cost.additional.extend(mode_additional_costs);

            // Snapshot before the move - CR 603.10's last_known_info (see
            // GameEventKind::ZoneChange's doc comment).
            let last_known_info = object.clone();

            // CR 400.7/601.2a: casting moves the card to the stack, which is
            // a zone change that mints a new ObjectId (same convention
            // PlayLand uses for hand->battlefield) - see ObjectLineage.
            let new_id = state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(card) };
            new_object.zone = ZoneKind::Stack;
            new_object.timestamp = state.allocate_sequence_no();

            state
                .zone_mut(from_zone, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no such zone".to_string()))?
                .remove(card);
            state.objects.remove(&card);
            state.objects.insert(new_id, new_object);
            remap_abilities(state, card, new_id);

            // CR 903.8: this cast counts toward the *next* commander tax
            // once it actually commits (the zone move above, not merely
            // attempted) - matches where the tax was computed above (this
            // player's own count, at the point of casting). TODO: if this
            // cast is later abandoned via `Engine::unwind_payment` (CR
            // 601.6), this increment isn't rolled back - a real gap, same
            // family as `unwind_payment`'s own pre-existing bug of returning
            // any unwound spell to hand regardless of its real origin zone
            // (so an unwound commander cast already loses its command-zone
            // recastability independent of this). Flagged, not fixed here.
            if cast_from_command_zone {
                if let Some(p) = state.player_mut(command.actor) {
                    *p.commander_casts.entry(last_known_info.definition.name.clone()).or_insert(0) += 1;
                }
            }

            // CR 608.2m: a real, Rust-native fast-path effect (e.g. "Shock
            // deals 2 damage to any target") if the card's own
            // CardDefinition carries one - see CardDefinition::effect's own
            // doc comment for why a permanent spell never has one. For a
            // modal card, the chosen mode's own effect (resolved above,
            // before targets) is used instead of the card's singular field.
            let effect = if last_known_info.definition.modes.is_empty() {
                last_known_info.definition.effect.clone()
            } else {
                modal_effect
            };

            // A script-declared custom-function operation (`s.operation()`
            // returning a Lua function rather than a `Game.Ops.*` tag) is
            // packed onto `CardDefinition.effect` as `AbilityEffect::
            // Script(handle)` - see that variant's own doc comment for why
            // `CardDefinition` has no separate `operation: ScriptHandle`
            // field to carry it natively. Unpacked here into StackObject's
            // real two-field shape (`operation`/`effect`) so
            // `ResolveStackTop` never has to know the difference from a
            // triggered/activated ability's own script fallback - `None` on
            // `effect` is what makes it invoke `operation` for real. Every
            // other shape (a `Game.Ops.*` primitive, or no script-declared
            // operation at all) keeps the pre-existing `ScriptHandle(0)`
            // placeholder - "nothing real bound to it," only `effect`'s
            // Rust-native fast path can resolve those.
            let (operation, effect) = match effect {
                Some(AbilityEffect::Script(handle)) => (handle, None),
                other => (ScriptHandle(0), other),
            };

            state.stack.push(StackObject::new(
                new_id,
                StackEntryKind::Spell,
                new_id,
                // CR 800.4a: a spell's owner is its card's own owner, not
                // necessarily whoever is casting it.
                last_known_info.owner,
                command.actor,
                modes,
                targets,
                target_requirement,
                x_value,
                exile_on_leaving_stack,
                operation,
                effect,
                // CR 603.4's "if" clause is triggered-ability-only text - a
                // spell never carries one.
                None,
                pending_mode_choice,
            ));

            return Ok(CommandOutcome {
                events: vec![
                    GameEventKind::ZoneChange {
                        object: new_id,
                        from: from_zone,
                        to: ZoneKind::Stack,
                        last_known_info: Some(last_known_info),
                    },
                    GameEventKind::StackObjectPushed { stack_id: new_id },
                ],
                // CR 601.2i: the controller already held priority before
                // casting (checked above), so they keep it - it doesn't move.
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                // CR 601.2f-h: cost isn't paid yet just by being on the
                // stack - engine.rs pauses on AwaitingPayment until
                // Engine::submit_payment answers, even for a {0} cost (CR
                // 118.5a still requires an explicit empty acknowledgment).
                requires_payment: Some(PendingPaymentRequest {
                    stack_id: new_id,
                    payer: command.actor,
                    cost,
                    // Placeholder - a spell's CostSpec never contains
                    // AdditionalCost::TapSelf, so this is never consulted.
                    source: new_id,
                    on_paid: PaymentResolution::StackObject,
                }),
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::ActivateAbility { source, ability_index, targets, chosen_colors } => {
            // CR 602.2a/117.3c: activating an ability follows the same
            // process as casting a spell - the activator must hold priority
            // to begin with.
            if state.priority_player != command.actor {
                return Err(CommandError::IllegalAction(
                    "actor does not hold priority".to_string(),
                ));
            }

            let source_object = state.object(source).ok_or(CommandError::InvalidTarget)?;
            // CR 800.4a: an ability's owner is its source permanent's owner,
            // not its (possibly `GainControl`'d) controller - captured now,
            // this borrow doesn't survive past this statement.
            let source_owner = source_object.owner;
            // CR 602.2a: only the source's *current* controller may activate
            // it - layers::effective_controller, not the raw field, so a
            // GainControl'd permanent's abilities follow its new controller.
            let controller = crate::layers::effective_controller(state, script_runtime, source);
            if controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "actor does not control the ability's source".to_string(),
                ));
            }
            // CR 602.1/112.6: activated abilities on permanents are only
            // usable while the permanent is on the battlefield (unless the
            // ability itself says otherwise — no such override exists yet).
            if source_object.zone != ZoneKind::Battlefield {
                return Err(CommandError::IllegalAction(
                    "source is not on the battlefield".to_string(),
                ));
            }

            let ability = find_ability(state, source, ability_index).ok_or_else(|| {
                CommandError::IllegalAction("no such ability on source".to_string())
            })?;
            if ability.kind != AbilityKind::Activated {
                return Err(CommandError::IllegalAction(
                    "ability is not an activated ability".to_string(),
                ));
            }
            let effect = ability.effect.clone();
            let cost = ability.cost.clone();
            let operation = ability.operation;
            let target_requirement = ability.target.clone();

            // CR 702.6a: "Activate only as a sorcery" - Equip's own timing
            // restriction, enforced here by matching on the effect shape
            // rather than a new RegisteredAbility field (see AbilityEffect::
            // Attach's own doc comment for why - a generic per-ability
            // timing-restriction field isn't worth the ~94-call-site
            // blast radius for a restriction only this one effect needs
            // today). `is_sorcery_speed` was written anticipating exactly
            // this call site (see its own doc comment) but had no real
            // caller until now.
            if matches!(effect, Some(AbilityEffect::Attach)) && !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "this ability can only be activated as a sorcery".to_string(),
                ));
            }

            // CR 602.2b: `chosen_colors` is only meaningful alongside
            // `AddManaChoice` - reject rather than silently ignore it
            // against any other ability (a plain `AddMana`'s units are
            // fixed, and no non-mana ability has a color choice at all), so
            // a caller passing it by mistake gets a loud error instead of a
            // quietly-dropped value.
            if !chosen_colors.is_empty() && !matches!(effect, Some(AbilityEffect::AddManaChoice { .. })) {
                return Err(CommandError::IllegalAction(
                    "chosen_colors given but this ability has no mana-color choice to make".to_string(),
                ));
            }

            match effect {
                Some(effect @ (AbilityEffect::AddMana(_) | AbilityEffect::AddManaChoice { .. })) => {
                    // CR 605.1a: a mana ability never has a target.
                    if !targets.is_empty() {
                        return Err(CommandError::IllegalAction(
                            "a mana ability cannot have targets".to_string(),
                        ));
                    }

                    let units: Vec<ManaUnit> = match effect {
                        AbilityEffect::AddMana(units) => units,
                        AbilityEffect::AddManaChoice { count, choices } => {
                            if chosen_colors.len() != count as usize {
                                return Err(CommandError::IllegalAction(format!(
                                    "expected {count} chosen color(s), got {}",
                                    chosen_colors.len()
                                )));
                            }
                            let mut units = Vec::with_capacity(chosen_colors.len());
                            for color in &chosen_colors {
                                if !choices.contains_color(*color) {
                                    return Err(CommandError::IllegalAction(format!(
                                        "chosen color {color:?} is not among this ability's allowed colors"
                                    )));
                                }
                                units.push(ManaUnit::Colored(*color));
                            }
                            units
                        }
                        _ => unreachable!("matched only AddMana/AddManaChoice above"),
                    };

                    // A mana ability built with no `Game.Costs.*` component at
                    // all (`cost: None` - the only shape `mana_ability` TOML
                    // can ever produce) keeps its original hardcoded-tap,
                    // no-payment-pause behavior unchanged below. A mana
                    // ability with a *real* cost (`Game.CreateManaAbility`'s
                    // own `:AddCost`, e.g. a Treasure/Lotus-Petal-style
                    // sacrifice, or an explicit `Game.Costs.TapSelf`) still
                    // never reaches the stack (CR 605.3b), but does need a
                    // real payment step first, same as any other ability (CR
                    // 602.2f-h/118.5a) - see `PaymentResolution::ManaAbility`.
                    let has_real_cost = cost
                        .as_ref()
                        .is_some_and(|c| c.mana.is_some() || !c.additional.is_empty());

                    if !has_real_cost {
                        let source_object = state.object(source).ok_or(CommandError::InvalidTarget)?;
                        if source_object.tapped {
                            return Err(CommandError::IllegalAction(
                                "source is already tapped".to_string(),
                            ));
                        }
                        // CR 302.6: summoning sickness only restricts a
                        // creature's {T}/{Q} abilities - non-creature
                        // permanents (lands, artifacts, ...) ignore it
                        // entirely.
                        if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                            && has_summoning_sickness(state, script_runtime, source_object)
                        {
                            return Err(CommandError::IllegalAction(
                                "source has summoning sickness".to_string(),
                            ));
                        }

                        state.object_mut(source).ok_or(CommandError::InvalidTarget)?.tapped = true;
                        let pool = &mut state
                            .player_mut(command.actor)
                            .ok_or_else(|| {
                                CommandError::IllegalAction("actor has no player state".to_string())
                            })?
                            .mana_pool;
                        for unit in &units {
                            pool.add_unit(*unit);
                        }

                        // CR 605.3b: a mana ability resolves immediately as
                        // part of activation - it never uses the stack.
                        return Ok(CommandOutcome {
                            events: vec![
                                GameEventKind::Tapped { object: source },
                                GameEventKind::ManaAdded { player: command.actor, units },
                            ],
                            broke_pass_chain: true,
                            priority_handoff: PriorityHandoff::Unchanged,
                            requires_payment: None,
                            battlefield_entries: Vec::new(),
                        });
                    }

                    let cost = cost.expect("has_real_cost already confirmed Some");

                    // CR 602.5a/302.6: same TapSelf pre-check the non-mana
                    // branch below takes, when this mana ability's own cost
                    // happens to include one - the actual tap still only
                    // commits once Engine::submit_payment does, same as any
                    // other AdditionalCost::TapSelf.
                    if cost.additional.iter().any(|c| matches!(c, AdditionalCost::TapSelf)) {
                        let source_object =
                            state.object(source).ok_or(CommandError::InvalidTarget)?;
                        if source_object.tapped {
                            return Err(CommandError::IllegalAction(
                                "source is already tapped".to_string(),
                            ));
                        }
                        if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                            && has_summoning_sickness(state, script_runtime, source_object)
                        {
                            return Err(CommandError::IllegalAction(
                                "source has summoning sickness".to_string(),
                            ));
                        }
                    }

                    // No StackObject is ever pushed for a mana ability (CR
                    // 605.1a/605.3b) - `stack_id` here is only an opaque
                    // handle for this payment pause to be identified by
                    // (`Engine::unwind_payment` already tolerates a `stack_id`
                    // matching nothing on `state.stack`), never a real
                    // resolvable stack entry.
                    let stack_id = state.allocate_object_id();

                    return Ok(CommandOutcome {
                        events: Vec::new(),
                        broke_pass_chain: true,
                        priority_handoff: PriorityHandoff::Unchanged,
                        requires_payment: Some(PendingPaymentRequest {
                            stack_id,
                            payer: command.actor,
                            cost,
                            source,
                            on_paid: PaymentResolution::ManaAbility { controller: command.actor, units },
                        }),
                        battlefield_entries: Vec::new(),
                    });
                }
                // None (script-bridge-only effect) and Some(GainLife)/
                // Some(Damage)/Some(Destroy)/Some(Counters)/Some(Pump)/
                // Some(GainControl)/Some(Regenerate)/Some(DrawCards)/
                // Some(Shuffle)/Some(Scry)/Some(Surveil)/Some(Search)/
                // Some(BecomeCopy)/Some(Attach) (a non-mana ActivateAbility
                // whose effect happens to be a recognized primitive) all
                // take the same path: unlike CR 605's mana abilities, a
                // non-mana activated ability uses the stack (CR 602.2)
                // rather than resolving immediately. Targets are validated
                // below (CR 602.2b/601.2c-e) via the same validate_targets
                // CastSpell uses, before any state mutation (stack push)
                // happens - for Attach (Equip), this is where CR 702.6a's
                // "target creature you control" is enforced, same as any
                // other targeted ability.
                non_mana_effect @ (None
                | Some(AbilityEffect::GainLife(_))
                | Some(AbilityEffect::Damage(_))
                | Some(AbilityEffect::Destroy)
                // CR 701.8/601.2c: no different from any other untargeted
                // effect shape at this level - "destroy all X" needs no
                // targets validated (target_requirement is None for it),
                // same as Shuffle just below.
                | Some(AbilityEffect::DestroyAll(_))
                | Some(AbilityEffect::Sacrifice)
                | Some(AbilityEffect::Exile)
                | Some(AbilityEffect::Counters { .. })
                | Some(AbilityEffect::Pump { .. })
                | Some(AbilityEffect::GainControl(_))
                | Some(AbilityEffect::Regenerate)
                | Some(AbilityEffect::DrawCards(_))
                | Some(AbilityEffect::CreateToken { .. })
                | Some(AbilityEffect::Shuffle)
                | Some(AbilityEffect::Scry(_))
                | Some(AbilityEffect::Surveil(_))
                | Some(AbilityEffect::Search { .. })
                | Some(AbilityEffect::BecomeCopy(_))
                | Some(AbilityEffect::Attach)
                | Some(AbilityEffect::ExchangeTextBoxes)
                // CR 700.2b (a modal activated/triggered ability) is still
                // unmodeled - RegisteredAbility has no modes field, so this
                // variant can't actually originate from an ability today.
                // Listed for exhaustiveness only, same "every variant named,
                // no wildcard" convention this disjunction already follows.
                | Some(AbilityEffect::Modal(_))
                // AbilityEffect::Script only ever comes from a plain spell's
                // CardDefinition.effect (CastSpell unpacks it before push -
                // see that variant's own doc comment) - an ability's effect
                // can't be this shape. Listed for exhaustiveness only, same
                // reason as Modal just above.
                | Some(AbilityEffect::Script(_))) => {
                    validate_targets(
                        state,
                        script_runtime,
                        target_requirement.as_ref(),
                        &targets,
                        command.actor,
                        source,
                    )?;

                    // `None` means "no cost at all" - same empty-CostSpec
                    // treatment RegisteredAbility::cost's own doc comment
                    // describes.
                    let cost = cost.unwrap_or(CostSpec {
                        mana: None,
                        additional: Vec::new(),
                        alternative: Vec::new(),
                    });

                    // CR 602.5a/302.6: a TapSelf cost can't be paid by an
                    // already-tapped source, and a creature's own {T}
                    // ability is restricted by summoning sickness - both
                    // checked against the declared cost up front, before
                    // anything is pushed (an illegal activation never
                    // happens, same stance validate_targets already takes).
                    // The actual tap (like every other additional-cost
                    // mutation) happens once Engine::submit_payment commits
                    // a real PaymentPlan - CR 602.2f-h treats paying the
                    // cost as its own step, separate from announcing the
                    // activation.
                    if cost.additional.iter().any(|c| matches!(c, AdditionalCost::TapSelf)) {
                        let source_object =
                            state.object(source).ok_or(CommandError::InvalidTarget)?;
                        if source_object.tapped {
                            return Err(CommandError::IllegalAction(
                                "source is already tapped".to_string(),
                            ));
                        }
                        if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                            && has_summoning_sickness(state, script_runtime, source_object)
                        {
                            return Err(CommandError::IllegalAction(
                                "source has summoning sickness".to_string(),
                            ));
                        }
                    }

                    // CR 400.7: the stack object is a new id, distinct
                    // from `source` (which stays on the battlefield) -
                    // same convention PutTriggersApnap uses for
                    // triggered abilities.
                    let stack_id = state.allocate_object_id();
                    state.stack.push(StackObject::new(
                        stack_id,
                        StackEntryKind::ActivatedAbility,
                        source,
                        source_owner,
                        command.actor,
                        // TODO: no modes/X value on activated abilities yet.
                        Vec::new(),
                        targets,
                        target_requirement,
                        None,
                        false,
                        operation,
                        non_mana_effect,
                        // CR 603.4's "if" clause is triggered-ability-only
                        // text - an activated ability never carries one.
                        None,
                        // CR 700.2e: only a modal spell can defer its mode
                        // choice this way - an activated ability never does.
                        None,
                    ));

                    return Ok(CommandOutcome {
                        events: vec![GameEventKind::StackObjectPushed { stack_id }],
                        broke_pass_chain: true,
                        priority_handoff: PriorityHandoff::Unchanged,
                        // CR 602.2f-h: same AwaitingPayment pause CastSpell
                        // uses, even for a bare {T} cost (CR 118.5a's
                        // "explicit empty acknowledgment" stance applies
                        // here too via the empty-CostSpec default above).
                        requires_payment: Some(PendingPaymentRequest {
                            stack_id,
                            payer: command.actor,
                            cost,
                            source,
                            on_paid: PaymentResolution::StackObject,
                        }),
                        battlefield_entries: Vec::new(),
                    });
                }
            }
        }
        PlayerActionKind::DeclareAttackers { attackers } => {
            // CR 508.1: a turn-based action of the active player, only
            // during the declare attackers step, and only once per combat.
            if command.actor != state.active_player {
                return Err(CommandError::IllegalAction(
                    "only the active player declares attackers".to_string(),
                ));
            }
            if state.phase != Phase::Combat || state.step != Step::DeclareAttackers {
                return Err(CommandError::IllegalAction(
                    "attackers can only be declared during the declare attackers step"
                        .to_string(),
                ));
            }
            if state.combat.is_some() {
                return Err(CommandError::IllegalAction(
                    "attackers have already been declared this combat".to_string(),
                ));
            }

            // CR 508.1: "if at any point ... unable to comply ... the
            // declaration is illegal" - validate every attacker before
            // mutating anything, so an illegal batch taps nobody.
            for (attacker, target) in &attackers {
                let object = state.object(*attacker).ok_or(CommandError::InvalidTarget)?;
                // CR 508.1a: attacking requires *current* control, per
                // layers::effective_controller.
                if crate::layers::effective_controller(state, script_runtime, *attacker) != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not control a chosen attacker".to_string(),
                    ));
                }
                if !object.definition.card_types.contains(CardTypeSet::CREATURE)
                    || object.definition.card_types.contains(CardTypeSet::BATTLE)
                {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker is not a creature, or is also a battle".to_string(),
                    ));
                }
                if object.tapped {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker is already tapped".to_string(),
                    ));
                }
                // CR 508.1a/302.6/702.10b: has_summoning_sickness itself
                // grants Haste's bypass.
                if has_summoning_sickness(state, script_runtime, object) {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker has summoning sickness".to_string(),
                    ));
                }
                // CR 302.6b/702.3b: a creature with defender can't attack -
                // effective_keywords so a granted/removed Defender (e.g.
                // Assault Formation's RemoveKeyword(Defender)) is respected,
                // not just the printed keyword.
                if effective_keywords(state, script_runtime, object.object_id).contains(KeywordSet::DEFENDER) {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker has defender and can't attack".to_string(),
                    ));
                }
                match target {
                    Target::Player(pid) => {
                        if *pid == command.actor || !state.turn_order.contains(pid) {
                            return Err(CommandError::IllegalAction(
                                "an attacker's target must be an opponent".to_string(),
                            ));
                        }
                    }
                    Target::Object(target_id) => {
                        let target_object =
                            state.object(*target_id).ok_or(CommandError::InvalidTarget)?;
                        // CR 508.1b: the target permanent's *current*
                        // controller must be an opponent.
                        if crate::layers::effective_controller(state, script_runtime, *target_id)
                            == command.actor
                        {
                            return Err(CommandError::IllegalAction(
                                "an attacker's target permanent must be controlled by an \
                                 opponent"
                                    .to_string(),
                            ));
                        }
                        if !target_object
                            .definition
                            .card_types
                            .intersects(CardTypeSet::PLANESWALKER | CardTypeSet::BATTLE)
                        {
                            return Err(CommandError::IllegalAction(
                                "an attacker's target permanent must be a planeswalker or \
                                 battle"
                                    .to_string(),
                            ));
                        }
                    }
                }
            }

            // CR 508.1f: tapping an attacker isn't a cost - attacking
            // simply causes it. CR 702.20a: Vigilance means attacking
            // doesn't tap it.
            let mut events = Vec::with_capacity(attackers.len() * 2);
            for (attacker, target) in &attackers {
                // effective_keywords first (immutable borrow) so a granted
                // Vigilance is respected too, not just the printed keyword -
                // computed before the mutable borrow below, since the two
                // can't overlap.
                let has_vigilance = effective_keywords(state, script_runtime, *attacker).contains(KeywordSet::VIGILANCE);
                let object = state.object_mut(*attacker).expect("validated above");
                if !has_vigilance {
                    object.tapped = true;
                    events.push(GameEventKind::Tapped { object: *attacker });
                }
                events
                    .push(GameEventKind::AttackerDeclared { attacker: *attacker, target: *target });
            }
            state.combat = Some(CombatState {
                attackers,
                blocks: Vec::new(),
                declared_defenders: Vec::new(),
                bands: Vec::new(),
            });

            return Ok(CommandOutcome {
                events,
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::DeclareBands { bands } => {
            // CR 508.1e/702.22c: same window as DeclareAttackers itself -
            // active player, only during the declare attackers step, only
            // once attackers actually exist to group.
            if command.actor != state.active_player {
                return Err(CommandError::IllegalAction(
                    "only the active player declares bands".to_string(),
                ));
            }
            if state.phase != Phase::Combat || state.step != Step::DeclareAttackers {
                return Err(CommandError::IllegalAction(
                    "bands can only be declared during the declare attackers step".to_string(),
                ));
            }
            let combat = state.combat.as_ref().ok_or_else(|| {
                CommandError::IllegalAction("no attackers have been declared this combat".to_string())
            })?;

            let mut seen: Vec<ObjectId> = Vec::new();
            for band in &bands {
                let mut target: Option<Target> = None;
                let mut banding_members = 0u32;
                let mut non_banding_members = 0u32;
                // CR 702.22c's second band-formation path: one or more
                // members with "bands with other [quality]" for some
                // quality Q, plus any number of other members whose own
                // subtypes include Q. `member_subtypes`/`bands_with_other`
                // collected per member below, checked once the whole band
                // is known (a member's own quality can qualify a bandmate
                // that comes later in the list).
                let mut member_subtypes: Vec<Vec<crate::Subtype>> = Vec::new();
                let mut member_bands_with_other: Vec<Vec<crate::Subtype>> = Vec::new();
                for member in band {
                    let (_, member_target) = combat.attackers.iter().find(|(id, _)| id == member).ok_or_else(|| {
                        CommandError::IllegalAction(
                            "a band member isn't actually attacking this combat".to_string(),
                        )
                    })?;
                    if seen.contains(member) {
                        return Err(CommandError::IllegalAction(
                            "a creature can be in at most one band".to_string(),
                        ));
                    }
                    seen.push(*member);

                    // CR 702.22d/802.3b: every member of a band must attack
                    // the same player, planeswalker, or battle.
                    match target {
                        None => target = Some(*member_target),
                        Some(t) if t == *member_target => {}
                        Some(_) => {
                            return Err(CommandError::IllegalAction(
                                "every creature in a band must attack the same player, \
                                 planeswalker, or battle"
                                    .to_string(),
                            ));
                        }
                    }

                    if effective_keywords(state, script_runtime, *member).contains(KeywordSet::BANDING) {
                        banding_members += 1;
                    } else {
                        non_banding_members += 1;
                    }

                    let characteristics =
                        crate::layers::effective_characteristics(state, script_runtime, *member);
                    member_subtypes.push(characteristics.subtypes);
                    member_bands_with_other.push(characteristics.bands_with_other_subtypes);
                }

                // CR 702.22c, first path: one or more attacking creatures
                // with banding, plus up to one attacking creature without
                // banding.
                let plain_banding_shape = banding_members >= 1 && non_banding_members <= 1;
                // CR 702.22c, second path ("bands with other [quality]"): at
                // least one member has bands-with-other for some quality Q,
                // and every *other* member's own subtypes include Q. Tries
                // every quality named by any member in turn - the band is
                // legal if any one of them makes every other member match.
                let quality_shape = member_bands_with_other.iter().enumerate().any(|(i, qualities)| {
                    qualities.iter().any(|quality| {
                        member_subtypes
                            .iter()
                            .enumerate()
                            .all(|(j, subtypes)| j == i || subtypes.contains(quality))
                    })
                });
                if !plain_banding_shape && !quality_shape {
                    return Err(CommandError::IllegalAction(
                        "a band needs either at least one attacking creature with banding (and \
                         at most one without), or a creature with \"bands with other [quality]\" \
                         whose bandmates all share that quality"
                            .to_string(),
                    ));
                }
            }

            let combat = state.combat.as_mut().expect("checked Some above");
            combat.bands = bands;

            return Ok(CommandOutcome {
                events: Vec::new(),
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::DeclareBlockers { mut blocks } => {
            if state.phase != Phase::Combat || state.step != Step::DeclareBlockers {
                return Err(CommandError::IllegalAction(
                    "blockers can only be declared during the declare blockers step".to_string(),
                ));
            }
            let combat = state.combat.as_ref().ok_or_else(|| {
                CommandError::IllegalAction("no attackers have been declared this combat".to_string())
            })?;
            if combat.declared_defenders.contains(&command.actor) {
                return Err(CommandError::IllegalAction(
                    "this player has already declared blockers this combat".to_string(),
                ));
            }
            // CR 509.1: a turn-based action, one per defending player (CR
            // 508.5a - multiplayer can have more than one defending player
            // this combat, each individually determined by which attacker
            // is aimed at them or a planeswalker/battle they control, CR
            // 506.4d). `command.actor` is only legal here if some attacker
            // is actually aimed at them.
            // CR 508.5a: "defending player" is resolved against a targeted
            // permanent's *current* controller.
            let is_defending_player = combat.attackers.iter().any(|(_, target)| match target {
                Target::Player(pid) => *pid == command.actor,
                Target::Object(target_id) => state.object(*target_id).is_some_and(|_| {
                    crate::layers::effective_controller(state, script_runtime, *target_id) == command.actor
                }),
            });
            if !is_defending_player {
                return Err(CommandError::IllegalAction(
                    "this player is not a defending player this combat".to_string(),
                ));
            }

            // CR 509.1a: "the defending player chooses one creature for it
            // to block" - a blocker can only appear once across this
            // declaration.
            let mut seen_blockers = Vec::with_capacity(blocks.len());
            for (blocker, attacker) in &blocks {
                if seen_blockers.contains(blocker) {
                    return Err(CommandError::IllegalAction(
                        "the same blocker can't be assigned to block twice".to_string(),
                    ));
                }
                seen_blockers.push(*blocker);

                let blocker_object = state.object(*blocker).ok_or(CommandError::InvalidTarget)?;
                // CR 509.1a: blocking requires *current* control.
                if crate::layers::effective_controller(state, script_runtime, *blocker) != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not control a chosen blocker".to_string(),
                    ));
                }
                if !blocker_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    || blocker_object.definition.card_types.contains(CardTypeSet::BATTLE)
                {
                    return Err(CommandError::IllegalAction(
                        "a chosen blocker is not a creature, or is also a battle".to_string(),
                    ));
                }
                if blocker_object.tapped {
                    return Err(CommandError::IllegalAction(
                        "a chosen blocker is already tapped".to_string(),
                    ));
                }

                let (_, attacking) = combat
                    .attackers
                    .iter()
                    .find(|(id, _)| id == attacker)
                    .ok_or_else(|| {
                        CommandError::IllegalAction(
                            "a chosen attacker isn't actually attacking this combat".to_string(),
                        )
                    })?;
                let attacking_this_defender = match attacking {
                    Target::Player(pid) => *pid == command.actor,
                    Target::Object(target_id) => state.object(*target_id).is_some_and(|_| {
                        crate::layers::effective_controller(state, script_runtime, *target_id) == command.actor
                    }),
                };
                if !attacking_this_defender {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker isn't attacking this defending player, their \
                         planeswalker, or their battle"
                            .to_string(),
                    ));
                }

                // CR 702.9b: an attacker with flying can only be blocked by
                // a creature with flying and/or reach. Reads through
                // effective_keywords (CR 613 layer 6) - a continuous effect
                // granting/removing Flying/Reach now actually changes
                // blocking legality, not just the printed KeywordSet.
                let attacker_object = state.object(*attacker).ok_or(CommandError::InvalidTarget)?;
                let attacker_keywords = effective_keywords(state, script_runtime, attacker_object.object_id);
                let blocker_keywords = effective_keywords(state, script_runtime, blocker_object.object_id);
                if attacker_keywords.contains(KeywordSet::FLYING)
                    && !blocker_keywords.intersects(KeywordSet::FLYING | KeywordSet::REACH)
                {
                    return Err(CommandError::IllegalAction(
                        "an attacker with flying can only be blocked by a creature with \
                         flying or reach"
                            .to_string(),
                    ));
                }

                // CR 702.16d's own protection check below already needs both
                // objects' full effective_characteristics - computed here
                // instead, once, so Fear/Intimidate's color/card-type checks
                // (which need the same live values) can reuse them too.
                let attacker_characteristics =
                    crate::layers::effective_characteristics(state, script_runtime, attacker_object.object_id);
                let blocker_characteristics =
                    crate::layers::effective_characteristics(state, script_runtime, blocker_object.object_id);

                // CR 702.36b/702.13b: Fear/Intimidate - can't be blocked
                // except by an artifact creature and/or (Fear: a black
                // creature; Intimidate: a creature sharing a color with the
                // attacker).
                if attacker_keywords.contains(KeywordSet::FEAR)
                    && !(blocker_characteristics.card_types.contains(CardTypeSet::ARTIFACT)
                        || blocker_characteristics.colors.contains(ColorSet::BLACK))
                {
                    return Err(CommandError::IllegalAction(
                        "an attacker with fear can only be blocked by an artifact and/or \
                         black creature"
                            .to_string(),
                    ));
                }
                if attacker_keywords.contains(KeywordSet::INTIMIDATE)
                    && !(blocker_characteristics.card_types.contains(CardTypeSet::ARTIFACT)
                        || blocker_characteristics.colors.intersects(attacker_characteristics.colors))
                {
                    return Err(CommandError::IllegalAction(
                        "an attacker with intimidate can only be blocked by an artifact \
                         creature and/or a creature sharing a color with it"
                            .to_string(),
                    ));
                }

                // CR 702.28b: Shadow - symmetric, a shadow creature can only
                // block/be blocked by another shadow creature.
                if attacker_keywords.contains(KeywordSet::SHADOW) != blocker_keywords.contains(KeywordSet::SHADOW) {
                    return Err(CommandError::IllegalAction(
                        "shadow creatures can only block or be blocked by other shadow \
                         creatures"
                            .to_string(),
                    ));
                }

                // CR 702.31b: Horsemanship - can't be blocked by a creature
                // without horsemanship (a horsemanship creature may itself
                // block anything, no restriction on that direction).
                if attacker_keywords.contains(KeywordSet::HORSEMANSHIP)
                    && !blocker_keywords.contains(KeywordSet::HORSEMANSHIP)
                {
                    return Err(CommandError::IllegalAction(
                        "an attacker with horsemanship can only be blocked by a creature \
                         with horsemanship"
                            .to_string(),
                    ));
                }

                // CR 702.14c: landwalk - a creature with landwalk can't be
                // blocked at all while the defending player controls at
                // least one land with the specified subtype. Unlike
                // Fear/Intimidate/Shadow/Horsemanship above, this checks the
                // *defending player's own battlefield*, not the blocker's
                // own characteristics - see EffectiveCharacteristics::
                // landwalk_subtypes' own doc comment for why.
                if !attacker_characteristics.landwalk_subtypes.is_empty() || !attacker_characteristics.landwalk_qualities.is_empty() {
                    let defender_controls_matching_land = state.objects.values().any(|land| {
                        if land.zone != ZoneKind::Battlefield
                            || crate::layers::effective_controller(state, script_runtime, land.object_id) != command.actor
                        {
                            return false;
                        }
                        let land_characteristics = crate::layers::effective_characteristics(state, script_runtime, land.object_id);
                        land_characteristics.subtypes.iter().any(|s| attacker_characteristics.landwalk_subtypes.contains(s))
                            // CR 702.14a's combination forms - the land must
                            // satisfy every field the matched `LandwalkQuality`
                            // specifies, in addition to being a land at all
                            // (the "[type]" is always "land plus ...", never
                            // just "artifact"/"snow" alone).
                            || attacker_characteristics.landwalk_qualities.iter().any(|quality| {
                                land_characteristics.card_types.contains(CardTypeSet::LAND)
                                    && quality.subtypes.iter().all(|s| land_characteristics.subtypes.contains(s))
                                    && quality.card_types.is_none_or(|ct| land_characteristics.card_types.intersects(ct))
                                    && quality.supertypes.is_none_or(|st| land_characteristics.supertypes.contains(st))
                                    && quality.excluded_supertypes.is_none_or(|st| !land_characteristics.supertypes.intersects(st))
                            })
                    });
                    if defender_controls_matching_land {
                        return Err(CommandError::IllegalAction(
                            "an attacker with landwalk can't be blocked while the defending \
                             player controls a matching land"
                                .to_string(),
                        ));
                    }
                }

                // CR 702.16d: an attacker with protection from a quality
                // (color, 702.16e, card type, subtype, name, 702.16a,
                // "everything," 702.16j, or a specific player, 702.16k)
                // can't be blocked by a creature matching that quality.
                // Reuses attacker_characteristics/blocker_characteristics
                // computed above (Fear/Intimidate's own doc comment) - a
                // granted color/type/subtype/name or a granted protection
                // all change blocking legality for real, not just the
                // printed values. The player quality compares raw
                // GameObject.controller fields (attacker_object/
                // blocker_object, already in scope), resolved relative to
                // the attacker's own controller - same "You"/"Opponent"
                // resolution target_has_protection_from's own player check
                // uses.
                if attacker_characteristics.protection_everything
                    || attacker_characteristics.protection_colors.intersects(blocker_characteristics.colors)
                    || attacker_characteristics
                        .protection_card_types
                        .intersects(blocker_characteristics.card_types)
                    || attacker_characteristics
                        .protection_subtypes
                        .iter()
                        .any(|s| blocker_characteristics.subtypes.contains(s))
                    || attacker_characteristics
                        .protection_names
                        .iter()
                        .any(|n| *n == blocker_characteristics.name)
                    || attacker_characteristics.protection_players.iter().any(|who| match who {
                        FilterController::You => blocker_object.controller == attacker_object.controller,
                        FilterController::Opponent => blocker_object.controller != attacker_object.controller,
                        FilterController::Any => false,
                    })
                {
                    return Err(CommandError::IllegalAction(
                        "an attacker with protection from a quality can't be blocked by a \
                         creature matching that quality"
                            .to_string(),
                    ));
                }
            }

            // CR 702.111b: an attacker with menace can't be blocked except
            // by two or more creatures - a whole-batch check, since it's
            // about how many blockers ended up assigned to that attacker,
            // not any single pairing. Same effective_keywords treatment as
            // Flying/Reach above.
            let mut blocker_counts: HashMap<ObjectId, u32> = HashMap::new();
            for (_, attacker) in &blocks {
                *blocker_counts.entry(*attacker).or_insert(0) += 1;
            }
            for (attacker, count) in &blocker_counts {
                let attacker_object = state.object(*attacker).ok_or(CommandError::InvalidTarget)?;
                let attacker_keywords = effective_keywords(state, script_runtime, attacker_object.object_id);
                if attacker_keywords.contains(KeywordSet::MENACE) && *count < 2 {
                    return Err(CommandError::IllegalAction(
                        "an attacker with menace can't be blocked except by two or more \
                         creatures"
                            .to_string(),
                    ));
                }
            }

            // CR 702.22h: banding - once a band member has been directly
            // blocked by a creature in this submission, every other member
            // of its band becomes blocked by that same creature too,
            // regardless of that member's own evasion/protection (the
            // worked example under CR 702.22h blocks a swampwalker this way
            // even though the defending player controls a Swamp - the
            // propagated block bypasses every per-pair check this loop just
            // ran, and the whole-batch Menace check above). Deliberately
            // scoped to direct blocks from *this* submission only - CR
            // 702.22d already forces every band member to share one
            // defending player, so no earlier submission this combat could
            // contain a same-band pairing. A member still gets a
            // *propagated* block from every band-mate's own direct block,
            // even one it's already otherwise blocked by (CR 702.22h has no
            // "unless already blocked" carve-out) - only an exact duplicate
            // (blocker, member) pair is skipped.
            let mut propagated: Vec<(ObjectId, ObjectId)> = Vec::new();
            for (blocker, attacker) in &blocks {
                let Some(band) = combat.bands.iter().find(|band| band.contains(attacker)) else { continue };
                for member in band {
                    if member == attacker || !combat.attackers.iter().any(|(id, _)| id == member) {
                        continue;
                    }
                    let already =
                        blocks.iter().chain(propagated.iter()).any(|(b, a)| b == blocker && a == member);
                    if !already {
                        propagated.push((*blocker, *member));
                    }
                }
            }
            blocks.extend(propagated);

            let mut events = Vec::with_capacity(blocks.len());
            for (blocker, attacker) in &blocks {
                events.push(GameEventKind::BlockerDeclared {
                    blocker: *blocker,
                    attacker: *attacker,
                });
            }
            let combat = state.combat.as_mut().expect("checked Some above");
            combat.blocks.extend(blocks);
            combat.declared_defenders.push(command.actor);

            return Ok(CommandOutcome {
                events,
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::AssignCombatDamage { assignments } => {
            // CR 510.1/509.1's turn-based-action shape, same as
            // DeclareAttackers/DeclareBlockers: only during the combat
            // damage step. Which player is allowed to be `command.actor`
            // is resolved further down, once we know which splits (if any)
            // this step actually needs - see the "required chooser" block
            // below `requires_assignment`.
            if state.phase != Phase::Combat
                || !matches!(state.step, Step::CombatDamage | Step::FirstStrikeDamage)
            {
                return Err(CommandError::IllegalAction(
                    "combat damage can only be assigned during a combat damage step"
                        .to_string(),
                ));
            }
            let combat = state.combat.as_ref().ok_or_else(|| {
                CommandError::IllegalAction("no attackers have been declared this combat".to_string())
            })?;
            let step = state.step;

            // CR 510.1a/702.7: a participant that no longer resolves in
            // state.objects (killed by a spell in the priority window before
            // this step, or - the case first strike makes central - by the
            // first-strike step's own damage) never assigns or deals damage,
            // full stop, rather than hard-erroring the whole command. Same
            // treatment for a participant that resolves fine but isn't
            // eligible for *this* damage step: a plain creature during the
            // first-strike step, or a first-strike-only creature (no double
            // strike) during the regular step, sits this step out entirely.
            let attackers: Vec<(ObjectId, Target)> = combat
                .attackers
                .iter()
                .copied()
                .filter(|(attacker, _)| {
                    state.object(*attacker).is_some()
                        && participates_in_damage_step(
                            effective_keywords(state, script_runtime, *attacker),
                            step,
                        )
                })
                .collect();
            // Alive only, deliberately NOT filtered by the blocker's own
            // step-eligibility - which step an attacker's damage is dealt in
            // is decided by the attacker's own keywords, not the blocker's
            // (a first-strike attacker's damage lands on a plain blocker
            // during the first-strike step same as always; the blocker's
            // lack of first strike only decides whether *it* strikes back
            // this step, handled by the separately-filtered `blocks` below).
            let alive_blocks: Vec<(ObjectId, ObjectId)> = combat
                .blocks
                .iter()
                .copied()
                .filter(|(blocker, _)| state.object(*blocker).is_some())
                .collect();
            let blocks: Vec<(ObjectId, ObjectId)> = alive_blocks
                .iter()
                .copied()
                .filter(|(blocker, _)| {
                    participates_in_damage_step(effective_keywords(state, script_runtime, *blocker), step)
                })
                .collect();

            // CR 509.1h/510.1c: "blocked" is decided once, at declare-
            // blockers time, and doesn't change even if every blocker
            // assigned to an attacker later leaves combat (dies to a spell,
            // etc) before damage is assigned — a blocked attacker with zero
            // *living* blockers still deals no combat damage at all, unless
            // it has trample (CR 702.19d, the one case where it's treated as
            // though every blocker got lethal damage). Computed from the raw
            // `combat.blocks` (unfiltered by aliveness), deliberately
            // distinct from `blockers_by_attacker` below (which only tracks
            // *living* blockers) - conflating the two was a real pre-
            // existing bug: an attacker whose blockers all died before this
            // step fell into the same "no entry" case as a never-blocked
            // attacker and incorrectly dealt full damage to the defending
            // player regardless of trample.
            let declared_blocked_attackers: HashSet<ObjectId> =
                combat.blocks.iter().map(|(_, attacker)| *attacker).collect();

            // CR 510.1c: a real split only exists for an attacker with 2+
            // living blockers, or exactly 1 living blocker if it has trample
            // (CR 702.19b - even a single blocker needs a chosen lethal-vs-
            // excess split once trample is in play). Every other attacker
            // (0 living blockers, or 1 with no trample) has exactly one
            // legal place for its damage to go, computed below rather than
            // chosen. Computed from `alive_blocks` (not step-filtered - see
            // above), so a dead blocker is never required in a split, but a
            // step-ineligible one still is (its attacker's damage still has
            // to go somewhere this step).
            let mut blockers_by_attacker: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
            for (blocker, attacker) in &alive_blocks {
                blockers_by_attacker.entry(*attacker).or_default().push(*blocker);
            }
            // Also scoped to attackers that themselves participate this step
            // (alive and step-eligible) - a dead or step-ineligible attacker
            // deals no damage this step at all, so it can't need a split
            // either, even if 2+ of its blockers happen to be eligible now.
            let participating_attackers: HashSet<ObjectId> =
                attackers.iter().map(|(attacker, _)| *attacker).collect();
            let mut requires_assignment: HashSet<ObjectId> = blockers_by_attacker
                .iter()
                .filter(|(attacker, blockers)| {
                    participating_attackers.contains(attacker)
                        && (blockers.len() >= 2
                            || (blockers.len() == 1
                                && effective_keywords(state, script_runtime, **attacker)
                                    .contains(KeywordSet::TRAMPLE)))
                })
                .map(|(attacker, _)| *attacker)
                .collect();
            // CR 702.22h/k: banding can force a single blocker onto 2+
            // attackers (band propagation in DeclareBlockers) - that blocker
            // then needs the same kind of chosen split `blockers_by_attacker`
            // above already models for a multi-blocked attacker. Built from
            // the step-filtered `blocks` (not `alive_blocks`) since only a
            // blocker that itself deals damage this step can need one.
            let mut attackers_by_blocker: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
            for (blocker, attacker) in &blocks {
                attackers_by_blocker.entry(*blocker).or_default().push(*attacker);
            }
            requires_assignment.extend(
                attackers_by_blocker.iter().filter(|(_, attackers)| attackers.len() >= 2).map(|(blocker, _)| *blocker),
            );

            // CR 702.22j/k's second trigger condition: independent of
            // banding entirely (and independent of any declared band - this
            // is a pure grouping-by-block check at the damage step), the
            // chooser also flips when `ids` contains a creature X with
            // "bands with other [quality]" for some quality Q and another
            // creature Y (Y != X) whose own subtypes include Q. Tries every
            // quality X names in turn, same "any one match is enough"
            // posture `commands::PlayerActionKind::DeclareBands`'s own
            // quality-band check already takes.
            let has_bands_with_other_pair = |ids: &[ObjectId]| -> bool {
                ids.iter().any(|x| {
                    let x_characteristics = crate::layers::effective_characteristics(state, script_runtime, *x);
                    x_characteristics.bands_with_other_subtypes.iter().any(|quality| {
                        ids.iter().any(|y| {
                            y != x
                                && crate::layers::effective_characteristics(state, script_runtime, *y)
                                    .subtypes
                                    .contains(quality)
                        })
                    })
                })
            };

            // CR 510.1c/d: a split is chosen by the object's own controller
            // by default - attacker splits by the attacker's controller,
            // blocker splits by the blocker's controller. CR 702.22j/k flip
            // that per-id when either trigger condition holds on the
            // *other* side of that particular block: a plain `BANDING`
            // creature there, or `has_bands_with_other_pair` above. An
            // attacker blocked by either has its split chosen by the
            // defending player instead (702.22j); a blocker blocking either
            // has its split chosen by the active player instead (702.22k -
            // this also covers the pre-existing band-forced-blocker case
            // for real now, rather than by the old blanket-active-player
            // hardcode's accident: every band needs >=1 banding member, so
            // a band-forced blocker split was always going to resolve to
            // "active player" here anyway).
            let attacker_defending_player = |attacker: &ObjectId| -> PlayerId {
                let (_, target) =
                    combat.attackers.iter().find(|(id, _)| id == attacker).expect("attacker is attacking");
                match target {
                    Target::Player(pid) => *pid,
                    Target::Object(target_id) => crate::layers::effective_controller(state, script_runtime, *target_id),
                }
            };
            let mut required_actors: HashSet<PlayerId> = HashSet::new();
            for id in &requires_assignment {
                let actor = if let Some(blockers) = blockers_by_attacker.get(id) {
                    let banding_blocker = blockers
                        .iter()
                        .any(|b| effective_keywords(state, script_runtime, *b).contains(KeywordSet::BANDING))
                        || has_bands_with_other_pair(blockers);
                    if banding_blocker {
                        attacker_defending_player(id)
                    } else {
                        crate::layers::effective_controller(state, script_runtime, *id)
                    }
                } else {
                    let attackers_for_blocker = &attackers_by_blocker[id];
                    let banding_attacker = attackers_for_blocker
                        .iter()
                        .find(|a| effective_keywords(state, script_runtime, **a).contains(KeywordSet::BANDING));
                    match banding_attacker {
                        Some(attacker) => crate::layers::effective_controller(state, script_runtime, *attacker),
                        None if has_bands_with_other_pair(attackers_for_blocker) => state.active_player,
                        None => crate::layers::effective_controller(state, script_runtime, *id),
                    }
                };
                required_actors.insert(actor);
            }
            if required_actors.len() > 1 {
                return Err(CommandError::IllegalAction(
                    "this combat damage step needs splits chosen by two different players at \
                     once (e.g. a plain multi-block plus a separate banding-blocked attacker) - \
                     not yet supported, see AssignCombatDamage's own doc comment"
                        .to_string(),
                ));
            }
            // No split needed at all this step - same turn-based-action
            // convention DeclareAttackers/DeclareBlockers use, active
            // player only (matches every pre-existing zero-split test).
            let required_actor = required_actors.into_iter().next().unwrap_or(state.active_player);
            if command.actor != required_actor {
                return Err(CommandError::IllegalAction(
                    "actor is not the player who chooses this combat damage step's split(s)"
                        .to_string(),
                ));
            }

            // CR 510.1e atomicity: `assignments` must cover exactly the
            // set that needs one - no extra (nothing to split for an
            // attacker with 0, or 1 non-trampling, blocker; nothing to split
            // for a blocker blocking only 1 attacker), no missing (the
            // engine can't guess a split).
            let submitted: HashSet<ObjectId> = assignments.iter().map(|(id, _)| *id).collect();
            if submitted != requires_assignment {
                return Err(CommandError::IllegalAction(
                    "assignments must cover exactly the attackers and blockers that need a \
                     chosen split (two or more blockers on an attacker, one blocker with \
                     trample, or a blocker forced onto two or more attackers by banding)"
                        .to_string(),
                ));
            }

            for (id, splits) in &assignments {
                if state.object(*id).is_none() {
                    return Err(CommandError::InvalidTarget);
                }
                // CR 510.1a: 0-or-less power assigns no damage. Reads through
                // effective_power (CR 613 layer 7c) so this validation stays
                // consistent with the actual damage-event amounts computed
                // below, which use the same helper.
                let power = effective_power(state, script_runtime, *id).map(|p| p.max(0) as u32).unwrap_or(0);

                if let Some(blockers) = blockers_by_attacker.get(id) {
                    let attacker_keywords = effective_keywords(state, script_runtime, *id);
                    let has_trample = attacker_keywords.contains(KeywordSet::TRAMPLE);
                    let has_deathtouch = attacker_keywords.contains(KeywordSet::DEATHTOUCH);

                    let mut seen = Vec::with_capacity(splits.len());
                    let mut sum: u32 = 0;
                    for (blocker, amount) in splits {
                        if !blockers.contains(blocker) {
                            return Err(CommandError::IllegalAction(
                                "a split entry's blocker isn't actually blocking that attacker"
                                    .to_string(),
                            ));
                        }
                        if seen.contains(blocker) {
                            return Err(CommandError::IllegalAction(
                                "a split can't assign the same blocker twice".to_string(),
                            ));
                        }
                        seen.push(*blocker);
                        sum += amount;
                    }
                    if seen.len() != blockers.len() {
                        return Err(CommandError::IllegalAction(
                            "a split must assign an amount (possibly zero) to every blocker"
                                .to_string(),
                        ));
                    }
                    if !has_trample {
                        if sum != power {
                            return Err(CommandError::IllegalAction(
                                "a split's amounts must sum to exactly the attacker's power".to_string(),
                            ));
                        }
                    } else {
                        // CR 702.19b: a trampler may send the remainder to the
                        // defending player (handled in the assign-and-deal pass
                        // below), but only once every blocker in the split has
                        // been given at least its own lethal amount - short one
                        // and *nothing* can go to the player, all of `power`
                        // must stay on blockers instead.
                        if sum > power {
                            return Err(CommandError::IllegalAction(
                                "a split's amounts can't sum to more than the attacker's power".to_string(),
                            ));
                        }
                        if sum < power {
                            for (blocker, amount) in splits {
                                let needed =
                                    lethal_damage_needed(state, script_runtime, *blocker, has_deathtouch);
                                if *amount < needed {
                                    return Err(CommandError::IllegalAction(
                                        "trample can't send excess damage to the defending player until \
                                         every blocker has been assigned its own lethal amount"
                                            .to_string(),
                                    ));
                                }
                            }
                        }
                    }
                } else {
                    // CR 702.22h/k: the blocker-side case - a blocker forced
                    // onto 2+ attackers by band propagation. No trample-style
                    // carve-out here: a blocker has no "spill to the
                    // defending player" outlet, so its split must sum to
                    // exactly its power every time.
                    let attackers_for_blocker = &attackers_by_blocker[id];
                    let mut seen = Vec::with_capacity(splits.len());
                    let mut sum: u32 = 0;
                    for (attacker, amount) in splits {
                        if !attackers_for_blocker.contains(attacker) {
                            return Err(CommandError::IllegalAction(
                                "a split entry's attacker isn't actually blocked by that blocker"
                                    .to_string(),
                            ));
                        }
                        if seen.contains(attacker) {
                            return Err(CommandError::IllegalAction(
                                "a split can't assign the same attacker twice".to_string(),
                            ));
                        }
                        seen.push(*attacker);
                        sum += amount;
                    }
                    if seen.len() != attackers_for_blocker.len() {
                        return Err(CommandError::IllegalAction(
                            "a split must assign an amount (possibly zero) to every attacker \
                             it blocks"
                                .to_string(),
                        ));
                    }
                    if sum != power {
                        return Err(CommandError::IllegalAction(
                            "a split's amounts must sum to exactly the blocker's power".to_string(),
                        ));
                    }
                }
            }

            let assignment_map: HashMap<ObjectId, &Vec<(ObjectId, u32)>> =
                assignments.iter().map(|(id, splits)| (*id, splits)).collect();

            // CR 510.1b/c/d + 510.2: assign and deal in the same pass - no
            // priority window between them.
            let mut events = Vec::new();
            for (attacker, target) in &attackers {
                // CDA power (PTValue::CharacteristicDefining, layer 7a) now
                // resolves for real if the card's base value carries a
                // script - a script-less/erroring CDA still skips rather
                // than guesses, same stance sba.rs's toughness check
                // already takes. effective_power also folds in any
                // applicable ModifyPT/SetPT continuous effect (layer 7c) on
                // top of that.
                let Some(power) = effective_power(state, script_runtime, *attacker) else {
                    continue;
                };
                let power = power.max(0) as u32;
                if power == 0 {
                    continue;
                }

                let has_trample =
                    effective_keywords(state, script_runtime, *attacker).contains(KeywordSet::TRAMPLE);

                match blockers_by_attacker.get(attacker) {
                    None if declared_blocked_attackers.contains(attacker) && !has_trample => {
                        // CR 509.1h/510.1c: blocked, but every blocker is
                        // gone by damage time, no trample - deals no combat
                        // damage at all, not "falls back to the original
                        // target" (that's only the genuinely-unblocked case
                        // below, or the trample case CR 702.19d covers).
                    }
                    None => {
                        // 510.1b (never blocked), or 702.19d (blocked, no
                        // living blockers left, trample - as though every
                        // blocker got lethal damage) - all damage to the
                        // original target either way.
                        events.push(GameEventKind::DamageDealt {
                            source: *attacker,
                            target: *target,
                            amount: power,
                            is_combat_damage: true,
                        });
                    }
                    Some(blockers) if blockers.len() == 1 && !has_trample => {
                        // 510.1c: exactly one blocker, no trample - no
                        // choice, all damage there.
                        events.push(GameEventKind::DamageDealt {
                            source: *attacker,
                            target: Target::Object(blockers[0]),
                            amount: power,
                            is_combat_damage: true,
                        });
                    }
                    Some(_) => {
                        // 510.1c (2+ blockers) or a single trampling blocker
                        // (CR 702.19b) - the validated split above, plus
                        // whatever it left unassigned spilling to the
                        // original target (CR 702.19b's "excess... assigned
                        // as its controller chooses"; validation above
                        // already confirmed every blocker got its lethal
                        // amount whenever this remainder is nonzero).
                        let mut assigned_to_blockers = 0u32;
                        for (blocker, amount) in assignment_map[attacker] {
                            assigned_to_blockers += *amount;
                            if *amount > 0 {
                                events.push(GameEventKind::DamageDealt {
                                    source: *attacker,
                                    target: Target::Object(*blocker),
                                    amount: *amount,
                                    is_combat_damage: true,
                                });
                            }
                        }
                        let remainder = power - assigned_to_blockers;
                        if remainder > 0 {
                            events.push(GameEventKind::DamageDealt {
                                source: *attacker,
                                target: *target,
                                amount: remainder,
                                is_combat_damage: true,
                            });
                        }
                    }
                }
            }
            // 510.1d: a blocker deals its damage to the one attacker it
            // blocks - unambiguous, unless banding forced it onto 2+
            // attackers (CR 702.22h/k), in which case the validated split
            // above decides. `handled_blockers` collapses `blocks`'s
            // possibly-repeated blocker entries (one per attacker it's
            // forced onto) down to one pass per distinct blocker.
            let mut handled_blockers: HashSet<ObjectId> = HashSet::new();
            for (blocker, _) in &blocks {
                if !handled_blockers.insert(*blocker) {
                    continue;
                }
                let Some(power) = effective_power(state, script_runtime, *blocker) else {
                    continue;
                };
                let power = power.max(0) as u32;
                if power == 0 {
                    continue;
                }
                let attackers_for_blocker = &attackers_by_blocker[blocker];
                if let [only_attacker] = attackers_for_blocker[..] {
                    events.push(GameEventKind::DamageDealt {
                        source: *blocker,
                        target: Target::Object(only_attacker),
                        amount: power,
                        is_combat_damage: true,
                    });
                } else {
                    for (attacker, amount) in assignment_map[blocker] {
                        if *amount > 0 {
                            events.push(GameEventKind::DamageDealt {
                                source: *blocker,
                                target: Target::Object(*attacker),
                                amount: *amount,
                                is_combat_damage: true,
                            });
                        }
                    }
                }
            }

            // CR 510.2/601.2a: these are proposed events only - engine.rs's
            // ReplacementCheck (CR 614-616) gets a chance to rewrite or
            // prevent each one (see apply_damage_event's own doc comment for
            // why the real mutation isn't performed here anymore).
            return Ok(CommandOutcome {
                events,
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        _ => {}
    }
    // TODO: match command.action (PlayerActionKind) and handle each variant:
    //   CastSpell is implemented above.
    //   ActivateAbility (mana-ability case) is implemented above; non-mana
    //     abilities fall through to todo!() pending the script bridge.
    //   PassPriority/PlayLand are implemented above (the match arms before
    //     this comment block) - everything else here is still unimplemented.
    //   DeclareAttackers/DeclareBlockers are implemented above (declarations
    //     only - no combat damage resolution, no triggered-ability
    //     collection beyond the AttackerDeclared/BlockerDeclared events
    //     themselves, no costs-to-attack/block, no restrictions check).
    //   SubmitPayment is NOT handled here, deliberately - see
    //     Engine::submit_payment (engine.rs) and CommandOutcome::
    //     requires_payment's doc comment above. Payment answers
    //     ProcessState::AwaitingPayment, a mid-pipeline pause, the same way
    //     submit_replacement_choice/submit_trigger_order answer their own
    //     pauses - never goes through submit_action/execute_command at all.
    //   MakeChoice: generic - depends entirely on what asked for the
    //     choice. broke_pass_chain = true, priority_handoff = Unchanged.
    // Every branch ends in returning
    // Ok(CommandOutcome { events, broke_pass_chain, priority_handoff }).
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::counter::CounterSet;
    use crate::types::effect::{
        AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload,
        EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
    };
    use crate::types::game::RngState;
    use crate::types::ids::SequenceNo;
    use crate::types::mana::ManaPool;
    use crate::types::player::PlayerState;

    fn minimal_state() -> GameState {
        let p0 = PlayerId(0);
        GameState {
            players: vec![PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            }],
            turn_order: vec![p0],
            active_player: p0,
            turn_number: 1,
            phase: Phase::PreGame,
            step: Step::Untap,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: HashMap::new(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
        }
    }

    fn dummy_ability(source: ObjectId) -> RegisteredAbility {
        RegisteredAbility {
            id: AbilityId(0),
            source,
            kind: AbilityKind::Triggered,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }
    }

    fn dummy_continuous_effect(source: ObjectId) -> ContinuousEffect {
        ContinuousEffect {
            source,
            layer: Layer::PowerToughnessModifying,
            affected: AffectedFilter::Primitive(EffectFilter {
                controller: None,
                card_types: None,
                self_only: true,
            }),
            effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
            timestamp: SequenceNo(0),
            duration: EffectDuration::Permanent,
            condition: None,
        }
    }

    /// `phase5/static-regeneration`: a `Repeatable` `WouldDie`/`Regenerate`
    /// shield, the shape `Game.CreateReplacementEffect` produces — the first
    /// durable `state.replacement_effects` entry that ever needed
    /// `retire_object` to purge it (every prior entry was `OneShot`/
    /// `LimitedUses` and self-consumed via `apply_one_replacement`).
    fn dummy_replacement_effect(source: ObjectId) -> ReplacementEffect {
        ReplacementEffect {
            source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::WouldDie {
                    affected: EffectFilter { controller: None, card_types: None, self_only: true },
                },
                payload: ReplacementPayload::Regenerate,
            },
            consumption: ConsumptionPolicy::Repeatable,
        }
    }

    #[test]
    fn retire_object_purges_only_the_retired_source() {
        let mut state = minimal_state();
        state.abilities.push(dummy_ability(ObjectId(1)));
        state.abilities.push(dummy_ability(ObjectId(2)));
        state.continuous_effects.push(dummy_continuous_effect(ObjectId(1)));
        state.continuous_effects.push(dummy_continuous_effect(ObjectId(2)));
        state.replacement_effects.push(dummy_replacement_effect(ObjectId(1)));
        state.replacement_effects.push(dummy_replacement_effect(ObjectId(2)));

        let departed = retire_object(&mut state, ObjectId(1));

        assert_eq!(departed.len(), 1, "the retired object's own Triggered ability is returned");
        assert_eq!(departed[0].source, ObjectId(1));
        assert_eq!(state.abilities.len(), 1, "only ObjectId(1)'s ability should be purged");
        assert_eq!(state.abilities[0].source, ObjectId(2), "ObjectId(2)'s ability must survive untouched");
        assert_eq!(
            state.continuous_effects.len(),
            1,
            "only ObjectId(1)'s continuous effect should be purged"
        );
        assert_eq!(
            state.continuous_effects[0].source,
            ObjectId(2),
            "ObjectId(2)'s continuous effect must survive untouched"
        );
        assert_eq!(
            state.replacement_effects.len(),
            1,
            "only ObjectId(1)'s replacement effect should be purged"
        );
        assert_eq!(
            state.replacement_effects[0].source,
            ObjectId(2),
            "ObjectId(2)'s replacement effect must survive untouched"
        );
    }
}
