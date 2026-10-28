//! The `AssignCombatDamage` command body: ordering, first and double strike,
//! lethal-damage math and event building. See `DESIGN.md` for the rest.

use std::collections::{HashMap, HashSet};

use crate::script::ScriptRuntime;
use crate::types::card::KeywordSet;
use crate::types::combat::CombatState;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::turn::{Phase, Step};

use super::super::legality::{effective_keywords, effective_power, lethal_damage_needed, participates_in_damage_step};
use super::super::{CommandOutcome, Internal, PriorityHandoff, Rejected, Validated};

/// Who takes part in this damage step and which of them need a chosen split
/// (CR 510.1c). `execute_assign_combat_damage` deals the damage from it, and
/// the damage Decision lists the splits that are still missing.
pub(crate) struct SplitPlan<'a> {
    pub combat: &'a CombatState,
    /// Attackers that are alive and strike in this step.
    pub attackers: Vec<(ObjectId, Target)>,
    /// `alive_blocks` limited to blockers that strike in this step.
    pub blocks: Vec<(ObjectId, ObjectId)>,
    pub declared_blocked_attackers: HashSet<ObjectId>,
    pub blockers_by_attacker: HashMap<ObjectId, Vec<ObjectId>>,
    pub attackers_by_blocker: HashMap<ObjectId, Vec<ObjectId>>,
    /// Attackers and blockers that need a split this step.
    pub requires_assignment: HashSet<ObjectId>,
    /// The player who chooses each split (CR 702.22j/k can flip it).
    pub required_actor_by_id: HashMap<ObjectId, PlayerId>,
}

