//! `execute_command`'s `AssignCombatDamage` arm body, split out of the
//! original monolithic `commands.rs` - ordering, first/double strike,
//! lethal-damage math, and event building.

use std::collections::{HashMap, HashSet};

use crate::script::ScriptRuntime;
use crate::types::card::KeywordSet;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::turn::{Phase, Step};

use super::super::legality::{effective_keywords, effective_power, lethal_damage_needed, participates_in_damage_step};
use super::super::{CommandError, CommandOutcome, PriorityHandoff};

// `pub(crate)`, not `pub(in crate::commands)`: `phase5/automatic-combat-
// damage`'s `engine::turn::resolve_advance_turn_step_state` calls this
// directly (its own auto-fire attempt on entering the damage step),
// outside the normal `execute_command` dispatch this was originally
// scoped to.
pub(crate) fn execute_assign_combat_damage(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<CommandOutcome, CommandError> {
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

    // `phase5/automatic-combat-damage`: this step's damage was already
    // assigned+dealt - either `engine::turn`'s own auto-fire attempt beat
    // this call to it (the common case a redundant explicit
    // `AssignCombatDamage{assignments: vec![]}` submission hits, kept
    // legal so every pre-existing zero-split call site across the test
    // suite stays a harmless no-op rather than needing to be deleted), or
    // this is a genuine second submission for a step that's already done.
    // CR 510 has no "assign twice" case for a real player to hit either
    // way, so both collapse to the same no-op rather than distinguishing
    // them.
    if combat.damage_assigned_this_step {
        return Ok(CommandOutcome {
            events: Vec::new(),
            broke_pass_chain: true,
            priority_handoff: PriorityHandoff::Unchanged,
            requires_payment: None,
            battlefield_entries: Vec::new(),
        });
    }

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
    // player regardless of trample. `combat.effect_blocked` (CR
    // 509.1h/702.22i: became blocked via an effect, never had a real
    // blocker to begin with) joins the same set for the exact same
    // reason - zero blockers, living or otherwise, still means no
    // combat damage unless trample.
    let declared_blocked_attackers: HashSet<ObjectId> = combat
        .blocks
        .iter()
        .map(|(_, attacker)| *attacker)
        .chain(combat.effect_blocked.iter().copied())
        .collect();

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
            let subtype_match = x_characteristics.bands_with_other_subtypes.iter().any(|quality| {
                ids.iter().any(|y| {
                    y != x
                        && crate::layers::effective_characteristics(state, script_runtime, *y)
                            .subtypes
                            .contains(quality)
                })
            });
            // CR 702.22c/j/k's supertype form ("bands with other Legends"):
            // same pairwise check, against each candidate's `supertypes`
            // bitset instead of a `Vec<Subtype>`.
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
            // Structurally unreachable: DeclareAttackers already rejects
            // Target::Ability (an attacker never targets an ability).
            Target::Ability(_) => unreachable!("an attacker's own combat target is never an ability"),
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
    if actor != required_actor {
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

    // `phase5/automatic-combat-damage`: mark this step done so a later
    // call this same step (auto-fire or a redundant explicit submission)
    // hits the no-op guard above instead of dealing damage twice.
    state
        .combat
        .as_mut()
        .expect("checked Some above")
        .damage_assigned_this_step = true;

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
