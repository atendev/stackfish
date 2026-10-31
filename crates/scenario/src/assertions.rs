use std::collections::HashMap;

use stackfish_engine::*;

use crate::actions::resolve_card;
use crate::build::{parse_counter_kind, parse_phase, parse_step, parse_zone_kind};
use crate::schema::*;

pub enum AssertionOutcome {
    Passed,
    Failed { expected: String, actual: String },
}

pub(crate) fn check_assertion(
    spec: &AssertionSpec,
    state: &GameState,
    card_ids: &HashMap<String, ObjectId>,
) -> AssertionOutcome {
    match spec {
        AssertionSpec::Life { player, equals } => {
            let pid = PlayerId(*player);
            let actual = state
                .players
                .iter()
                .find(|p| p.player_id == pid)
                .map(|p| p.life);
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::LedgerLen { equals } => {
            let actual = state.ledger.len();
            if actual == *equals {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: actual.to_string(),
                }
            }
        }
        AssertionSpec::ObjectZone { card, equals } => {
            let object_id = resolve_card(card, card_ids);
            let expected_kind = parse_zone_kind(equals);
            // The stack is not in `state.zones`, so fall back to `state.stack` for `"stack"`.
            let actual_kind = state
                .zones
                .iter()
                .find(|z| z.contents.contains(&object_id))
                .map(|z| z.kind)
                .or_else(|| {
                    state
                        .stack
                        .iter()
                        .any(|s| s.stack_id == object_id)
                        .then_some(ZoneKind::Stack)
                });
            if actual_kind == Some(expected_kind) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.clone(),
                    actual: format!("{actual_kind:?}"),
                }
            }
        }
        AssertionSpec::ManaPool {
            player,
            color,
            equals,
        } => {
            let pid = PlayerId(*player);
            let pool = state
                .players
                .iter()
                .find(|p| p.player_id == pid)
                .map(|p| &p.mana_pool);
            let actual = pool.map(|pool| match color.as_str() {
                "white" => pool.white,
                "blue" => pool.blue,
                "black" => pool.black,
                "red" => pool.red,
                "green" => pool.green,
                "colorless" => pool.colorless,
                other => panic!("unknown mana_pool color in scenario file: {other}"),
            });
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::Tapped { card, equals } => {
            let object_id = resolve_card(card, card_ids);
            let actual = state.object(object_id).map(|o| o.tapped);
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::Eliminated { player, equals } => {
            let pid = PlayerId(*player);
            let actual = state
                .players
                .iter()
                .find(|p| p.player_id == pid)
                .map(|p| p.eliminated);
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::Phase { equals } => {
            let expected = parse_phase(equals);
            if state.phase == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.clone(),
                    actual: format!("{:?}", state.phase),
                }
            }
        }
        AssertionSpec::Step { equals } => {
            let expected = parse_step(equals);
            if state.step == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.clone(),
                    actual: format!("{:?}", state.step),
                }
            }
        }
        AssertionSpec::ActivePlayer { equals } => {
            let expected = PlayerId(*equals);
            if state.active_player == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{:?}", state.active_player),
                }
            }
        }
        AssertionSpec::PriorityPlayer { equals } => {
            let expected = PlayerId(*equals);
            if state.priority_player == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{:?}", state.priority_player),
                }
            }
        }
        AssertionSpec::HandSize { player, equals } => {
            let pid = PlayerId(*player);
            let actual = state
                .zones
                .iter()
                .find(|z| z.kind == ZoneKind::Hand && z.owner == Some(pid))
                .map(|z| z.contents.len())
                .unwrap_or(0);
            if actual == *equals {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: actual.to_string(),
                }
            }
        }
        AssertionSpec::ObjectCounters { card, kind, equals } => {
            let object_id = resolve_card(card, card_ids);
            let counter_kind = parse_counter_kind(kind);
            let actual = state
                .object(object_id)
                .map(|o| o.counters.count(&counter_kind));
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::PlayerCounters {
            player,
            kind,
            equals,
        } => {
            let pid = PlayerId(*player);
            let counter_kind = parse_counter_kind(kind);
            let actual = state
                .players
                .iter()
                .find(|p| p.player_id == pid)
                .map(|p| p.counters.count(&counter_kind));
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::CommanderDamage {
            player,
            source_card,
            equals,
        } => {
            let pid = PlayerId(*player);
            // The lookup key is the commander's name, read off its live object. `card_ids` is
            // remapped only after pregame, so a recast mid-scenario cannot be followed.
            // CR 903.9a
            let source_id = resolve_card(source_card, card_ids);
            let source_name = state.object(source_id).map(|o| o.definition.name.clone());
            let actual = source_name.and_then(|name| {
                state
                    .players
                    .iter()
                    .find(|p| p.player_id == pid)
                    .map(|p| p.commander_damage_from(&name))
            });
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::BandMembers { card, equals } => {
            let object_id = resolve_card(card, card_ids);
            let actual_band: Option<Vec<ObjectId>> = state
                .combat
                .as_ref()
                .and_then(|c| c.bands.iter().find(|b| b.contains(&object_id)))
                .map(|b| {
                    let mut sorted = b.clone();
                    sorted.sort();
                    sorted
                });
            let matches = if equals.is_empty() {
                actual_band.is_none()
            } else {
                let mut expected_ids: Vec<ObjectId> =
                    equals.iter().map(|k| resolve_card(k, card_ids)).collect();
                expected_ids.sort();
                actual_band.as_ref() == Some(&expected_ids)
            };
            if matches {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: format!("{equals:?}"),
                    actual: format!("{actual_band:?}"),
                }
            }
        }
    }
}