/// `None` when this step's damage is already assigned. An `Err` means it is
/// not a combat damage step, or there is no combat.
pub(crate) fn plan_damage_splits<'a>(
    state: &'a GameState,
    script_runtime: &ScriptRuntime,
) -> Result<Option<SplitPlan<'a>>, Rejected> {
    // CR 510.1/509.1
    // A turn-based action like DeclareAttackers and DeclareBlockers, allowed only in
    // the combat damage step. Who may be `command.actor` is decided below, once it
    // is known which splits this step needs (see the required-chooser block after
    // `requires_assignment`).
    if state.phase != Phase::Combat
        || !matches!(state.step, Step::CombatDamage | Step::FirstStrikeDamage)
    {
        return Err(Rejected::IllegalAction(
            "combat damage can only be assigned during a combat damage step"
                .to_string(),
        ));
    }
    let combat = state.combat.as_ref().ok_or_else(|| {
        Rejected::IllegalAction("no attackers have been declared this combat".to_string())
    })?;
    let step = state.step;

    // This step's damage was already assigned and dealt, either by the auto-fire
    // attempt in `engine::turn` or by an earlier submission. CR 510 has no
    // assign-twice case, so a redundant submission (such as an empty
    // `AssignCombatDamage`) is a harmless no-op.
    if combat.damage_assigned_this_step {
        return Ok(None);
    }

    // CR 510.1a/702.7
    // A participant missing from state.objects (killed before this step, or by the
    // first-strike step's damage) never assigns or deals damage, and does not error
    // the command. The same goes for one not eligible for this step: a plain
    // creature in the first-strike step, or a first-strike-only creature in the
    // regular step.
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
    // Alive only, and not filtered by the blocker's step eligibility: the attacker's
    // own keywords decide which step its damage lands in. The blocker's lack of
    // first strike only decides whether it strikes back this step, which the
    // filtered `blocks` below handles.
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

    // CR 509.1h/510.1c
    // "Blocked" is fixed at declare-blockers time and survives every blocker leaving
    // combat before damage. A blocked attacker with no living blockers deals no
    // combat damage unless it has trample (CR 702.19d). Computed from the raw
    // `combat.blocks`, unlike `blockers_by_attacker` below, which tracks only living
    // blockers: conflating them made such an attacker deal full damage to the
    // defending player regardless of trample. `combat.effect_blocked` (CR
    // 509.1h/702.22i: blocked by an effect, with no real blocker) joins the same set
    // for the same reason.
    let declared_blocked_attackers: HashSet<ObjectId> = combat
        .blocks
        .iter()
        .map(|(_, attacker)| *attacker)
        .chain(combat.effect_blocked.iter().copied())
        .collect();

    // CR 510.1c
    // A real split exists only for an attacker with 2+ living blockers, or one
    // living blocker if it has trample (CR 702.19b). Any other attacker has one
    // legal place for its damage, computed below. Built from `alive_blocks`, so a
    // dead blocker is never in a split but a step-ineligible one still is, since its
    // attacker's damage still has to go somewhere.
    let mut blockers_by_attacker: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (blocker, attacker) in &alive_blocks {
        blockers_by_attacker.entry(*attacker).or_default().push(*blocker);
    }
    // Scoped to attackers that participate this step (alive and step-eligible): a
    // dead or step-ineligible attacker deals no damage, so it needs no split.
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
    // CR 702.22h/k
    // Banding can force one blocker onto 2+ attackers (band propagation in
    // DeclareBlockers), and that blocker needs a chosen split like a multi-blocked
    // attacker. Built from the step-filtered `blocks`, since only a blocker that
    // deals damage this step can need one.
    let mut attackers_by_blocker: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (blocker, attacker) in &blocks {
        attackers_by_blocker.entry(*blocker).or_default().push(*attacker);
    }
    requires_assignment.extend(
        attackers_by_blocker.iter().filter(|(_, attackers)| attackers.len() >= 2).map(|(blocker, _)| *blocker),
    );

    // CR 702.22j/k
    // The chooser also flips, independent of banding and of any declared band, when
    // `ids` contains a creature X with "bands with other [quality]" for some quality
    // Q and another creature Y whose subtypes include Q. Tries every quality X
    // names, as `PlayerActionKind::DeclareBands`'s quality check does.
    let has_bands_with_other_pair = |ids: &[ObjectId]| -> bool {
        ids.iter().any(|x| {
            let x_characteristics = crate::layers::effective_characteristics(state, script_runtime, *x);
            let subtype_match = x_characteristics.bands_with_other_subtypes.iter().any(|quality| {
                ids.iter().any(|y| {
                    y != x
                        && crate::layers::effective_characteristics(state, script_runtime, *y)
                            .subtypes
                            .contains(quality)
                })
            });
            // CR 702.22c/j/k
            // The supertype form: the same pairwise check against each candidate's
            // `supertypes` bitset instead of a `Vec<Subtype>`.
            let supertype_match = x_characteristics.bands_with_other_supertypes.iter().any(|quality| {
                ids.iter().any(|y| {
                    y != x
                        && crate::layers::effective_characteristics(state, script_runtime, *y)
                            .supertypes
                            .contains(quality)
                })
            });
            subtype_match || supertype_match
        })
    };

    // CR 510.1c/d
    // A split is chosen by the object's own controller by default (attacker splits
    // by the attacker's controller, blocker splits by the blocker's). CR 702.22j/k
    // flip that per id when the other side of that block has a plain `BANDING`
    // creature or `has_bands_with_other_pair`: an attacker so blocked has its split
    // chosen by the defending player (702.22j), and a blocker so blocking by the
    // active player (702.22k). This also covers a band-forced blocker, since every
    // band has at least one banding member.
    let attacker_defending_player = |attacker: &ObjectId| -> PlayerId {
        let (_, target) =
            combat.attackers.iter().find(|(id, _)| id == attacker).expect("attacker is attacking");
        match target {
            Target::Player(pid) => *pid,
            Target::Object(target_id) => crate::layers::effective_controller(state, script_runtime, *target_id),
            // Structurally unreachable: DeclareAttackers already rejects
            // Target::Ability (an attacker never targets an ability).
            Target::Ability(_) => unreachable!("an attacker's own combat target is never an ability"),
        }
    };
    // CR 510.1c/d/702.22j/k
    // Which player chooses each id's split. A map and not a set, because two ids in
    // one step can have different choosers (a multi-blocked attacker chosen by its
    // controller beside a Banding-blocked attacker chosen by the defending player).
    let mut required_actor_by_id: HashMap<ObjectId, PlayerId> = HashMap::new();
    for id in &requires_assignment {
        let actor_for_id = if let Some(blockers) = blockers_by_attacker.get(id) {
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
        required_actor_by_id.insert(*id, actor_for_id);
    }
    Ok(Some(SplitPlan {
        combat,
        attackers,
        blocks,
        declared_blocked_attackers,
        blockers_by_attacker,
        attackers_by_blocker,
        requires_assignment,
        required_actor_by_id,
    }))
}

