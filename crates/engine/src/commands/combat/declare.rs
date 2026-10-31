//! The `DeclareAttackers`, `DeclareBands` and `DeclareBlockers` command bodies.
//! See `DESIGN.md` for the rest.

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
use super::super::{CommandOutcome, Internal, PriorityHandoff, Rejected, Validated};
use super::predicate::{attack_declaration_is_legal, block_declaration_is_legal};

/// A declaration that passed `validate_declare_attackers`. The flag is false for
/// an attacker with vigilance (CR 702.20a), decided with the layered keywords at
/// validation time.
#[derive(Debug)]
pub(crate) struct AttackersPlan {
    attackers: Vec<(ObjectId, Target, bool)>,
}

pub(crate) fn validate_declare_attackers(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> Result<Validated, Rejected> {
    // CR 508.1
    // A turn-based action of the active player, in the declare attackers step, once
    // per combat.
    if actor != state.active_player {
        return Err(Rejected::IllegalAction(
            "only the active player declares attackers".to_string(),
        ));
    }
    if state.phase != Phase::Combat || state.step != Step::DeclareAttackers {
        return Err(Rejected::IllegalAction(
            "attackers can only be declared during the declare attackers step".to_string(),
        ));
    }
    if state.combat.is_some() {
        return Err(Rejected::IllegalAction(
            "attackers have already been declared this combat".to_string(),
        ));
    }

    // CR 508.1
    // Validate every attacker before mutating anything, so an illegal batch taps
    // nobody.
    let mut seen_attackers = Vec::with_capacity(attackers.len());
    for (attacker, target) in &attackers {
        // CR 508.1
        if seen_attackers.contains(attacker) {
            return Err(Rejected::IllegalAction(
                "the same creature can't be declared as an attacker twice".to_string(),
            ));
        }
        seen_attackers.push(*attacker);
        attacker_can_attack(state, script_runtime, actor, *attacker)?;
        attack_target_is_legal(state, script_runtime, actor, target)?;
    }
    // CR 508.1d
    attack_declaration_is_legal(state, script_runtime, actor, &attackers)?;

    // CR 508.1f/702.20a
    // Tapping an attacker is not a cost; attacking causes it, except with Vigilance.
    // effective_keywords respects a granted Vigilance too.
    let attackers = attackers
        .into_iter()
        .map(|(attacker, target)| {
            let taps = !effective_keywords(state, script_runtime, attacker)
                .contains(KeywordSet::VIGILANCE);
            (attacker, target, taps)
        })
        .collect();
    Ok(Validated::DeclareAttackers(AttackersPlan { attackers }))
}

pub(crate) fn apply_declare_attackers(
    state: &mut GameState,
    plan: AttackersPlan,
) -> Result<CommandOutcome, Internal> {
    let mut events = Vec::with_capacity(plan.attackers.len() * 2);
    let mut declared = Vec::with_capacity(plan.attackers.len());
    for (attacker, target, taps) in plan.attackers {
        if taps {
            let object = state
                .object_mut(attacker)
                .ok_or_else(|| Internal::new("declare attackers: a validated attacker is gone"))?;
            object.tapped = true;
            events.push(GameEventKind::Tapped { object: attacker });
        }
        events.push(GameEventKind::AttackerDeclared { attacker, target });
        declared.push((attacker, target));
    }
    state.combat = Some(CombatState {
        attackers: declared,
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });

    Ok(CommandOutcome {
        events,
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    })
}

/// A band declaration that passed `validate_declare_bands`.
#[derive(Debug)]
pub(crate) struct BandsPlan {
    bands: Vec<Vec<ObjectId>>,
}

pub(crate) fn validate_declare_bands(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    bands: Vec<Vec<ObjectId>>,
) -> Result<Validated, Rejected> {
    // CR 508.1e/702.22c
    // The same window as DeclareAttackers: active player, declare attackers step,
    // once attackers exist to group.
    if actor != state.active_player {
        return Err(Rejected::IllegalAction(
            "only the active player declares bands".to_string(),
        ));
    }
    if state.phase != Phase::Combat || state.step != Step::DeclareAttackers {
        return Err(Rejected::IllegalAction(
            "bands can only be declared during the declare attackers step".to_string(),
        ));
    }
    let combat = state.combat.as_ref().ok_or_else(|| {
        Rejected::IllegalAction("no attackers have been declared this combat".to_string())
    })?;

    let mut seen: Vec<ObjectId> = Vec::new();
    for band in &bands {
        let mut target: Option<Target> = None;
        let mut banding_members = 0u32;
        let mut non_banding_members = 0u32;
        // CR 702.22c
        // Second band-formation path: members with "bands with other [quality]" for some
        // quality Q, plus members whose subtypes include Q. Collected per member and
        // checked once the band is known, since a member's quality can qualify a later
        // bandmate.
        let mut member_subtypes: Vec<Vec<crate::Subtype>> = Vec::new();
        let mut member_supertypes: Vec<SupertypeSet> = Vec::new();
        let mut member_bands_with_other: Vec<Vec<crate::Subtype>> = Vec::new();
        let mut member_bands_with_other_supertypes: Vec<SupertypeSet> = Vec::new();
        for member in band {
            let (_, member_target) = combat
                .attackers
                .iter()
                .find(|(id, _)| id == member)
                .ok_or_else(|| {
                    Rejected::IllegalAction(
                        "a band member isn't actually attacking this combat".to_string(),
                    )
                })?;
            if seen.contains(member) {
                return Err(Rejected::IllegalAction(
                    "a creature can be in at most one band".to_string(),
                ));
            }
            seen.push(*member);

            // CR 702.22d/802.3b
            // Every band member must attack the same player, planeswalker or battle.
            match target {
                None => target = Some(*member_target),
                Some(t) if t == *member_target => {}
                Some(_) => {
                    return Err(Rejected::IllegalAction(
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

        // CR 702.22c
        // First path: one or more banding attackers plus at most one without banding.
        let plain_banding_shape = banding_members >= 1 && non_banding_members <= 1;
        // CR 702.22c
        // Second path: at least one member has bands-with-other for some quality Q and
        // every other member's subtypes include Q. Tries each named quality in turn.
        let quality_shape = member_bands_with_other
            .iter()
            .enumerate()
            .any(|(i, qualities)| {
                qualities.iter().any(|quality| {
                    member_subtypes
                        .iter()
                        .enumerate()
                        .all(|(j, subtypes)| j == i || subtypes.contains(quality))
                })
            });
        // CR 702.22c/m
        // The supertype form: the same any-one-quality shape, against each member's
        // `supertypes` bitset.
        let supertype_quality_shape =
            member_bands_with_other_supertypes
                .iter()
                .enumerate()
                .any(|(i, qualities)| {
                    qualities.iter().any(|quality| {
                        member_supertypes
                            .iter()
                            .enumerate()
                            .all(|(j, supertypes)| j == i || supertypes.contains(quality))
                    })
                });
        if !plain_banding_shape && !quality_shape && !supertype_quality_shape {
            return Err(Rejected::IllegalAction(
                "a band needs either at least one attacking creature with banding (and \
                 at most one without), or a creature with \"bands with other [quality]\" \
                 whose bandmates all share that quality"
                    .to_string(),
            ));
        }
    }

    Ok(Validated::DeclareBands(BandsPlan { bands }))
}

pub(crate) fn apply_declare_bands(
    state: &mut GameState,
    plan: BandsPlan,
) -> Result<CommandOutcome, Internal> {
    let combat = state
        .combat
        .as_mut()
        .ok_or_else(|| Internal::new("declare bands: the validated combat is gone"))?;
    combat.bands = plan.bands;

    Ok(CommandOutcome {
        events: Vec::new(),
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    })
}

/// A blocker declaration that passed `validate_declare_blockers`, with the CR
/// 702.22h banding blocks already added.
#[derive(Debug)]
pub(crate) struct BlockersPlan {
    actor: PlayerId,
    blocks: Vec<(ObjectId, ObjectId)>,
}

pub(crate) fn validate_declare_blockers(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    mut blocks: Vec<(ObjectId, ObjectId)>,
) -> Result<Validated, Rejected> {
    if state.phase != Phase::Combat || state.step != Step::DeclareBlockers {
        return Err(Rejected::IllegalAction(
            "blockers can only be declared during the declare blockers step".to_string(),
        ));
    }
    let combat = state.combat.as_ref().ok_or_else(|| {
        Rejected::IllegalAction("no attackers have been declared this combat".to_string())
    })?;
    if combat.declared_defenders.contains(&actor) {
        return Err(Rejected::IllegalAction(
            "this player has already declared blockers this combat".to_string(),
        ));
    }
    // CR 509.1/508.5a/506.4d
    // A turn-based action, one per defending player. Multiplayer can have several,
    // each determined by which attacker is aimed at them or at a permanent they
    // control. `command.actor` is legal here only if some attacker is aimed at them,
    // resolving "defending player" against a targeted permanent's current
    // controller.
    if !is_defending_player(state, script_runtime, combat, actor) {
        return Err(Rejected::IllegalAction(
            "this player is not a defending player this combat".to_string(),
        ));
    }

    // CR 509.1a
    // A blocker can appear only once across the declaration.
    let mut seen_blockers = Vec::with_capacity(blocks.len());
    for (blocker, attacker) in &blocks {
        if seen_blockers.contains(blocker) {
            return Err(Rejected::IllegalAction(
                "the same blocker can't be assigned to block twice".to_string(),
            ));
        }
        seen_blockers.push(*blocker);

        block_is_legal(state, script_runtime, actor, combat, *blocker, *attacker)?;
    }

    // CR 509.1b/702.111b: menace is about how many blockers ended up on one
    // attacker, not any single pairing, so it checks the whole batch. The
    // entries share `blockers_needed` with this check.
    block_declaration_is_legal(state, script_runtime, &blocks)?;

    // CR 702.22h
    // Banding: once a band member is directly blocked in this submission, every
    // other member of its band becomes blocked by that creature too, bypassing every
    // per-pair check above and the whole-batch menace check (the CR 702.22h
    // swampwalker example blocks despite a Swamp). Scoped to direct blocks from this
    // submission because CR 702.22d makes every band member share one defending
    // player, so no earlier submission holds a same-band pairing. A member gets a
    // propagated block from every band-mate's direct block even if it is already
    // blocked otherwise, because CR 702.22h has no carve-out; only an exact
    // duplicate (blocker, member) pair is skipped.
    let mut propagated: Vec<(ObjectId, ObjectId)> = Vec::new();
    for (blocker, attacker) in &blocks {
        let Some(band) = combat.bands.iter().find(|band| band.contains(attacker)) else {
            continue;
        };
        for member in band {
            if member == attacker || !combat.attackers.iter().any(|(id, _)| id == member) {
                continue;
            }
            let already = blocks
                .iter()
                .chain(propagated.iter())
                .any(|(b, a)| b == blocker && a == member);
            if !already {
                propagated.push((*blocker, *member));
            }
        }
    }
    blocks.extend(propagated);
    Ok(Validated::DeclareBlockers(BlockersPlan { actor, blocks }))
}

pub(crate) fn apply_declare_blockers(
    state: &mut GameState,
    plan: BlockersPlan,
) -> Result<CommandOutcome, Internal> {
    let BlockersPlan { actor, blocks } = plan;
    let events = blocks
        .iter()
        .map(|(blocker, attacker)| GameEventKind::BlockerDeclared {
            blocker: *blocker,
            attacker: *attacker,
        })
        .collect();
    let combat = state
        .combat
        .as_mut()
        .ok_or_else(|| Internal::new("declare blockers: the validated combat is gone"))?;
    combat.blocks.extend(blocks);
    combat.declared_defenders.push(actor);

    Ok(CommandOutcome {
        events,
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        requires_payment: None,
        battlefield_entries: Vec::new(),
    })
}

/// CR 509.1h/702.22i: makes `target` become blocked by an effect
/// (`AbilityEffect::BecomeBlocked`, not a declared blocker), recorded in
/// `CombatState.effect_blocked` because there is no real blocker to pair it with.
/// A no-op if `target` is not an attacker or there is no combat (see
/// `AbilityEffect::BecomeBlocked` for why target legality cannot catch this
/// earlier). CR 603.2e: "becomes blocked" fires only on a real state change, so
/// an already-blocked attacker is left untouched.
///
/// CR 702.22i: unlike CR 702.22h's propagation of the same blocker, this
/// propagates only the blocked state: every other living, still-attacking member
/// of `target`'s band becomes blocked by no one, like `target`.
pub(crate) fn mark_effect_blocked(state: &mut GameState, target: ObjectId) {
    let Some(combat) = state.combat.as_mut() else {
        return;
    };
    if !combat.attackers.iter().any(|(id, _)| *id == target) {
        return;
    }
    mark_one_effect_blocked(combat, target);
    let Some(band) = combat
        .bands
        .iter()
        .find(|band| band.contains(&target))
        .cloned()
    else {
        return;
    };
    for member in band {
        if member == target || !combat.attackers.iter().any(|(id, _)| *id == member) {
            continue;
        }
        mark_one_effect_blocked(combat, member);
    }
}

fn mark_one_effect_blocked(combat: &mut CombatState, attacker: ObjectId) {
    let already = combat.blocks.iter().any(|(_, a)| *a == attacker)
        || combat.effect_blocked.contains(&attacker);
    if !already {
        combat.effect_blocked.push(attacker);
    }
}

/// CR 508.1a/302.6/702.3b: whether `attacker` may be declared as an attacker by `actor`.
/// Shared by `DeclareAttackers` and the attacker entries a Decision lists.
pub(crate) fn attacker_can_attack(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    attacker: ObjectId,
) -> Result<(), Rejected> {
    let object = state.object(attacker).ok_or(Rejected::InvalidTarget)?;
    // CR 508.1a: only a creature on the battlefield can be declared as an
    // attacker.
    if object.zone != ZoneKind::Battlefield {
        return Err(Rejected::IllegalAction(
            "a chosen attacker is not on the battlefield".to_string(),
        ));
    }
    // CR 508.1a: attacking requires *current* control, per
    // layers::effective_controller.
    if crate::layers::effective_controller(state, script_runtime, attacker) != actor {
        return Err(Rejected::IllegalAction(
            "actor does not control a chosen attacker".to_string(),
        ));
    }
    if !object.definition.card_types.contains(CardTypeSet::CREATURE)
        || object.definition.card_types.contains(CardTypeSet::BATTLE)
    {
        return Err(Rejected::IllegalAction(
            "a chosen attacker is not a creature, or is also a battle".to_string(),
        ));
    }
    if object.tapped {
        return Err(Rejected::IllegalAction(
            "a chosen attacker is already tapped".to_string(),
        ));
    }
    // CR 508.1a/302.6/702.10b: has_summoning_sickness itself
    // grants Haste's bypass.
    if has_summoning_sickness(state, script_runtime, object) {
        return Err(Rejected::IllegalAction(
            "a chosen attacker has summoning sickness".to_string(),
        ));
    }
    // CR 302.6b/702.3b
    // A creature with defender cannot attack. effective_keywords respects a granted
    // or removed Defender. An applicable IgnoreKeyword(DEFENDER) grant (Assault
    // Formation) leaves Defender in place but exempts the creature from the
    // restriction (see that primitive).
    if effective_keywords(state, script_runtime, object.object_id).contains(KeywordSet::DEFENDER)
        && !ignored_keywords(state, script_runtime, object.object_id).contains(KeywordSet::DEFENDER)
    {
        return Err(Rejected::IllegalAction(
            "a chosen attacker has defender and can't attack".to_string(),
        ));
    }
    Ok(())
}

/// CR 506.4/508.1b: whether `target` is something `actor` may attack.
pub(crate) fn attack_target_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    target: &Target,
) -> Result<(), Rejected> {
    match target {
        Target::Player(pid) => {
            if *pid == actor || !state.turn_order.contains(pid) {
                return Err(Rejected::IllegalAction(
                    "an attacker's target must be an opponent".to_string(),
                ));
            }
        }
        Target::Object(target_id) => {
            let target_object = state.object(*target_id).ok_or(Rejected::InvalidTarget)?;
            // CR 508.1b: an attacked planeswalker or battle is a permanent.
            if target_object.zone != ZoneKind::Battlefield {
                return Err(Rejected::IllegalAction(
                    "an attacker's target permanent must be on the battlefield".to_string(),
                ));
            }
            // CR 508.1b: the target permanent's *current*
            // controller must be an opponent.
            if crate::layers::effective_controller(state, script_runtime, *target_id) == actor {
                return Err(Rejected::IllegalAction(
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
                return Err(Rejected::IllegalAction(
                    "an attacker's target permanent must be a planeswalker or \
                     battle"
                        .to_string(),
                ));
            }
        }
        // CR 506.4/508.1a
        // An attacker is declared against a player, planeswalker or battle, never an
        // ability.
        Target::Ability(_) => return Err(Rejected::InvalidTarget),
    }
    Ok(())
}

/// Every per-pair rule for `blocker` blocking `attacker` (CR 509.1a, 702.9b, 702.13b, 702.14c, 702.16d,
/// 702.28b, 702.31b, 702.36b). The whole-declaration rules (menace, banding) are in
/// `predicate` and `validate_declare_blockers`. Shared with the blocker entries a Decision lists.
pub(crate) fn block_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    combat: &CombatState,
    blocker: ObjectId,
    attacker: ObjectId,
) -> Result<(), Rejected> {
    let blocker_object = state.object(blocker).ok_or(Rejected::InvalidTarget)?;
    // CR 509.1a: only a creature on the battlefield can be declared as a
    // blocker.
    if blocker_object.zone != ZoneKind::Battlefield {
        return Err(Rejected::IllegalAction(
            "a chosen blocker is not on the battlefield".to_string(),
        ));
    }
    // CR 509.1a: blocking requires *current* control.
    if crate::layers::effective_controller(state, script_runtime, blocker) != actor {
        return Err(Rejected::IllegalAction(
            "actor does not control a chosen blocker".to_string(),
        ));
    }
    if !blocker_object
        .definition
        .card_types
        .contains(CardTypeSet::CREATURE)
        || blocker_object
            .definition
            .card_types
            .contains(CardTypeSet::BATTLE)
    {
        return Err(Rejected::IllegalAction(
            "a chosen blocker is not a creature, or is also a battle".to_string(),
        ));
    }
    if blocker_object.tapped {
        return Err(Rejected::IllegalAction(
            "a chosen blocker is already tapped".to_string(),
        ));
    }

    let (_, attacking) = combat
        .attackers
        .iter()
        .find(|(id, _)| *id == attacker)
        .ok_or_else(|| {
            Rejected::IllegalAction(
                "a chosen attacker isn't actually attacking this combat".to_string(),
            )
        })?;
    let attacking_this_defender = match attacking {
        Target::Player(pid) => *pid == actor,
        Target::Object(target_id) => state.object(*target_id).is_some_and(|_| {
            crate::layers::effective_controller(state, script_runtime, *target_id) == actor
        }),
        // Structurally unreachable: DeclareAttackers already rejects
        // Target::Ability (an attacker never targets an ability).
        Target::Ability(_) => false,
    };
    if !attacking_this_defender {
        return Err(Rejected::IllegalAction(
            "a chosen attacker isn't attacking this defending player, their \
             planeswalker, or their battle"
                .to_string(),
        ));
    }

    // CR 702.9b
    // An attacker with flying can be blocked only by a creature with flying or
    // reach. Reads effective_keywords (CR 613 layer 6), so a granted or removed
    // Flying or Reach changes legality.
    let attacker_object = state.object(attacker).ok_or(Rejected::InvalidTarget)?;
    let attacker_keywords = effective_keywords(state, script_runtime, attacker_object.object_id);
    let blocker_keywords = effective_keywords(state, script_runtime, blocker_object.object_id);
    if attacker_keywords.contains(KeywordSet::FLYING)
        && !blocker_keywords.intersects(KeywordSet::FLYING | KeywordSet::REACH)
    {
        return Err(Rejected::IllegalAction(
            "an attacker with flying can only be blocked by a creature with \
             flying or reach"
                .to_string(),
        ));
    }

    // CR 702.16d's protection check below needs both objects'
    // effective_characteristics, so they are computed once here and Fear and
    // Intimidate reuse them.
    let attacker_characteristics =
        crate::layers::effective_characteristics(state, script_runtime, attacker_object.object_id);
    let blocker_characteristics =
        crate::layers::effective_characteristics(state, script_runtime, blocker_object.object_id);

    // CR 702.36b/702.13b
    // Fear: only artifact or black creatures can block. Intimidate: only artifact
    // creatures or creatures sharing a color with the attacker.
    if attacker_keywords.contains(KeywordSet::FEAR)
        && !(blocker_characteristics
            .card_types
            .contains(CardTypeSet::ARTIFACT)
            || blocker_characteristics.colors.contains(ColorSet::BLACK))
    {
        return Err(Rejected::IllegalAction(
            "an attacker with fear can only be blocked by an artifact and/or \
             black creature"
                .to_string(),
        ));
    }
    if attacker_keywords.contains(KeywordSet::INTIMIDATE)
        && !(blocker_characteristics
            .card_types
            .contains(CardTypeSet::ARTIFACT)
            || blocker_characteristics
                .colors
                .intersects(attacker_characteristics.colors))
    {
        return Err(Rejected::IllegalAction(
            "an attacker with intimidate can only be blocked by an artifact \
             creature and/or a creature sharing a color with it"
                .to_string(),
        ));
    }

    // CR 702.28b
    // Shadow is symmetric: a shadow creature blocks and is blocked only by shadow
    // creatures.
    if attacker_keywords.contains(KeywordSet::SHADOW)
        != blocker_keywords.contains(KeywordSet::SHADOW)
    {
        return Err(Rejected::IllegalAction(
            "shadow creatures can only block or be blocked by other shadow \
             creatures"
                .to_string(),
        ));
    }

    // CR 702.31b
    // Horsemanship: cannot be blocked by a creature without it. A horsemanship
    // creature may block anything.
    if attacker_keywords.contains(KeywordSet::HORSEMANSHIP)
        && !blocker_keywords.contains(KeywordSet::HORSEMANSHIP)
    {
        return Err(Rejected::IllegalAction(
            "an attacker with horsemanship can only be blocked by a creature \
             with horsemanship"
                .to_string(),
        ));
    }

    // CR 702.14c
    // Landwalk makes the attacker unblockable while the defending player controls a
    // land with the specified subtype. Unlike Fear, Intimidate, Shadow and
    // Horsemanship above, this checks the defending player's battlefield and not
    // the blocker (see EffectiveCharacteristics::landwalk_subtypes).
    if !attacker_characteristics.landwalk_subtypes.is_empty()
        || !attacker_characteristics.landwalk_qualities.is_empty()
    {
        let defender_controls_matching_land = state.objects.values().any(|land| {
            if land.zone != ZoneKind::Battlefield
                || crate::layers::effective_controller(state, script_runtime, land.object_id) != actor
            {
                return false;
            }
            let land_characteristics = crate::layers::effective_characteristics(state, script_runtime, land.object_id);
            land_characteristics.subtypes.iter().any(|s| attacker_characteristics.landwalk_subtypes.contains(s))
                // CR 702.14a
                // The combination forms: the land must satisfy every field the matched
                // `LandwalkQuality` specifies, besides being a land.
                || attacker_characteristics.landwalk_qualities.iter().any(|quality| {
                    land_characteristics.card_types.contains(CardTypeSet::LAND)
                        && quality.subtypes.iter().all(|s| land_characteristics.subtypes.contains(s))
                        && quality.card_types.is_none_or(|ct| land_characteristics.card_types.intersects(ct))
                        && quality.supertypes.is_none_or(|st| land_characteristics.supertypes.contains(st))
                        && quality.excluded_supertypes.is_none_or(|st| !land_characteristics.supertypes.intersects(st))
                })
        });
        if defender_controls_matching_land {
            return Err(Rejected::IllegalAction(
                "an attacker with landwalk can't be blocked while the defending \
                 player controls a matching land"
                    .to_string(),
            ));
        }
    }

    // CR 702.16d
    // An attacker with protection from a quality (color 702.16e, card type, subtype,
    // name 702.16a, everything 702.16j, or a specific player 702.16k) cannot be
    // blocked by a creature matching it. Reuses the characteristics computed above,
    // so granted colors, types, subtypes, names and protections count. The player
    // quality compares raw GameObject.controller fields relative to the attacker's
    // controller, as target_has_protection_from does.
    if attacker_characteristics.protection_everything
        || attacker_characteristics
            .protection_colors
            .intersects(blocker_characteristics.colors)
        || attacker_characteristics
            .protection_card_types
            .intersects(blocker_characteristics.card_types)
        || attacker_characteristics
            .protection_subtypes
            .iter()
            .any(|s| blocker_characteristics.subtypes.contains(s))
        || attacker_characteristics
            .protection_names
            .contains(&blocker_characteristics.name)
        || attacker_characteristics
            .protection_players
            .iter()
            .any(|who| match who {
                FilterController::You => blocker_object.controller == attacker_object.controller,
                FilterController::Opponent => {
                    blocker_object.controller != attacker_object.controller
                }
                FilterController::Any => false,
            })
    {
        return Err(Rejected::IllegalAction(
            "an attacker with protection from a quality can't be blocked by a \
             creature matching that quality"
                .to_string(),
        ));
    }
    Ok(())
}

/// CR 508.5a/506.4d: `player` is a defending player when some attacker is aimed at them, or at a
/// planeswalker or battle they currently control.
pub(crate) fn is_defending_player(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    combat: &CombatState,
    player: PlayerId,
) -> bool {
    combat.attackers.iter().any(|(_, target)| match target {
        Target::Player(pid) => *pid == player,
        Target::Object(target_id) => state.object(*target_id).is_some_and(|_| {
            crate::layers::effective_controller(state, script_runtime, *target_id) == player
        }),
        // Structurally unreachable: DeclareAttackers already rejects
        // Target::Ability (an attacker never targets an ability).
        Target::Ability(_) => false,
    })
}
