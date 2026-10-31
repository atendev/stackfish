//! Structural checks of a priority, attack, block or damage Answer against the
//! entries the Decision listed, run before the Answer reaches the engine. See
//! `DESIGN.md` for the rest.

// TODO: which mode pairs combine and how many blockers a menace attacker got are only checked when the action runs, so a bad Answer there fails during `step()` and not as a rejection

use std::collections::HashSet;

use crate::types::command::PlayerActionKind;
use crate::types::ids::{ObjectId, Target};

use super::{
    ActivateEntry, AttackerEntry, BlockerEntry, CastEntry, DamageSplitEntry, Entry, EntryTargets,
    ModeRange, Rejection, RejectionCode, TargetSlot,
};

fn invalid(message: impl Into<String>) -> Rejection {
    Rejection::new(RejectionCode::InvalidChoice, message)
}

pub(super) fn check_action(entries: &[Entry], kind: &PlayerActionKind) -> Result<(), Rejection> {
    match kind {
        PlayerActionKind::PassPriority => Ok(()),
        PlayerActionKind::PlayLand { card } => {
            let listed = entries
                .iter()
                .any(|entry| matches!(entry, Entry::PlayLand { card: c } if c == card));
            listed
                .then_some(())
                .ok_or_else(|| invalid("that land cannot be played now"))
        }
        PlayerActionKind::CastSpell {
            card,
            modes,
            targets,
            x_value,
            alternative_cost,
            pay_buyback,
        } => {
            let entry = entries
                .iter()
                .find_map(|entry| match entry {
                    Entry::Cast(cast) if cast.card == *card => Some(cast),
                    _ => None,
                })
                .ok_or_else(|| invalid("that card cannot be cast now"))?;
            check_cast(
                entry,
                modes,
                targets,
                *x_value,
                *alternative_cost,
                *pay_buyback,
            )
        }
        PlayerActionKind::ActivateAbility {
            source,
            ability_index,
            targets,
            chosen_colors,
            x_value,
        } => {
            let entry = entries
                .iter()
                .find_map(|entry| match entry {
                    Entry::Activate(activate)
                        if activate.source == *source
                            && activate.ability_index == *ability_index =>
                    {
                        Some(activate)
                    }
                    _ => None,
                })
                .ok_or_else(|| invalid("that ability cannot be activated now"))?;
            check_activate(entry, targets, chosen_colors, *x_value)
        }
        _ => Err(invalid("that action is not offered at a priority Decision")),
    }
}

fn check_cast(
    entry: &CastEntry,
    modes: &[u32],
    targets: &[Target],
    x_value: Option<u32>,
    alternative_cost: Option<usize>,
    pay_buyback: bool,
) -> Result<(), Rejection> {
    match alternative_cost {
        None if !entry.normal => {
            return Err(invalid(
                "that card can only be cast with an alternative cost",
            ));
        }
        Some(index) if !entry.alternative_costs.contains(&index) => {
            return Err(invalid("that alternative cost is not available"));
        }
        _ => {}
    }
    if pay_buyback && !entry.buyback {
        return Err(invalid("that card has no buyback cost"));
    }
    // The engine leaves X unchecked when an alternative cost replaces the
    // mana cost.
    if alternative_cost.is_none() && x_value.is_some() != entry.x {
        return Err(invalid(if entry.x {
            "this spell's cost includes {X}, so a value must be announced"
        } else {
            "an X value was announced but this spell's cost has no {X}"
        }));
    }
    match (&entry.modes, &entry.targets) {
        (None, EntryTargets::Fixed(slots)) => {
            if !modes.is_empty() {
                return Err(invalid("this card is not modal"));
            }
            check_targets(slots.iter().collect(), targets)
        }
        (Some(range), EntryTargets::PerMode(per_mode)) => {
            check_modes(range, modes)?;
            let slots = modes
                .iter()
                .flat_map(|&mode| per_mode[mode as usize].iter())
                .collect();
            check_targets(slots, targets)
        }
        _ => unreachable!("an entry has mode targets exactly when it has a mode range"),
    }
}

fn check_modes(range: &ModeRange, modes: &[u32]) -> Result<(), Rejection> {
    if modes.iter().any(|&mode| mode as usize >= range.mode_count) {
        return Err(invalid("no such mode on this card"));
    }
    if modes.len() >= 2 && modes.iter().any(|mode| range.solo_only.contains(mode)) {
        return Err(invalid(
            "a mode that cannot be combined was chosen beside another mode",
        ));
    }
    if !range.repeatable && modes.iter().collect::<HashSet<_>>().len() != modes.len() {
        return Err(invalid("the same mode cannot be chosen twice"));
    }
    match &range.pawprint_weights {
        Some(weights) => {
            let total: u32 = modes.iter().map(|&mode| weights[mode as usize]).sum();
            if total > range.max {
                return Err(invalid(format!(
                    "the chosen modes weigh {total}, over the budget of {}",
                    range.max
                )));
            }
        }
        None => {
            let count = modes.len() as u32;
            if count < range.min || count > range.max {
                return Err(invalid(format!(
                    "this spell takes between {} and {} modes, got {count}",
                    range.min, range.max
                )));
            }
        }
    }
    Ok(())
}

