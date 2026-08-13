//! `execute_command`'s `DeclareAttackers`/`DeclareBands`/`DeclareBlockers`
//! arm bodies, split out of the original monolithic `commands.rs`.

use std::collections::HashMap;

use crate::script::ScriptRuntime;
use crate::types::card::CardTypeSet;
use crate::types::card::KeywordSet;
use crate::types::card::SupertypeSet;
use crate::types::combat::CombatState;
use crate::types::cost::FilterController;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::mana::ColorSet;
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

use super::super::legality::{effective_keywords, has_summoning_sickness, ignored_keywords};
use super::super::{CommandError, CommandOutcome, PriorityHandoff};

pub(in crate::commands) fn execute_declare_attackers(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> Result<CommandOutcome, CommandError> {
    // CR 508.1: a turn-based action of the active player, only
    // during the declare attackers step, and only once per combat.
    if actor != state.active_player {
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
        if crate::layers::effective_controller(state, script_runtime, *attacker) != actor {
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
        // effective_keywords so a granted/removed Defender is respected, not
        // just the printed keyword. A creature with an applicable
        // IgnoreKeyword(DEFENDER) grant (e.g. Assault Formation's
        // "Walls you control can attack as though they didn't have
        // defender") still has Defender - it's just exempt from the
        // restriction it imposes, see that primitive's own doc comment.
        if effective_keywords(state, script_runtime, object.object_id).contains(KeywordSet::DEFENDER)
            && !ignored_keywords(state, script_runtime, object.object_id).contains(KeywordSet::DEFENDER)
        {
            return Err(CommandError::IllegalAction(
                "a chosen attacker has defender and can't attack".to_string(),
            ));
        }
        match target {
            Target::Player(pid) => {
                if *pid == actor || !state.turn_order.contains(pid) {
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
                    == actor
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
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
    });

    return Ok(CommandOutcome {
        events,
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    });
}

pub(in crate::commands) fn execute_declare_bands(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    bands: Vec<Vec<ObjectId>>,
) -> Result<CommandOutcome, CommandError> {
    // CR 508.1e/702.22c: same window as DeclareAttackers itself -
    // active player, only during the declare attackers step, only
    // once attackers actually exist to group.
    if actor != state.active_player {
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
        let mut member_supertypes: Vec<SupertypeSet> = Vec::new();
        let mut member_bands_with_other: Vec<Vec<crate::Subtype>> = Vec::new();
        let mut member_bands_with_other_supertypes: Vec<SupertypeSet> = Vec::new();
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
            member_supertypes.push(characteristics.supertypes);
            member_bands_with_other.push(characteristics.bands_with_other_subtypes);
            member_bands_with_other_supertypes.push(characteristics.bands_with_other_supertypes);
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
        // CR 702.22c/m's supertype form ("bands with other Legends," i.e.
        // Legendary): same "any one named quality makes every other member
        // match" shape as the subtype form above, checked against each
        // member's own `supertypes` bitset instead of a `Vec<Subtype>`.
        let supertype_quality_shape =
            member_bands_with_other_supertypes.iter().enumerate().any(|(i, qualities)| {
                qualities.iter().any(|quality| {
                    member_supertypes.iter().enumerate().all(|(j, supertypes)| j == i || supertypes.contains(quality))
                })
            });
        if !plain_banding_shape && !quality_shape && !supertype_quality_shape {
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

pub(in crate::commands) fn execute_declare_blockers(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    mut blocks: Vec<(ObjectId, ObjectId)>,
) -> Result<CommandOutcome, CommandError> {
    if state.phase != Phase::Combat || state.step != Step::DeclareBlockers {
        return Err(CommandError::IllegalAction(
            "blockers can only be declared during the declare blockers step".to_string(),
        ));
    }
    let combat = state.combat.as_ref().ok_or_else(|| {
        CommandError::IllegalAction("no attackers have been declared this combat".to_string())
    })?;
    if combat.declared_defenders.contains(&actor) {
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
        Target::Player(pid) => *pid == actor,
        Target::Object(target_id) => state.object(*target_id).is_some_and(|_| {
            crate::layers::effective_controller(state, script_runtime, *target_id) == actor
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
        if crate::layers::effective_controller(state, script_runtime, *blocker) != actor {
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
            Target::Player(pid) => *pid == actor,
            Target::Object(target_id) => state.object(*target_id).is_some_and(|_| {
                crate::layers::effective_controller(state, script_runtime, *target_id) == actor
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
                    || crate::layers::effective_controller(state, script_runtime, land.object_id) != actor
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
    combat.declared_defenders.push(actor);

    return Ok(CommandOutcome {
        events,
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    });
}

/// CR 509.1h/702.22i: makes `target` become blocked via an effect (`Ability
/// Effect::BecomeBlocked` — not a declared blocker), by recording it in
/// `CombatState.effect_blocked` rather than `blocks` (there's no real
/// blocker to pair it with). No-op if `target` isn't currently an attacker
/// at all (see `AbilityEffect::BecomeBlocked`'s own doc comment for why
/// this can't be caught earlier, at target-legality time) or if there's no
/// combat happening. CR 603.2e: "becomes" blocked only fires on a real
/// state change, so an attacker already blocked — whether by a declared
/// blocker or an earlier effect — is left untouched, not re-added.
///
/// CR 702.22i: unlike CR 702.22h's declared-blocker propagation (which
/// gives every band member the *same* blocker), this only propagates the
/// blocked *state* — every other living, still-attacking member of
/// `target`'s own band independently becomes blocked by no one, exactly
/// like `target` itself.
pub(crate) fn mark_effect_blocked(state: &mut GameState, target: ObjectId) {
    let Some(combat) = state.combat.as_mut() else { return };
    if !combat.attackers.iter().any(|(id, _)| *id == target) {
        return;
    }
    mark_one_effect_blocked(combat, target);
    let Some(band) = combat.bands.iter().find(|band| band.contains(&target)).cloned() else { return };
    for member in band {
        if member == target || !combat.attackers.iter().any(|(id, _)| *id == member) {
            continue;
        }
        mark_one_effect_blocked(combat, member);
    }
}

fn mark_one_effect_blocked(combat: &mut CombatState, attacker: ObjectId) {
    let already = combat.blocks.iter().any(|(_, a)| *a == attacker) || combat.effect_blocked.contains(&attacker);
    if !already {
        combat.effect_blocked.push(attacker);
    }
}
