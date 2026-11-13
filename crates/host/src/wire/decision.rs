use serde::Serialize;
use stackfish_engine::decision::{
    ActivateEntry, AttackerEntry, BlockerEntry, Candidate, CastEntry, CastOptions,
    DamageSplitEntry, Decision, Entry, EntryTargets, MinBlockers, ModeRange, PartitionRest,
    ReplyShape, TargetSlot,
};
use stackfish_engine::triggers::PendingTrigger;
use stackfish_engine::{AdditionalCost, CostSpec, ObjectId, Target, TargetRequirement, TargetSpec};

use super::mana::mana_cost_string;
use super::{U64, WireCandidate, WireColor, WireIdentity, WireTarget};

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireTrigger {
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
pub(crate) enum WireRest {
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
/// A filtered cost lists the objects that may pay it in `candidates`.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum WireAdditionalCost {
    TapSelf,
    Sacrifice { candidates: Vec<U64> },
    SacrificeSelf,
    Discard { candidates: Vec<U64> },
    DiscardSelf,
    TapPermanents { count: u32, candidates: Vec<U64> },
    ExileFromGraveyard { count: u32, candidates: Vec<U64> },
    LifePayment { amount: u32 },
    Other { description: String },
}

impl WireAdditionalCost {
    fn new(cost: &AdditionalCost, eligible: &[ObjectId]) -> Self {
        let candidates = || eligible.iter().map(|id| (*id).into()).collect();
        match cost {
            AdditionalCost::TapSelf => WireAdditionalCost::TapSelf,
            AdditionalCost::Sacrifice { .. } => WireAdditionalCost::Sacrifice {
                candidates: candidates(),
            },
            AdditionalCost::SacrificeSelf => WireAdditionalCost::SacrificeSelf,
            AdditionalCost::Discard { .. } => WireAdditionalCost::Discard {
                candidates: candidates(),
            },
            AdditionalCost::DiscardSelf => WireAdditionalCost::DiscardSelf,
            AdditionalCost::TapPermanents { count, .. } => WireAdditionalCost::TapPermanents {
                count: *count,
                candidates: candidates(),
            },
            AdditionalCost::ExileFromGraveyard { count, .. } => {
                WireAdditionalCost::ExileFromGraveyard {
                    count: *count,
                    candidates: candidates(),
                }
            }
            AdditionalCost::LifePayment { amount } => {
                WireAdditionalCost::LifePayment { amount: *amount }
            }
            AdditionalCost::Other { description } => WireAdditionalCost::Other {
                description: description.clone(),
            },
        }
    }
}

/// The cost a Payment Decision asks for. `mana` is brace notation such as
/// `{2}{R}`, empty when the cost has no mana part.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireCost {
    pub mana: String,
    pub additional: Vec<WireAdditionalCost>,
}

impl WireCost {
    /// `eligible` holds one list per additional cost, as `ReplyShape::Payment` gives it.
    fn new(cost: &CostSpec, eligible: &[Vec<ObjectId>]) -> Self {
        WireCost {
            mana: cost.mana.as_ref().map(mana_cost_string).unwrap_or_default(),
            additional: cost
                .additional
                .iter()
                .enumerate()
                .map(|(i, additional)| {
                    WireAdditionalCost::new(additional, eligible.get(i).map_or(&[], Vec::as_slice))
                })
                .collect(),
        }
    }
}

/// What a cast offered by a `may_cast` Decision announces. `x` asks for an X
/// value, and 0 is the only legal one.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireCastOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<WireModeRange>,
    pub targets: WireEntryTargets,
    pub x: bool,
}

impl From<&CastOptions> for WireCastOptions {
    fn from(options: &CastOptions) -> Self {
        WireCastOptions {
            modes: options.modes.as_ref().map(WireModeRange::from),
            targets: (&options.targets).into(),
            x: options.x,
        }
    }
}

/// One target word of a trigger: how many targets it takes and what it offers.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireTargetWord {
    pub min: u32,
    /// Absent for an "N or more" word.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    pub candidates: Vec<WireTarget>,
    /// CR 601.2d: the total this word's targets divide, absent when undivided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divide_total: Option<u32>,
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

