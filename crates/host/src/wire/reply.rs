use serde::Deserialize;
use stackfish_engine::decision::{CastAnnouncement, Reply};
use stackfish_engine::{CostPayment, ObjectId, PaymentPlan};

use super::mana::WireManaUnit;
use super::{U64, WireCandidate, WireColor, WireTarget};

/// The body of an `answer` request: the Decision it answers and the reply.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnswerBody {
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
pub(crate) enum WireAction {
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
        modes: Vec<u32>,
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
                modes,
                targets,
                chosen_colors,
                x_value,
            } => Kind::ActivateAbility {
                source: source.into(),
                ability_index,
                modes,
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
pub(crate) struct WireAttack {
    pub attacker: U64,
    pub defender: WireTarget,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireBlock {
    pub blocker: U64,
    pub attacker: U64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireDamage {
    pub recipient: U64,
    pub amount: u32,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireAssignment {
    pub source: U64,
    pub splits: Vec<WireDamage>,
}

/// An Answer's reply. Its `kind` must match the open Decision's `shape`.
/// `order` holds indices into the Decision's `items`.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WireReply {
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
    /// One list of attacker ids per band.
    DeclareBands {
        bands: Vec<Vec<U64>>,
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
    pub(crate) fn into_reply(self, player: stackfish_engine::PlayerId) -> Reply {
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
            WireReply::DeclareBands { bands } => {
                Reply::DeclareBands(bands.into_iter().map(objects).collect())
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use stackfish_engine::PlayerId;

    // CR 508.1e
    #[test]
    fn a_band_reply_becomes_one_id_list_per_band() {
        let wire: WireReply = serde_json::from_value(serde_json::json!({
            "kind": "declare_bands",
            "bands": [["1", "2"], ["4", "5"]],
        }))
        .unwrap();
        let Reply::DeclareBands(bands) = wire.into_reply(PlayerId(0)) else {
            panic!("expected a DeclareBands reply");
        };
        assert_eq!(
            bands,
            vec![
                vec![ObjectId(1), ObjectId(2)],
                vec![ObjectId(4), ObjectId(5)]
            ]
        );
    }

    // CR 602.2b, 700.2a
    #[test]
    fn an_activate_reply_carries_its_modes_and_they_default_to_none() {
        let action = |extra: serde_json::Value| {
            let mut action = serde_json::json!({
                "kind": "activate_ability",
                "source": "3",
                "ability_index": 1,
                "x_value": null,
            });
            action
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let wire: WireReply =
                serde_json::from_value(serde_json::json!({ "kind": "action", "action": action }))
                    .unwrap();
            let Reply::Action(action) = wire.into_reply(PlayerId(0)) else {
                panic!("expected an Action reply");
            };
            let stackfish_engine::PlayerActionKind::ActivateAbility { modes, .. } = action.kind
            else {
                panic!("expected an activation");
            };
            modes
        };
        assert_eq!(action(serde_json::json!({ "modes": [1, 0] })), vec![1, 0]);
        assert!(action(serde_json::json!({})).is_empty());
    }
}