/// What `apply_assign_combat_damage` does, decided by `validate_assign_combat_damage`.
#[derive(Debug)]
pub(crate) enum DamagePlan {
    /// This step's damage was already dealt, so a repeat submission does nothing.
    AlreadyAssigned,
    /// This actor's share is recorded while another chooser's share is still missing.
    Record { assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)> },
    /// Every required share is in. The damage events are built and the step is marked done.
    Deal { events: Vec<GameEventKind> },
}

/// `engine::turn::resolve_advance_turn_step_state` calls this directly on
/// entering the damage step (its auto-fire attempt), outside the normal
/// `execute_command` dispatch.
pub(crate) fn execute_assign_combat_damage(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<CommandOutcome, super::super::CommandError> {
    let plan = validate_damage(state, script_runtime, actor, assignments)?;
    Ok(apply_damage(state, plan)?)
}

pub(crate) fn validate_assign_combat_damage(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<Validated, Rejected> {
    validate_damage(state, script_runtime, actor, assignments).map(Validated::AssignCombatDamage)
}

pub(crate) fn apply_assign_combat_damage(state: &mut GameState, plan: DamagePlan) -> Result<CommandOutcome, Internal> {
    apply_damage(state, plan)
}

fn validate_damage(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<DamagePlan, Rejected> {
    let Some(SplitPlan {
        combat,
        attackers,
        blocks,
        declared_blocked_attackers,
        blockers_by_attacker,
        attackers_by_blocker,
        requires_assignment,
        required_actor_by_id,
    }) = plan_damage_splits(state, script_runtime)?
    else {
        // Already assigned and dealt, so a redundant submission is a no-op.
        return Ok(DamagePlan::AlreadyAssigned);
    };
    // Splits another actor already submitted this step (the same shape as CR
    // 508.5a's `declared_defenders`: a turn-based action that can need input from
    // more than one player). Cloned so nothing below juggles a `state.combat`
    // borrow beside the `effective_*(state, ...)` calls.
    let already_submitted = combat.pending_damage_splits.clone();
    let still_needed: HashSet<ObjectId> =
        requires_assignment.iter().copied().filter(|id| !already_submitted.contains_key(id)).collect();
    // This actor's remaining share: not the whole step's `requires_assignment`
    // (which may include ids a different actor chooses) and not ids an earlier call
    // covered.
    let this_actors_ids: HashSet<ObjectId> = still_needed
        .iter()
        .copied()
        .filter(|id| required_actor_by_id.get(id) == Some(&actor))
        .collect();
    if requires_assignment.is_empty() {
        // No split needed this step: a turn-based action for the active player only, as
        // with DeclareAttackers and DeclareBlockers.
        if actor != state.active_player {
            return Err(Rejected::IllegalAction(
                "actor is not the player who chooses this combat damage step's split(s)"
                    .to_string(),
            ));
        }
    } else if this_actors_ids.is_empty() {
        return Err(Rejected::IllegalAction(
            "actor is not the player who chooses any of this combat damage step's still-\
             pending split(s) (either not a chooser at all this step, or already submitted \
             their own share)"
                .to_string(),
        ));
    }

    // CR 510.1e
    // Atomic per actor: `assignments` must cover exactly the ids this actor chooses
    // and has not yet submitted, with none extra, none missing and none belonging to
    // a different required actor (that player submits their own share separately).
    let submitted: HashSet<ObjectId> = assignments.iter().map(|(id, _)| *id).collect();
    if submitted != this_actors_ids {
        return Err(Rejected::IllegalAction(
            "assignments must cover exactly this actor's own share of the attackers and \
             blockers that need a chosen split this step (two or more blockers on an \
             attacker, one blocker with trample, or a blocker forced onto two or more \
             attackers by banding) - a different required actor's own share, if any, is \
             submitted in a separate call"
                .to_string(),
        ));
    }

    for (id, splits) in &assignments {
        if state.object(*id).is_none() {
            return Err(Rejected::InvalidTarget);
        }
        // CR 510.1a
        // Power of 0 or less assigns no damage. Reads effective_power (CR 613 layer 7c)
        // so validation matches the damage amounts computed below.
        let power = effective_power(state, script_runtime, *id).map(|p| p.max(0) as u32).unwrap_or(0);

        if let Some(blockers) = blockers_by_attacker.get(id) {
            let attacker_keywords = effective_keywords(state, script_runtime, *id);
            let has_trample = attacker_keywords.contains(KeywordSet::TRAMPLE);
            let has_deathtouch = attacker_keywords.contains(KeywordSet::DEATHTOUCH);

            let mut seen = Vec::with_capacity(splits.len());
            let mut sum: u32 = 0;
            for (blocker, amount) in splits {
                if !blockers.contains(blocker) {
                    return Err(Rejected::IllegalAction(
                        "a split entry's blocker isn't actually blocking that attacker"
                            .to_string(),
                    ));
                }
                if seen.contains(blocker) {
                    return Err(Rejected::IllegalAction(
                        "a split can't assign the same blocker twice".to_string(),
                    ));
                }
                seen.push(*blocker);
                sum += amount;
            }
            if seen.len() != blockers.len() {
                return Err(Rejected::IllegalAction(
                    "a split must assign an amount (possibly zero) to every blocker"
                        .to_string(),
                ));
            }
            if !has_trample {
                if sum != power {
                    return Err(Rejected::IllegalAction(
                        "a split's amounts must sum to exactly the attacker's power".to_string(),
                    ));
                }
            } else {
                // CR 702.19b
                // A trampler may send the remainder to the defending player (in the
                // assign-and-deal pass below) only once every blocker in the split has at least
                // its lethal amount. Short one blocker, nothing goes to the player and all of
                // `power` must stay on blockers.
                if sum > power {
                    return Err(Rejected::IllegalAction(
                        "a split's amounts can't sum to more than the attacker's power".to_string(),
                    ));
                }
                if sum < power {
                    for (blocker, amount) in splits {
                        let needed =
                            lethal_damage_needed(state, script_runtime, *blocker, has_deathtouch);
                        if *amount < needed {
                            return Err(Rejected::IllegalAction(
                                "trample can't send excess damage to the defending player until \
                                 every blocker has been assigned its own lethal amount"
                                    .to_string(),
                            ));
                        }
                    }
                }
            }
        } else {
            // CR 702.22h/k
            // The blocker-side case: a blocker forced onto 2+ attackers by band propagation.
            // A blocker has no spill outlet like trample, so its split must sum to exactly
            // its power.
            let attackers_for_blocker = &attackers_by_blocker[id];
            let mut seen = Vec::with_capacity(splits.len());
            let mut sum: u32 = 0;
            for (attacker, amount) in splits {
                if !attackers_for_blocker.contains(attacker) {
                    return Err(Rejected::IllegalAction(
                        "a split entry's attacker isn't actually blocked by that blocker"
                            .to_string(),
                    ));
                }
                if seen.contains(attacker) {
                    return Err(Rejected::IllegalAction(
                        "a split can't assign the same attacker twice".to_string(),
                    ));
                }
                seen.push(*attacker);
                sum += amount;
            }
            if seen.len() != attackers_for_blocker.len() {
                return Err(Rejected::IllegalAction(
                    "a split must assign an amount (possibly zero) to every attacker \
                     it blocks"
                        .to_string(),
                ));
            }
            if sum != power {
                return Err(Rejected::IllegalAction(
                    "a split's amounts must sum to exactly the blocker's power".to_string(),
                ));
            }
        }
    }

    // Everyone required has submitted once this actor's share joins the recorded
    // shares (CR 508.5a's `declared_defenders` applied to a real payload).
    let mut assignment_map: HashMap<ObjectId, Vec<(ObjectId, u32)>> = already_submitted;
    for (id, splits) in &assignments {
        assignment_map.insert(*id, splits.clone());
    }
    if assignment_map.len() < requires_assignment.len() {
        // Still waiting on another required actor's share. CR 510.2's simultaneous
        // damage cannot be dealt until everyone has chosen, but this actor's split is
        // recorded.
        return Ok(DamagePlan::Record { assignments });
    }

    // CR 510.1b/c/d/510.2
    // Assign and deal in one pass, with no priority window between.
    let mut events = Vec::new();
    for (attacker, target) in &attackers {
        // CDA power (PTValue::CharacteristicDefining, layer 7a) resolves if the base
        // value carries a script; a script-less or erroring CDA is skipped and not
        // guessed, as in sba.rs's toughness check. effective_power also folds in
        // applicable ModifyPT and SetPT effects (layer 7c).
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
                // CR 509.1h/510.1c
                // Blocked but every blocker gone, no trample: deals no combat damage. It does
                // not fall back to the original target; only a never-blocked attacker or
                // trample (CR 702.19d) does.
            }
            None => {
                // CR 510.1b/702.19d
                // Never blocked, or blocked with no living blockers left and trample (as though
                // every blocker got lethal damage): all damage goes to the original target.
                events.push(GameEventKind::DamageDealt {
                    source: *attacker,
                    target: *target,
                    amount: power,
                    is_combat_damage: true,
                });
            }
            Some(blockers) if blockers.len() == 1 && !has_trample => {
                // CR 510.1c
                // One blocker, no trample: no choice, all damage there.
                events.push(GameEventKind::DamageDealt {
                    source: *attacker,
                    target: Target::Object(blockers[0]),
                    amount: power,
                    is_combat_damage: true,
                });
            }
            Some(_) => {
                // CR 510.1c/702.19b
                // 2+ blockers or a single trampling blocker: the validated split above, plus
                // whatever it left unassigned spilling to the original target. Validation
                // already confirmed every blocker got its lethal amount whenever the remainder
                // is nonzero.
                let mut assigned_to_blockers = 0u32;
                for (blocker, amount) in &assignment_map[attacker] {
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
    // CR 510.1d
    // A blocker deals damage to the one attacker it blocks, unless banding forced it
    // onto 2+ attackers (CR 702.22h/k), where the validated split above decides.
    // `handled_blockers` reduces `blocks`'s repeated blocker entries (one per
    // attacker it is forced onto) to one pass per blocker.
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
            for (attacker, amount) in &assignment_map[blocker] {
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

    // CR 510.2/601.2a
    // These are proposed events only. ReplacementCheck (CR 614-616) can rewrite or
    // prevent each one, so the real mutation happens in apply_damage_event.
    Ok(DamagePlan::Deal { events })
}

fn apply_damage(state: &mut GameState, plan: DamagePlan) -> Result<CommandOutcome, Internal> {
    let events = match plan {
        DamagePlan::AlreadyAssigned => Vec::new(),
        DamagePlan::Record { assignments } => {
            let combat =
                state.combat.as_mut().ok_or_else(|| Internal::new("assign damage: the validated combat is gone"))?;
            for (id, splits) in assignments {
                combat.pending_damage_splits.insert(id, splits);
            }
            Vec::new()
        }
        DamagePlan::Deal { events } => {
            // Mark this step done so a later call (auto-fire or a redundant explicit
            // submission) hits the no-op guard above and does not deal damage twice. This
            // also clears the consumed accumulator; both reset on the next transition into a
            // damage step (engine/turn.rs).
            let combat =
                state.combat.as_mut().ok_or_else(|| Internal::new("assign damage: the validated combat is gone"))?;
            combat.damage_assigned_this_step = true;
            combat.pending_damage_splits.clear();
            events
        }
    };
    Ok(CommandOutcome {
        events,
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    })
}