fn check_activate(
    entry: &ActivateEntry,
    targets: &[Target],
    chosen_colors: &[crate::types::mana::Color],
    x_value: Option<u32>,
) -> Result<(), Rejection> {
    if x_value.is_some() != entry.x {
        return Err(invalid(if entry.x {
            "this ability's cost includes {X}, so a value must be announced"
        } else {
            "an X value was announced but this ability's cost has no {X}"
        }));
    }
    match &entry.color_choice {
        None if !chosen_colors.is_empty() => {
            return Err(invalid("this ability has no mana color to choose"));
        }
        Some(choice)
            if chosen_colors.len() != choice.count as usize
                || chosen_colors.iter().any(|c| !choice.colors.contains(c)) =>
        {
            return Err(invalid(format!(
                "choose {} color(s) from the listed colors",
                choice.count
            )));
        }
        _ => {}
    }
    check_targets(entry.targets.iter().collect(), targets)
}

/// Every target is a candidate of some slot, and the count fits the slots.
fn check_targets(slots: Vec<&TargetSlot>, targets: &[Target]) -> Result<(), Rejection> {
    let min: u32 = slots.iter().map(|slot| slot.min).sum();
    let max: Option<u32> = slots.iter().map(|slot| slot.max).sum();
    let count = targets.len() as u32;
    if count < min || max.is_some_and(|max| count > max) {
        return Err(invalid(format!(
            "the number of targets ({count}) does not fit the listed target slots"
        )));
    }
    if targets
        .iter()
        .any(|target| !slots.iter().any(|slot| slot.candidates.contains(target)))
    {
        return Err(invalid("a target is not a listed candidate"));
    }
    Ok(())
}

pub(super) fn check_attackers(
    entries: &[AttackerEntry],
    declared: &[(ObjectId, Target)],
) -> Result<(), Rejection> {
    let mut seen = HashSet::new();
    for (attacker, defender) in declared {
        let entry = entries
            .iter()
            .find(|entry| entry.attacker == *attacker)
            .ok_or_else(|| invalid("a declared attacker is not listed"))?;
        if !entry.defenders.contains(defender) {
            return Err(invalid("a declared attacker cannot attack that defender"));
        }
        if !seen.insert(*attacker) {
            return Err(invalid("a creature cannot attack twice"));
        }
    }
    // CR 508.1d
    if entries
        .iter()
        .any(|entry| entry.required && !seen.contains(&entry.attacker))
    {
        return Err(invalid("a creature that must attack is not declared"));
    }
    Ok(())
}

pub(super) fn check_blockers(
    entries: &[BlockerEntry],
    declared: &[(ObjectId, ObjectId)],
) -> Result<(), Rejection> {
    let mut seen = HashSet::new();
    for (blocker, attacker) in declared {
        let entry = entries
            .iter()
            .find(|entry| entry.blocker == *blocker)
            .ok_or_else(|| invalid("a declared blocker is not listed"))?;
        if !entry.attackers.contains(attacker) {
            return Err(invalid("a declared blocker cannot block that attacker"));
        }
        if !seen.insert(*blocker) {
            return Err(invalid("a creature cannot block twice"));
        }
    }
    Ok(())
}

pub(super) fn check_splits(
    entries: &[DamageSplitEntry],
    assignments: &[(ObjectId, Vec<(ObjectId, u32)>)],
) -> Result<(), Rejection> {
    let listed: HashSet<ObjectId> = entries.iter().map(|entry| entry.source).collect();
    let given: HashSet<ObjectId> = assignments.iter().map(|(source, _)| *source).collect();
    if listed != given || assignments.len() != entries.len() {
        return Err(invalid(
            "the reply must split the damage of exactly the listed creatures",
        ));
    }
    for (source, split) in assignments {
        let entry = entries
            .iter()
            .find(|entry| entry.source == *source)
            .expect("sources match");
        let recipients: HashSet<ObjectId> = split.iter().map(|(recipient, _)| *recipient).collect();
        if recipients.len() != split.len()
            || recipients != entry.recipients.iter().copied().collect()
        {
            return Err(invalid(
                "a split must give every recipient an amount, possibly zero, once",
            ));
        }
        let total: u32 = split.iter().map(|(_, amount)| *amount).sum();
        let fits = if entry.spill.is_some() {
            total <= entry.power
        } else {
            total == entry.power
        };
        if !fits {
            return Err(invalid(format!(
                "a split of {total} does not fit a power of {}",
                entry.power
            )));
        }
    }
    Ok(())
}
