use std::collections::HashMap;

use stackfish_engine::*;

use crate::schema::*;
use crate::schema::TargetSpec;
use crate::build::parse_mana_unit;

pub(crate) fn resolve_target(
    spec: &TargetSpec,
    card_ids: &HashMap<String, ObjectId>,
) -> Target {
    match spec {
        TargetSpec::Player(id) => Target::Player(PlayerId(*id)),
        TargetSpec::Card(key) => Target::Object(
            *card_ids
                .get(key)
                .unwrap_or_else(|| panic!("action references unknown card key '{key}'")),
        ),
    }
}

pub(crate) fn resolve_card(key: &str, card_ids: &HashMap<String, ObjectId>) -> ObjectId {
    *card_ids
        .get(key)
        .unwrap_or_else(|| panic!("action references unknown card key '{key}'"))
}

pub(crate) fn to_player_action(spec: &ActionSpec, card_ids: &HashMap<String, ObjectId>) -> PlayerAction {
    let (player, kind) = match spec {
        ActionSpec::PassPriority { player } => (*player, PlayerActionKind::PassPriority),
        ActionSpec::PlayLand { player, card } => {
            (*player, PlayerActionKind::PlayLand { card: resolve_card(card, card_ids) })
        }
        ActionSpec::CastSpell { player, card, targets, modes, x_value, auto_pay: _ } => (
            *player,
            PlayerActionKind::CastSpell {
                card: resolve_card(card, card_ids),
                modes: modes.clone(),
                targets: targets.iter().map(|t| resolve_target(t, card_ids)).collect(),
                x_value: *x_value,
                // TODO: a TOML field for the announced alternative cost
                alternative_cost: None,
                // TODO: a TOML field for paying buyback
                pay_buyback: false,
            },
        ),
        ActionSpec::ActivateAbility { player, source, ability_index, targets, chosen_colors, x_value, auto_pay: _ } => (
            *player,
            PlayerActionKind::ActivateAbility {
                source: resolve_card(source, card_ids),
                ability_index: *ability_index,
                targets: targets.iter().map(|t| resolve_target(t, card_ids)).collect(),
                chosen_colors: chosen_colors
                    .iter()
                    .map(|s| stackfish_engine::types::mana::parse_color_letter(s).expect("invalid chosen color letter in scenario TOML"))
                    .collect(),
                x_value: *x_value,
            },
        ),
        ActionSpec::TapForMana { player, source } => (
            *player,
            PlayerActionKind::ActivateAbility {
                source: resolve_card(source, card_ids),
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        ),
        ActionSpec::DeclareAttackers { player, attackers } => (
            *player,
            PlayerActionKind::DeclareAttackers {
                attackers: attackers
                    .iter()
                    .map(|a| (resolve_card(&a.attacker, card_ids), resolve_target(&a.target, card_ids)))
                    .collect(),
            },
        ),
        ActionSpec::DeclareBands { player, bands } => (
            *player,
            PlayerActionKind::DeclareBands {
                bands: bands
                    .iter()
                    .map(|band| band.iter().map(|c| resolve_card(c, card_ids)).collect())
                    .collect(),
            },
        ),
        ActionSpec::DeclareBlockers { player, blocks } => (
            *player,
            PlayerActionKind::DeclareBlockers {
                blocks: blocks
                    .iter()
                    .map(|b| (resolve_card(&b.blocker, card_ids), resolve_card(&b.attacker, card_ids)))
                    .collect(),
            },
        ),
        ActionSpec::AssignCombatDamage { player, assignments } => (
            *player,
            PlayerActionKind::AssignCombatDamage {
                assignments: assignments
                    .iter()
                    .map(|a| {
                        let splits = a
                            .splits
                            .iter()
                            .map(|s| (resolve_card(&s.blocker, card_ids), s.amount))
                            .collect();
                        (resolve_card(&a.attacker, card_ids), splits)
                    })
                    .collect(),
            },
        ),
        ActionSpec::MakeChoice { player, description, selection } => (
            *player,
            PlayerActionKind::MakeChoice {
                description: description.clone(),
                selection: selection.iter().map(|t| resolve_target(t, card_ids)).collect(),
            },
        ),
        // `run_action_spec` intercepts these and calls their own engine entry points.
        ActionSpec::SubmitPayment { .. } => {
            unreachable!("SubmitPayment is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::UnwindPayment { .. } => {
            unreachable!("UnwindPayment is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::SubmitDiscard { .. } => {
            unreachable!("SubmitDiscard is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::KeepHand { .. } | ActionSpec::TakeMulligan { .. } | ActionSpec::BottomCards { .. } => {
            unreachable!("pregame actions are handled in run_action_spec, before to_player_action")
        }
        ActionSpec::SubmitScriptChoice { .. } => {
            unreachable!("SubmitScriptChoice is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::AdvanceToPhase { .. } | ActionSpec::AdvanceToStep { .. } => {
            unreachable!("AdvanceToPhase/AdvanceToStep are handled in run_action_spec, before to_player_action")
        }
    };
    PlayerAction { player: PlayerId(player), kind }
}

// Empty units give the empty plan a `{0}` cost needs; anything else is the single `Mana`
// entry that `PaymentPlan::validate` expects.
// CR 118.5a
pub(crate) fn build_payment_plan(mana: &ManaPaymentSpec) -> PaymentPlan {
    if mana.units.is_empty() {
        PaymentPlan { payments: Vec::new() }
    } else {
        let units = mana.units.iter().map(|s| parse_mana_unit(s)).collect();
        // TODO: Phyrexian life in a fixture payment; `phyrexian_life` is always 0
        PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] }
    }
}

