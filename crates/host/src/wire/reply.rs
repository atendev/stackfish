use serde::Deserialize;
use stackfish_engine::decision::{CastAnnouncement, Reply};
use stackfish_engine::{CostPayment, ObjectId, PaymentPlan};

use super::mana::WireManaUnit;
use super::{U64, WireCandidate, WireColor, WireTarget};

/// The body of an `answer` request: the Decision it answers and the reply.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct AnswerBody {
    pub decision: U64,
    pub reply: WireReply,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WirePayment {
    Mana {
        units: Vec<WireManaUnit>,
        #[serde(default)]
        phyrexian_life: u32,
    },
    Sacrifice {
        object: U64,
    },
    Discard {
        object: U64,
    },
    Exile {
        object: U64,
    },
    Tap {
        object: U64,
    },
    Life {
        amount: u32,
    },
    Other {
        description: String,
    },
}

impl From<WirePayment> for CostPayment {
    fn from(payment: WirePayment) -> Self {
        match payment {
            WirePayment::Mana {
                units,
                phyrexian_life,
            } => CostPayment::Mana {
                units: units.into_iter().map(Into::into).collect(),
                phyrexian_life,
            },
            WirePayment::Sacrifice { object } => CostPayment::Sacrifice(object.into()),
            WirePayment::Discard { object } => CostPayment::Discard(object.into()),
            WirePayment::Exile { object } => CostPayment::Exile(object.into()),
            WirePayment::Tap { object } => CostPayment::Tap(object.into()),
            WirePayment::Life { amount } => CostPayment::Life(amount),
            WirePayment::Other { description } => CostPayment::Other { description },
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WirePlan {
    pub payments: Vec<WirePayment>,
}

impl From<WirePlan> for PaymentPlan {
    fn from(plan: WirePlan) -> Self {
        PaymentPlan {
            payments: plan.payments.into_iter().map(Into::into).collect(),
        }
    }
}

/// What the player announces to cast a card from a Madness or Cascade Decision.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WireCast {
    #[serde(default)]
    pub modes: Vec<u32>,
    #[serde(default)]
    pub targets: Vec<WireTarget>,
    pub x_value: Option<u32>,
}

impl From<WireCast> for CastAnnouncement {
    fn from(cast: WireCast) -> Self {
        CastAnnouncement {
            modes: cast.modes,
            targets: cast.targets.into_iter().map(Into::into).collect(),
            x_value: cast.x_value,
        }
    }
}

/// The priority actions that an `action` reply can carry. Every other
/// `PlayerActionKind` has a reply shape of its own.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireAction {
    CastSpell {
        card: U64,
        #[serde(default)]
        modes: Vec<u32>,
        #[serde(default)]
        targets: Vec<WireTarget>,
        x_value: Option<u32>,
        alternative_cost: Option<usize>,
        #[serde(default)]
        pay_buyback: bool,
    },
    ActivateAbility {
        source: U64,
        ability_index: u32,
        #[serde(default)]
        targets: Vec<WireTarget>,
        #[serde(default)]
        chosen_colors: Vec<WireColor>,
        x_value: Option<u32>,
    },
    PlayLand {
        card: U64,
    },
    PassPriority,
}

impl WireAction {
    fn into_action(self, player: stackfish_engine::PlayerId) -> stackfish_engine::PlayerAction {
        use stackfish_engine::PlayerActionKind as Kind;
        let kind = match self {
            WireAction::CastSpell {
                card,
                modes,
                targets,
                x_value,
                alternative_cost,
                pay_buyback,
            } => Kind::CastSpell {
                card: card.into(),
                modes,
                targets: targets.into_iter().map(Into::into).collect(),
                x_value,
                alternative_cost,
                pay_buyback,
            },
            WireAction::ActivateAbility {
                source,
                ability_index,
                targets,
                chosen_colors,
                x_value,
            } => Kind::ActivateAbility {
                source: source.into(),
                ability_index,
                targets: targets.into_iter().map(Into::into).collect(),
                chosen_colors: chosen_colors.into_iter().map(Into::into).collect(),
                x_value,
            },
            WireAction::PlayLand { card } => Kind::PlayLand { card: card.into() },
            WireAction::PassPriority => Kind::PassPriority,
        };
        stackfish_engine::PlayerAction::new(player, kind)
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WireAttack {
    pub attacker: U64,
    pub defender: WireTarget,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WireBlock {
    pub blocker: U64,
    pub attacker: U64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WireDamage {
    pub recipient: U64,
    pub amount: u32,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WireAssignment {
    pub source: U64,
    pub splits: Vec<WireDamage>,
}

/// An Answer's reply. Its `kind` must match the open Decision's `shape`.
/// `order` holds indices into the Decision's `items`.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireReply {
    Pick {
        picks: Vec<WireCandidate>,
    },
    YesNo {
        value: bool,
    },
    Order {
        order: Vec<usize>,
    },
    Modes {
        modes: Vec<u32>,
    },
    Partition {
        top: Vec<U64>,
        rest: Vec<U64>,
    },
    /// A missing or null `plan` means the player cannot pay or declines.
    Payment {
        plan: Option<WirePlan>,
    },
    Targets {
        targets: Vec<Vec<WireTarget>>,
    },
    /// A missing or null `cast` declines.
    MayCast {
        cast: Option<WireCast>,
    },
    Action {
        action: WireAction,
    },
    DeclareAttackers {
        attackers: Vec<WireAttack>,
    },
    DeclareBlockers {
        blocks: Vec<WireBlock>,
    },
    AssignDamage {
        assignments: Vec<WireAssignment>,
    },
}

fn objects(ids: Vec<U64>) -> Vec<ObjectId> {
    ids.into_iter().map(ObjectId::from).collect()
}

impl WireReply {
    /// The engine reply. `player` is the Seat's player, which a priority action
    /// names.
    pub fn into_reply(self, player: stackfish_engine::PlayerId) -> Reply {
        match self {
            WireReply::Pick { picks } => Reply::Pick(picks.into_iter().map(Into::into).collect()),
            WireReply::YesNo { value } => Reply::YesNo(value),
            WireReply::Order { order } => Reply::Order(order),
            WireReply::Modes { modes } => Reply::Modes(modes),
            WireReply::Partition { top, rest } => Reply::Partition {
                top: objects(top),
                rest: objects(rest),
            },
            WireReply::Payment { plan } => Reply::Payment(plan.map(Into::into)),
            WireReply::Targets { targets } => Reply::Targets(
                targets
                    .into_iter()
                    .map(|word| word.into_iter().map(Into::into).collect())
                    .collect(),
            ),
            WireReply::MayCast { cast } => Reply::MayCast(cast.map(Into::into)),
            WireReply::Action { action } => Reply::Action(action.into_action(player)),
            WireReply::DeclareAttackers { attackers } => Reply::DeclareAttackers(
                attackers
                    .into_iter()
                    .map(|a| (a.attacker.into(), a.defender.into()))
                    .collect(),
            ),
            WireReply::DeclareBlockers { blocks } => Reply::DeclareBlockers(
                blocks
                    .into_iter()
                    .map(|b| (b.blocker.into(), b.attacker.into()))
                    .collect(),
            ),
            WireReply::AssignDamage { assignments } => Reply::AssignDamage(
                assignments
                    .into_iter()
                    .map(|a| {
                        (
                            a.source.into(),
                            a.splits
                                .into_iter()
                                .map(|s| (s.recipient.into(), s.amount))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        }
    }
}
