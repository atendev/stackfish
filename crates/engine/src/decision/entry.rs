//! The options a Decision lists: priority entries, attackers and blockers with
//! their legal counterparts, and damage splits. See `DESIGN.md` for the rest.

use crate::types::ids::{ObjectId, Target};
use crate::types::mana::Color;

/// One target word of a spell or ability. `candidates` is explicit and in a
/// stable order: objects by id, then players, then abilities on the stack.
///
/// The entry is listed before the caster announces X (CR 601.2b comes before
/// 601.2c). A word whose `mana_value` filter mentions X is `x_dependent`, and
/// lists every candidate legal for some X. The engine checks the pair of
/// announced X and targets when the Answer arrives.
///
/// TODO: "legal for some X" ignores whether the caster can pay that X (CR 107.3a)
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetSlot {
    pub candidates: Vec<Target>,
    /// Zero for an "up to" word.
    pub min: u32,
    /// `None` for an "N or more" word.
    pub max: Option<u32>,
    /// The candidates depend on the announced X, so they are a superset.
    pub x_dependent: bool,
}

/// The target slots of a spell. A modal spell lists them per mode, because the
/// chosen modes decide which slots apply.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EntryTargets {
    /// Not modal: the same slots whatever the player announces.
    Fixed(Vec<TargetSlot>),
    /// Modal: `PerMode[i]` holds the slots of mode `i`. A mode with no target
    /// requirement has an empty list.
    PerMode(Vec<Vec<TargetSlot>>),
}

/// How many modes a modal spell takes (CR 700.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModeRange {
    pub mode_count: usize,
    pub min: u32,
    pub max: u32,
    /// The same index may be chosen twice (CR 700.2d).
    pub repeatable: bool,
    /// CR 700.2e: another player chooses, so the caster announces no modes.
    /// `min` and `max` are 0.
    pub chosen_by_opponent: bool,
    /// CR 700.2i: `max` is a pawprint budget, and this is the weight of each
    /// mode. `None` for every other spell.
    pub pawprint_weights: Option<Vec<u32>>,
    /// Ascending indices of the modes that can only be chosen alone: the
    /// resolver cannot yet run them beside another mode. Empty for almost
    /// every card. A selection of two or more may not contain one.
    pub solo_only: Vec<u32>,
}

/// A card the player may cast.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CastEntry {
    pub card: ObjectId,
    /// `None` when the card is not modal.
    pub modes: Option<ModeRange>,
    pub targets: EntryTargets,
    /// The mana cost has an `{X}`, so the Answer must announce a value.
    /// There is no maximum. An alternative cost never needs one (the engine
    /// leaves X unchecked there).
    pub x: bool,
    /// The card may be cast for its normal cost: from the hand, or a
    /// commander from the command zone (CR 903.8).
    pub normal: bool,
    /// Indices into the card's `alternative_costs` whose zone matches where
    /// the card is now, such as flashback from the graveyard.
    pub alternative_costs: Vec<usize>,
    /// CR 702.27a: the card has a buyback cost the player may add.
    pub buyback: bool,
}

/// An "add one mana of any color" choice that the activation announces
/// (CR 602.2b).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ColorChoice {
    pub count: u32,
    pub colors: Vec<Color>,
}

/// An activated ability the player may activate.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ActivateEntry {
    pub source: ObjectId,
    /// The ability's position among all abilities registered to `source`, as
    /// `PlayerActionKind::ActivateAbility::ability_index` expects.
    pub ability_index: u32,
    pub targets: Vec<TargetSlot>,
    /// The activation cost has an `{X}`.
    pub x: bool,
    /// CR 605: a mana ability. A UI can fold these entries away. A player
    /// can still float mana at priority.
    pub mana_ability: bool,
    pub color_choice: Option<ColorChoice>,
}

/// One playable option at a priority Decision. An entry is listed when timing,
/// zone, restrictions and target existence allow it, without checking that the
/// player can pay. A Payment Decision that cannot be met answers
/// `Payment(None)` and the cast unwinds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Entry {
    PlayLand { card: ObjectId },
    Cast(CastEntry),
    Activate(ActivateEntry),
    Pass,
}

/// A creature that may attack, and the defenders it may attack: opponents,
/// and the planeswalkers and battles they control (CR 506.4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AttackerEntry {
    pub attacker: ObjectId,
    pub defenders: Vec<Target>,
    /// CR 508.1d: the creature attacks each combat if able, so a declaration
    /// that leaves it home is refused.
    pub required: bool,
}

/// A creature that may block, and the attackers it may block. A menace
/// attacker (CR 702.111b) is listed only when the defending player has two
/// creatures that could block it. The engine checks that the blockers picked
/// add up to two when the Answer arrives.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlockerEntry {
    pub blocker: ObjectId,
    pub attackers: Vec<ObjectId>,
}

/// An attacker or blocker whose damage the player splits (CR 510.1c). The
/// Answer gives an amount, possibly zero, for every recipient.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DamageSplitEntry {
    pub source: ObjectId,
    pub power: u32,
    /// The blockers of an attacker, or the attackers a band forced a blocker
    /// onto (CR 702.22h).
    pub recipients: Vec<ObjectId>,
    /// CR 702.19b: a trampling attacker may send what is left after every
    /// blocker has lethal damage to this target.
    pub spill: Option<Target>,
}
