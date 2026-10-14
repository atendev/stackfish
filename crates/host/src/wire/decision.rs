use serde::Serialize;
use stackfish_engine::decision::{
    ActivateEntry, AttackerEntry, BlockerEntry, CastEntry, Decision, DamageSplitEntry, Entry, EntryTargets, ModeRange,
    PartitionRest, ReplyShape, TargetSlot,
};
use stackfish_engine::triggers::PendingTrigger;
use stackfish_engine::{AdditionalCost, CostSpec, TargetRequirement, TargetSpec};

use super::mana::mana_cost_string;
use super::{U64, WireCandidate, WireColor, WireTarget};

#[derive(Serialize, Clone, Debug)]
pub struct WireTrigger {
    pub source: U64,
    pub controller: u32,
    pub ability: U64,
    pub once_per_turn: bool,
    pub triggering_stack: Option<U64>,
}

impl From<&PendingTrigger> for WireTrigger {
    fn from(trigger: &PendingTrigger) -> Self {
        WireTrigger {
            source: trigger.source.into(),
            controller: trigger.controller.0,
            ability: U64(trigger.ability.0),
            once_per_turn: trigger.once_per_turn,
            triggering_stack: trigger.triggering_stack.map(U64::from),
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WireRest {
    LibraryBottom,
    Graveyard,
}

impl From<PartitionRest> for WireRest {
    fn from(rest: PartitionRest) -> Self {
        match rest {
            PartitionRest::LibraryBottom => WireRest::LibraryBottom,
            PartitionRest::Graveyard => WireRest::Graveyard,
        }
    }
}

/// An additional cost the payer still has to meet. The plan names the objects.
///
/// TODO: `sacrifice`, `discard`, `tap_permanents` and `exile_from_graveyard`
/// carry no filter, because the engine does not list the eligible objects for
/// a Payment Decision (ticket 003). A host has to read the cost text itself.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireAdditionalCost {
    TapSelf,
    Sacrifice,
    SacrificeSelf,
    Discard,
    DiscardSelf,
    TapPermanents { count: u32 },
    ExileFromGraveyard { count: u32 },
    LifePayment { amount: u32 },
    Other { description: String },
}

impl From<&AdditionalCost> for WireAdditionalCost {
    fn from(cost: &AdditionalCost) -> Self {
        match cost {
            AdditionalCost::TapSelf => WireAdditionalCost::TapSelf,
            AdditionalCost::Sacrifice { .. } => WireAdditionalCost::Sacrifice,
            AdditionalCost::SacrificeSelf => WireAdditionalCost::SacrificeSelf,
            AdditionalCost::Discard { .. } => WireAdditionalCost::Discard,
            AdditionalCost::DiscardSelf => WireAdditionalCost::DiscardSelf,
            AdditionalCost::TapPermanents { count, .. } => WireAdditionalCost::TapPermanents { count: *count },
            AdditionalCost::ExileFromGraveyard { count, .. } => {
                WireAdditionalCost::ExileFromGraveyard { count: *count }
            }
            AdditionalCost::LifePayment { amount } => WireAdditionalCost::LifePayment { amount: *amount },
            AdditionalCost::Other { description } => WireAdditionalCost::Other { description: description.clone() },
        }
    }
}

/// The cost a Payment Decision asks for. `mana` is brace notation such as
/// `{2}{R}`, empty when the cost has no mana part.
#[derive(Serialize, Clone, Debug)]
pub struct WireCost {
    pub mana: String,
    pub additional: Vec<WireAdditionalCost>,
}

impl From<&CostSpec> for WireCost {
    fn from(cost: &CostSpec) -> Self {
        WireCost {
            mana: cost.mana.as_ref().map(mana_cost_string).unwrap_or_default(),
            additional: cost.additional.iter().map(WireAdditionalCost::from).collect(),
        }
    }
}

/// One target word of a trigger: how many targets it takes and what it offers.
#[derive(Serialize, Clone, Debug)]
pub struct WireTargetWord {
    pub min: u32,
    /// Absent for an "N or more" word.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    pub candidates: Vec<WireTarget>,
}

fn word_bounds(requirement: &TargetRequirement) -> (u32, Option<u32>) {
    if requirement.up_to {
        (0, Some(requirement.count))
    } else if requirement.at_least {
        (requirement.count, None)
    } else {
        (requirement.count, Some(requirement.count))
    }
}

fn target_words(spec: &TargetSpec, candidates: &[Vec<stackfish_engine::Target>]) -> Vec<WireTargetWord> {
    let requirements: Vec<&TargetRequirement> = match spec {
        TargetSpec::Primitive(requirement) => vec![requirement],
        TargetSpec::Multiple(requirements) => requirements.iter().collect(),
    };
    candidates
        .iter()
        .enumerate()
        .map(|(i, word)| {
            let (min, max) = requirements.get(i).map_or((0, None), |r| word_bounds(r));
            WireTargetWord { min, max, candidates: word.iter().map(|t| (*t).into()).collect() }
        })
        .collect()
}

#[derive(Serialize, Clone, Debug)]
pub struct WireSlot {
    pub candidates: Vec<WireTarget>,
    pub min: u32,
    /// Absent for an "N or more" word.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

impl From<&TargetSlot> for WireSlot {
    fn from(slot: &TargetSlot) -> Self {
        WireSlot { candidates: slot.candidates.iter().map(|t| (*t).into()).collect(), min: slot.min, max: slot.max }
    }
}

fn slots(slots: &[TargetSlot]) -> Vec<WireSlot> {
    slots.iter().map(WireSlot::from).collect()
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireEntryTargets {
    Fixed { slots: Vec<WireSlot> },
    PerMode { modes: Vec<Vec<WireSlot>> },
}

impl From<&EntryTargets> for WireEntryTargets {
    fn from(targets: &EntryTargets) -> Self {
        match targets {
            EntryTargets::Fixed(list) => WireEntryTargets::Fixed { slots: slots(list) },
            EntryTargets::PerMode(modes) => WireEntryTargets::PerMode { modes: modes.iter().map(|m| slots(m)).collect() },
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct WireModeRange {
    pub mode_count: usize,
    pub min: u32,
    pub max: u32,
    pub repeatable: bool,
    pub chosen_by_opponent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pawprint_weights: Option<Vec<u32>>,
}

impl From<&ModeRange> for WireModeRange {
    fn from(range: &ModeRange) -> Self {
        WireModeRange {
            mode_count: range.mode_count,
            min: range.min,
            max: range.max,
            repeatable: range.repeatable,
            chosen_by_opponent: range.chosen_by_opponent,
            pawprint_weights: range.pawprint_weights.clone(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct WireColorChoice {
    pub count: u32,
    pub colors: Vec<WireColor>,
}

/// One playable option at a priority Decision. The engine lists an entry
/// without checking that the player can pay for it.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireEntry {
    PlayLand {
        card: U64,
    },
    Cast {
        card: U64,
        #[serde(skip_serializing_if = "Option::is_none")]
        modes: Option<WireModeRange>,
        targets: WireEntryTargets,
        x: bool,
        normal: bool,
        alternative_costs: Vec<usize>,
        buyback: bool,
    },
    Activate {
        source: U64,
        ability_index: u32,
        targets: Vec<WireSlot>,
        x: bool,
        mana_ability: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_choice: Option<WireColorChoice>,
    },
    Pass,
}

impl From<&Entry> for WireEntry {
    fn from(entry: &Entry) -> Self {
        match entry {
            Entry::PlayLand { card } => WireEntry::PlayLand { card: (*card).into() },
            Entry::Cast(CastEntry { card, modes, targets, x, normal, alternative_costs, buyback }) => WireEntry::Cast {
                card: (*card).into(),
                modes: modes.as_ref().map(WireModeRange::from),
                targets: targets.into(),
                x: *x,
                normal: *normal,
                alternative_costs: alternative_costs.clone(),
                buyback: *buyback,
            },
            Entry::Activate(ActivateEntry { source, ability_index, targets, x, mana_ability, color_choice }) => {
                WireEntry::Activate {
                    source: (*source).into(),
                    ability_index: *ability_index,
                    targets: slots(targets),
                    x: *x,
                    mana_ability: *mana_ability,
                    color_choice: color_choice.as_ref().map(|choice| WireColorChoice {
                        count: choice.count,
                        colors: choice.colors.iter().map(|c| (*c).into()).collect(),
                    }),
                }
            }
            Entry::Pass => WireEntry::Pass,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct WireAttacker {
    pub attacker: U64,
    pub defenders: Vec<WireTarget>,
}

impl From<&AttackerEntry> for WireAttacker {
    fn from(entry: &AttackerEntry) -> Self {
        WireAttacker {
            attacker: entry.attacker.into(),
            defenders: entry.defenders.iter().map(|t| (*t).into()).collect(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct WireBlocker {
    pub blocker: U64,
    pub attackers: Vec<U64>,
}

impl From<&BlockerEntry> for WireBlocker {
    fn from(entry: &BlockerEntry) -> Self {
        WireBlocker { blocker: entry.blocker.into(), attackers: entry.attackers.iter().map(|a| (*a).into()).collect() }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct WireDamageSplit {
    pub source: U64,
    pub power: u32,
    pub recipients: Vec<U64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill: Option<WireTarget>,
}

impl From<&DamageSplitEntry> for WireDamageSplit {
    fn from(entry: &DamageSplitEntry) -> Self {
        WireDamageSplit {
            source: entry.source.into(),
            power: entry.power,
            recipients: entry.recipients.iter().map(|r| (*r).into()).collect(),
            spill: entry.spill.map(WireTarget::from),
        }
    }
}

/// The reply a Decision asks for. Each variant pairs with one reply kind of the
/// same name.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum WireShape {
    Pick { candidates: Vec<WireCandidate>, min: usize, max: usize },
    YesNo,
    Order { items: Vec<WireTrigger> },
    Modes { mode_count: usize, min: u32, max: u32, repeatable: bool },
    Partition { cards: Vec<U64>, rest: WireRest },
    Payment { cost: WireCost },
    Targets { words: Vec<WireTargetWord> },
    MayCast {
        #[serde(skip_serializing_if = "Option::is_none")]
        card: Option<U64>,
    },
    Action { entries: Vec<WireEntry> },
    DeclareAttackers { attackers: Vec<WireAttacker> },
    DeclareBlockers { blockers: Vec<WireBlocker> },
    AssignDamage { splits: Vec<WireDamageSplit> },
}

impl From<&ReplyShape> for WireShape {
    fn from(shape: &ReplyShape) -> Self {
        match shape {
            ReplyShape::Pick { candidates, min, max } => WireShape::Pick {
                candidates: candidates.iter().map(|c| (*c).into()).collect(),
                min: *min,
                max: *max,
            },
            ReplyShape::YesNo => WireShape::YesNo,
            ReplyShape::Order { items } => WireShape::Order { items: items.iter().map(WireTrigger::from).collect() },
            ReplyShape::Modes { mode_count, min, max, repeatable } => {
                WireShape::Modes { mode_count: *mode_count, min: *min, max: *max, repeatable: *repeatable }
            }
            ReplyShape::Partition { cards, rest } => WireShape::Partition {
                cards: cards.iter().map(|c| (*c).into()).collect(),
                rest: (*rest).into(),
            },
            ReplyShape::Payment { cost } => WireShape::Payment { cost: cost.into() },
            ReplyShape::Targets { spec, candidates } => WireShape::Targets { words: target_words(spec, candidates) },
            ReplyShape::MayCast { card } => WireShape::MayCast { card: card.map(U64::from) },
            ReplyShape::Action { entries } => WireShape::Action { entries: entries.iter().map(WireEntry::from).collect() },
            ReplyShape::DeclareAttackers { attackers } => {
                WireShape::DeclareAttackers { attackers: attackers.iter().map(WireAttacker::from).collect() }
            }
            ReplyShape::DeclareBlockers { blockers } => {
                WireShape::DeclareBlockers { blockers: blockers.iter().map(WireBlocker::from).collect() }
            }
            ReplyShape::AssignDamage { splits } => {
                WireShape::AssignDamage { splits: splits.iter().map(WireDamageSplit::from).collect() }
            }
        }
    }
}

/// The point where the engine waits on `player`. `id` goes up by one per
/// Decision and an Answer echoes it. `mechanic` is a display label, so a host
/// must not branch on it.
#[derive(Serialize, Clone, Debug)]
pub struct WireDecision {
    pub id: U64,
    pub player: u32,
    pub mechanic: &'static str,
    #[serde(flatten)]
    pub shape: WireShape,
}

impl From<&Decision> for WireDecision {
    fn from(decision: &Decision) -> Self {
        WireDecision {
            id: U64(decision.id.0),
            player: decision.player.0,
            mechanic: decision.mechanic,
            shape: (&decision.shape).into(),
        }
    }
}