fn target_words(
    spec: &TargetSpec,
    candidates: &[Vec<stackfish_engine::Target>],
    divide_totals: &[Option<u32>],
) -> Vec<WireTargetWord> {
    let requirements: Vec<&TargetRequirement> = match spec {
        TargetSpec::Primitive(requirement) => vec![requirement],
        TargetSpec::Multiple(requirements) => requirements.iter().collect(),
    };
    candidates
        .iter()
        .enumerate()
        .map(|(i, word)| {
            let (min, max) = requirements.get(i).map_or((0, None), |r| word_bounds(r));
            let divide_total = divide_totals.get(i).copied().flatten();
            WireTargetWord {
                min,
                // CR 601.2d: each target takes at least 1 of the total.
                max: match (max, divide_total) {
                    (Some(max), Some(total)) => Some(max.min(total)),
                    (None, total) => total,
                    (max, None) => max,
                },
                candidates: word.iter().map(|t| (*t).into()).collect(),
                divide_total,
            }
        })
        .collect()
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireSlot {
    pub candidates: Vec<WireTarget>,
    pub min: u32,
    /// Absent for an "N or more" word.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    /// The candidates are every target legal for some announced X.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub x_dependent: bool,
    /// CR 601.2d: the total this word's targets divide, absent when undivided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divide_total: Option<u32>,
}

impl From<&TargetSlot> for WireSlot {
    fn from(slot: &TargetSlot) -> Self {
        WireSlot {
            candidates: slot.candidates.iter().map(|t| (*t).into()).collect(),
            min: slot.min,
            max: slot.max,
            x_dependent: slot.x_dependent,
            divide_total: slot.divide_total,
        }
    }
}

fn slots(slots: &[TargetSlot]) -> Vec<WireSlot> {
    slots.iter().map(WireSlot::from).collect()
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum WireEntryTargets {
    Fixed { slots: Vec<WireSlot> },
    PerMode { modes: Vec<Vec<WireSlot>> },
}

impl From<&EntryTargets> for WireEntryTargets {
    fn from(targets: &EntryTargets) -> Self {
        match targets {
            EntryTargets::Fixed(list) => WireEntryTargets::Fixed { slots: slots(list) },
            EntryTargets::PerMode(modes) => WireEntryTargets::PerMode {
                modes: modes.iter().map(|m| slots(m)).collect(),
            },
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireModeRange {
    pub mode_count: usize,
    pub min: u32,
    pub max: u32,
    pub repeatable: bool,
    pub chosen_by_opponent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pawprint_weights: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub solo_only: Vec<u32>,
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
            solo_only: range.solo_only.clone(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireColorChoice {
    pub count: u32,
    pub colors: Vec<WireColor>,
}

/// One playable option at a priority Decision. The engine lists an entry
/// without checking that the player can pay for it.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum WireEntry {
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
    /// A modal ability has `modes` and its slots per mode in `mode_targets`,
    /// with `targets` empty. Both are absent for any other ability.
    Activate {
        source: U64,
        ability_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        modes: Option<WireModeRange>,
        targets: Vec<WireSlot>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        mode_targets: Vec<Vec<WireSlot>>,
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
            Entry::PlayLand { card } => WireEntry::PlayLand {
                card: (*card).into(),
            },
            Entry::Cast(CastEntry {
                card,
                modes,
                targets,
                x,
                normal,
                alternative_costs,
                buyback,
            }) => WireEntry::Cast {
                card: (*card).into(),
                modes: modes.as_ref().map(WireModeRange::from),
                targets: targets.into(),
                x: *x,
                normal: *normal,
                alternative_costs: alternative_costs.clone(),
                buyback: *buyback,
            },
            Entry::Activate(ActivateEntry {
                source,
                ability_index,
                modes,
                targets,
                x,
                mana_ability,
                color_choice,
            }) => WireEntry::Activate {
                source: (*source).into(),
                ability_index: *ability_index,
                modes: modes.as_ref().map(WireModeRange::from),
                targets: match targets {
                    EntryTargets::Fixed(list) => slots(list),
                    EntryTargets::PerMode(_) => Vec::new(),
                },
                mode_targets: match targets {
                    EntryTargets::Fixed(_) => Vec::new(),
                    EntryTargets::PerMode(modes) => modes.iter().map(|m| slots(m)).collect(),
                },
                x: *x,
                mana_ability: *mana_ability,
                color_choice: color_choice.as_ref().map(|choice| WireColorChoice {
                    count: choice.count,
                    colors: choice.colors.iter().map(|c| (*c).into()).collect(),
                }),
            },
            Entry::Pass => WireEntry::Pass,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireAttacker {
    pub attacker: U64,
    pub defenders: Vec<WireTarget>,
    pub required: bool,
}

impl From<&AttackerEntry> for WireAttacker {
    fn from(entry: &AttackerEntry) -> Self {
        WireAttacker {
            attacker: entry.attacker.into(),
            defenders: entry.defenders.iter().map(|t| (*t).into()).collect(),
            required: entry.required,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireBlocker {
    pub blocker: U64,
    pub attackers: Vec<U64>,
}

impl From<&BlockerEntry> for WireBlocker {
    fn from(entry: &BlockerEntry) -> Self {
        WireBlocker {
            blocker: entry.blocker.into(),
            attackers: entry.attackers.iter().map(|a| (*a).into()).collect(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireMinBlockers {
    pub attacker: U64,
    pub count: usize,
}

impl From<&MinBlockers> for WireMinBlockers {
    fn from(need: &MinBlockers) -> Self {
        WireMinBlockers {
            attacker: need.attacker.into(),
            count: need.count,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireDamageSplit {
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
pub(crate) enum WireShape {
    Pick {
        candidates: Vec<WireCandidate>,
        min: usize,
        max: usize,
    },
    YesNo,
    Order {
        items: Vec<WireTrigger>,
    },
    Modes {
        mode_count: usize,
        min: u32,
        max: u32,
        repeatable: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        solo_only: Vec<u32>,
    },
    Partition {
        cards: Vec<U64>,
        rest: WireRest,
    },
    /// `mana_abilities` are activate entries an `action` reply may name before
    /// paying (CR 605.3a).
    Payment {
        cost: WireCost,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        mana_abilities: Vec<WireEntry>,
    },
    Targets {
        words: Vec<WireTargetWord>,
    },
    MayCast {
        #[serde(skip_serializing_if = "Option::is_none")]
        card: Option<U64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<WireCastOptions>,
    },
    Action {
        entries: Vec<WireEntry>,
    },
    DeclareAttackers {
        attackers: Vec<WireAttacker>,
    },
    DeclareBands {
        attackers: Vec<U64>,
    },
    DeclareBlockers {
        blockers: Vec<WireBlocker>,
        /// Attackers that need more than one blocker if blocked at all (CR 509.1b).
        min_blockers: Vec<WireMinBlockers>,
    },
    AssignDamage {
        splits: Vec<WireDamageSplit>,
    },
    /// What a Seat that is not choosing sees: the reply kind and the option count.
    /// `candidates` lists only the options in public zones.
    Opaque {
        reply: &'static str,
        count: usize,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        candidates: Vec<WireCandidate>,
    },
}

impl From<&ReplyShape> for WireShape {
    fn from(shape: &ReplyShape) -> Self {
        match shape {
            ReplyShape::Pick {
                candidates,
                min,
                max,
            } => WireShape::Pick {
                candidates: candidates.iter().map(|c| (*c).into()).collect(),
                min: *min,
                max: *max,
            },
            ReplyShape::YesNo => WireShape::YesNo,
            ReplyShape::Order { items } => WireShape::Order {
                items: items.iter().map(WireTrigger::from).collect(),
            },
            ReplyShape::Modes {
                mode_count,
                min,
                max,
                repeatable,
                solo_only,
            } => WireShape::Modes {
                mode_count: *mode_count,
                min: *min,
                max: *max,
                repeatable: *repeatable,
                solo_only: solo_only.clone(),
            },
            ReplyShape::Partition { cards, rest } => WireShape::Partition {
                cards: cards.iter().map(|c| (*c).into()).collect(),
                rest: (*rest).into(),
            },
            ReplyShape::Payment {
                cost,
                eligible,
                mana_abilities,
            } => WireShape::Payment {
                cost: WireCost::new(cost, eligible),
                mana_abilities: mana_abilities
                    .iter()
                    .map(|entry| WireEntry::from(&Entry::Activate(entry.clone())))
                    .collect(),
            },
            ReplyShape::Targets {
                spec,
                candidates,
                divide_totals,
            } => WireShape::Targets {
                words: target_words(spec, candidates, divide_totals),
            },
            ReplyShape::MayCast { card, options } => WireShape::MayCast {
                card: card.map(U64::from),
                options: options.as_ref().map(WireCastOptions::from),
            },
            ReplyShape::Action { entries } => WireShape::Action {
                entries: entries.iter().map(WireEntry::from).collect(),
            },
            ReplyShape::DeclareAttackers { attackers } => WireShape::DeclareAttackers {
                attackers: attackers.iter().map(WireAttacker::from).collect(),
            },
            ReplyShape::DeclareBands { attackers } => WireShape::DeclareBands {
                attackers: attackers.iter().map(|a| (*a).into()).collect(),
            },
            ReplyShape::DeclareBlockers {
                blockers,
                min_blockers,
            } => WireShape::DeclareBlockers {
                blockers: blockers.iter().map(WireBlocker::from).collect(),
                min_blockers: min_blockers.iter().map(WireMinBlockers::from).collect(),
            },
            ReplyShape::AssignDamage { splits } => WireShape::AssignDamage {
                splits: splits.iter().map(WireDamageSplit::from).collect(),
            },
        }
    }
}

/// The identity of a card a Decision offers from a zone the chooser cannot
/// otherwise see, such as a library search or a scry.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireCardIdentity {
    pub object: U64,
    pub identity: WireIdentity,
}

/// The point where the engine waits on `player`. `id` goes up by one per
/// Decision and an Answer echoes it. `mechanic` is a display label, so a host
/// must not branch on it. `identities` names the cards in hidden zones that
/// the chooser is shown.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireDecision {
    pub id: U64,
    pub player: u32,
    pub mechanic: &'static str,
    #[serde(flatten)]
    pub shape: WireShape,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub identities: Vec<WireCardIdentity>,
}

impl WireDecision {
    /// The Decision as the chooser sees it.
    pub(crate) fn full(decision: &Decision, identities: Vec<WireCardIdentity>) -> Self {
        WireDecision {
            id: U64(decision.id.0),
            player: decision.player.0,
            mechanic: decision.mechanic,
            shape: (&decision.shape).into(),
            identities,
        }
    }

    /// The Decision as any other Seat sees it. `public` says which of the
    /// Decision's object candidates lie in public zones.
    pub(crate) fn opaque(decision: &Decision, public: impl Fn(&Candidate) -> bool) -> Self {
        let candidates = pick_candidates(&decision.shape)
            .into_iter()
            .filter(|c| public(c))
            .map(WireCandidate::from)
            .collect();
        WireDecision {
            id: U64(decision.id.0),
            player: decision.player.0,
            mechanic: decision.mechanic,
            shape: WireShape::Opaque {
                reply: reply_kind(&decision.shape),
                count: option_count(&decision.shape),
                candidates,
            },
            identities: Vec::new(),
        }
    }
}

/// The `kind` an Answer to this shape carries.
fn reply_kind(shape: &ReplyShape) -> &'static str {
    match shape {
        ReplyShape::Pick { .. } => "pick",
        ReplyShape::YesNo => "yes_no",
        ReplyShape::Order { .. } => "order",
        ReplyShape::Modes { .. } => "modes",
        ReplyShape::Partition { .. } => "partition",
        ReplyShape::Payment { .. } => "payment",
        ReplyShape::Targets { .. } => "targets",
        ReplyShape::MayCast { .. } => "may_cast",
        ReplyShape::Action { .. } => "action",
        ReplyShape::DeclareAttackers { .. } => "declare_attackers",
        ReplyShape::DeclareBands { .. } => "declare_bands",
        ReplyShape::DeclareBlockers { .. } => "declare_blockers",
        ReplyShape::AssignDamage { .. } => "assign_damage",
    }
}

/// How many options the shape lists. A shape with nothing to list counts 0.
fn option_count(shape: &ReplyShape) -> usize {
    match shape {
        ReplyShape::Pick { candidates, .. } => candidates.len(),
        ReplyShape::Order { items } => items.len(),
        ReplyShape::Partition { cards, .. } => cards.len(),
        ReplyShape::Targets { candidates, .. } => candidates.iter().map(Vec::len).sum(),
        ReplyShape::Action { entries } => entries.len(),
        ReplyShape::DeclareAttackers { attackers } => attackers.len(),
        ReplyShape::DeclareBands { attackers } => attackers.len(),
        ReplyShape::DeclareBlockers { blockers, .. } => blockers.len(),
        ReplyShape::AssignDamage { splits } => splits.len(),
        ReplyShape::YesNo
        | ReplyShape::Modes { .. }
        | ReplyShape::Payment { .. }
        | ReplyShape::MayCast { .. } => 0,
    }
}

/// The candidates a non-chooser may be shown if they lie in public zones:
/// those of a Pick and the flattened words of a Targets Decision.
fn pick_candidates(shape: &ReplyShape) -> Vec<Candidate> {
    match shape {
        ReplyShape::Pick { candidates, .. } => candidates.clone(),
        ReplyShape::Targets { candidates, .. } => candidates
            .iter()
            .flatten()
            .map(|t| Candidate::Target(*t))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every object a Decision asks the chooser to pick or order from a zone that
/// may be hidden: Pick and Targets candidates and Partition cards. Priority
/// entries are left out, because they name the chooser's own hand and the
/// battlefield.
pub(crate) fn offered_objects(shape: &ReplyShape) -> Vec<ObjectId> {
    let mut found: Vec<ObjectId> = pick_candidates(shape)
        .into_iter()
        .filter_map(|c| match c {
            Candidate::Target(Target::Object(id)) => Some(id),
            _ => None,
        })
        .collect();
    if let ReplyShape::Partition { cards, .. } = shape {
        found.extend(cards);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use stackfish_engine::{ObjectId, PlayerId, Target};

    #[test]
    fn an_attacker_entry_carries_its_required_flag() {
        let entry = AttackerEntry {
            attacker: ObjectId(7),
            defenders: vec![Target::Player(PlayerId(1))],
            required: true,
        };
        let json = serde_json::to_value(WireAttacker::from(&entry)).unwrap();
        assert_eq!(json["required"], true);
        assert_eq!(json["attacker"], "7");
    }

    // CR 508.1e
    #[test]
    fn a_band_decision_lists_attacker_ids() {
        let shape = ReplyShape::DeclareBands {
            attackers: vec![ObjectId(2), ObjectId(5)],
        };
        let json = serde_json::to_value(WireShape::from(&shape)).unwrap();
        assert_eq!(json["shape"], "declare_bands");
        assert_eq!(json["attackers"], serde_json::json!(["2", "5"]));
        assert_eq!(reply_kind(&shape), "declare_bands");
        assert_eq!(option_count(&shape), 2);
    }

    // CR 605.3a
    #[test]
    fn a_payment_lists_its_mana_abilities_as_activate_entries() {
        let cost = CostSpec {
            mana: None,
            additional: Vec::new(),
            alternative: Vec::new(),
        };
        let shape = |mana_abilities| ReplyShape::Payment {
            cost: cost.clone(),
            eligible: Vec::new(),
            mana_abilities,
        };
        let entry = ActivateEntry {
            source: ObjectId(4),
            ability_index: 0,
            modes: None,
            targets: EntryTargets::Fixed(Vec::new()),
            x: false,
            mana_ability: true,
            color_choice: None,
        };
        let json = serde_json::to_value(WireShape::from(&shape(vec![entry]))).unwrap();
        assert_eq!(json["mana_abilities"][0]["kind"], "activate");
        assert_eq!(json["mana_abilities"][0]["source"], "4");
        let json = serde_json::to_value(WireShape::from(&shape(Vec::new()))).unwrap();
        assert!(json.get("mana_abilities").is_none(), "{json}");
    }

    // CR 602.2b, 700.2a
    #[test]
    fn a_modal_activate_entry_lists_its_range_and_slots_per_mode() {
        let slot = TargetSlot {
            candidates: vec![Target::Object(ObjectId(9))],
            min: 1,
            max: Some(1),
            x_dependent: false,
            divide_total: None,
        };
        let entry = |modes, targets| {
            Entry::Activate(ActivateEntry {
                source: ObjectId(4),
                ability_index: 1,
                modes,
                targets,
                x: false,
                mana_ability: false,
                color_choice: None,
            })
        };
        let range = ModeRange {
            mode_count: 2,
            min: 1,
            max: 1,
            repeatable: false,
            chosen_by_opponent: false,
            pawprint_weights: None,
            solo_only: Vec::new(),
        };
        let modal = entry(
            Some(range),
            EntryTargets::PerMode(vec![vec![slot.clone()], Vec::new()]),
        );
        let json = serde_json::to_value(WireEntry::from(&modal)).unwrap();
        assert_eq!(json["modes"]["mode_count"], 2);
        assert_eq!(json["targets"], serde_json::json!([]));
        assert_eq!(json["mode_targets"][0][0]["candidates"][0]["id"], "9");
        assert_eq!(json["mode_targets"][1], serde_json::json!([]));
        let plain = entry(None, EntryTargets::Fixed(vec![slot]));
        let json = serde_json::to_value(WireEntry::from(&plain)).unwrap();
        assert!(json.get("modes").is_none(), "{json}");
        assert!(json.get("mode_targets").is_none(), "{json}");
        assert_eq!(json["targets"][0]["min"], 1);
    }

    #[test]
    fn a_mode_range_carries_its_solo_only_modes_and_omits_an_empty_list() {
        let range = |solo_only: Vec<u32>| ModeRange {
            mode_count: 3,
            min: 2,
            max: 2,
            repeatable: false,
            chosen_by_opponent: false,
            pawprint_weights: None,
            solo_only,
        };
        let json = serde_json::to_value(WireModeRange::from(&range(vec![1]))).unwrap();
        assert_eq!(json["solo_only"], serde_json::json!([1]));
        let json = serde_json::to_value(WireModeRange::from(&range(Vec::new()))).unwrap();
        assert!(json.get("solo_only").is_none(), "{json}");
    }

    #[test]
    fn a_modes_shape_carries_its_solo_only_modes() {
        let shape = ReplyShape::Modes {
            mode_count: 3,
            min: 2,
            max: 2,
            repeatable: false,
            solo_only: vec![1],
        };
        let json = serde_json::to_value(WireShape::from(&shape)).unwrap();
        assert_eq!(json["shape"], "modes");
        assert_eq!(json["solo_only"], serde_json::json!([1]));
    }

    fn any_filter() -> stackfish_engine::FilterSpec {
        stackfish_engine::FilterSpec {
            card_types: None,
            supertypes: None,
            subtypes: None,
            controller: None,
            random: false,
            extra: None,
        }
    }

    #[test]
    fn a_payment_shape_lists_candidates_on_each_filtered_cost() {
        let cost = CostSpec {
            mana: None,
            additional: vec![
                AdditionalCost::Sacrifice {
                    filter: any_filter(),
                },
                AdditionalCost::TapPermanents {
                    filter: any_filter(),
                    count: 2,
                },
                AdditionalCost::LifePayment { amount: 2 },
            ],
            alternative: Vec::new(),
        };
        let shape = ReplyShape::Payment {
            cost,
            eligible: vec![
                vec![ObjectId(3), ObjectId(9)],
                vec![ObjectId(4)],
                Vec::new(),
            ],
            mana_abilities: Vec::new(),
        };
        let json = serde_json::to_value(WireShape::from(&shape)).unwrap();
        let additional = &json["cost"]["additional"];
        assert_eq!(additional[0]["kind"], "sacrifice");
        assert_eq!(additional[0]["candidates"], serde_json::json!(["3", "9"]));
        assert_eq!(additional[1]["count"], 2);
        assert_eq!(additional[1]["candidates"], serde_json::json!(["4"]));
        assert!(additional[2].get("candidates").is_none(), "{json}");
    }

    #[test]
    fn a_may_cast_shape_carries_its_options() {
        let shape = ReplyShape::MayCast {
            card: Some(ObjectId(5)),
            options: Some(CastOptions {
                modes: None,
                targets: EntryTargets::Fixed(vec![TargetSlot {
                    candidates: vec![Target::Player(PlayerId(1))],
                    min: 1,
                    max: Some(1),
                    x_dependent: false,
                    divide_total: None,
                }]),
                x: true,
            }),
        };
        let json = serde_json::to_value(WireShape::from(&shape)).unwrap();
        assert_eq!(json["shape"], "may_cast");
        assert_eq!(json["card"], "5");
        assert_eq!(json["options"]["x"], true);
        assert_eq!(json["options"]["targets"]["kind"], "fixed");
        assert!(json["options"].get("modes").is_none(), "{json}");

        let declined = ReplyShape::MayCast {
            card: None,
            options: None,
        };
        let json = serde_json::to_value(WireShape::from(&declined)).unwrap();
        assert!(json.get("options").is_none(), "{json}");
    }
}
