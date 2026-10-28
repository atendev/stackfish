//! `Engine::open_decision` and `Engine::answer`: the checks on an Answer and the
//! routing of it to the engine's `submit_*` methods. See `DESIGN.md` for the
//! rest.

use std::fmt::Debug;

use crate::engine::Engine;
use crate::engine::{ActionError, CascadeChoiceError, MadnessChoiceError};
use crate::triggers::PendingTrigger;
use crate::types::command::{PlayerAction, PlayerActionKind};
use crate::types::ids::{ObjectId, PlayerId, Target};

use super::check;
use super::open::{self, Described, Route};
use super::{Answer, Candidate, Decision, Rejection, RejectionCode, Reply, ReplyShape};

impl Engine {
    /// The Decision the engine is waiting on, if any. Calling this twice
    /// without an accepted Answer in between returns the same id.
    pub fn open_decision(&mut self) -> Option<Decision> {
        let described = open::describe(self)?;
        let id = self.decisions.open_id();
        Some(Decision { id, player: described.player, mechanic: described.mechanic, shape: described.shape })
    }

    /// Routes `answer` from `player` to the engine entry point for the open
    /// Decision. On success the engine still has to be stepped to run on to
    /// its next pause. On rejection nothing changes and the Decision stays
    /// open.
    ///
    /// Checks run in this order: a Decision is open, the id matches, the player
    /// matches, the reply variant fits the shape, and the choice is legal.
    /// The engine's own validation covers the rest without touching the pause.
    pub fn answer(&mut self, player: PlayerId, answer: Answer) -> Result<(), Rejection> {
        let Some(described) = open::describe(self) else {
            return Err(Rejection::new(RejectionCode::NotOpen, "no Decision is open"));
        };
        let open_id = self.decisions.open_id();
        if answer.decision != open_id {
            return Err(Rejection::new(
                RejectionCode::StaleDecision,
                format!("Decision {} is not the open Decision, which is {}", answer.decision.0, open_id.0),
            ));
        }
        if player != described.player {
            return Err(Rejection::new(
                RejectionCode::WrongSeat,
                format!("Decision {} is for player {}, not player {}", open_id.0, described.player.0, player.0),
            ));
        }
        if !shape_matches(&described.shape, &answer.reply) {
            return Err(Rejection::new(
                RejectionCode::WrongAnswerShape,
                format!("Decision {} expects a different kind of reply", open_id.0),
            ));
        }
        self.route(player, described, answer.reply)?;
        self.decisions.close();
        Ok(())
    }

    fn route(&mut self, player: PlayerId, described: Described, reply: Reply) -> Result<(), Rejection> {
        let Described { shape, route, .. } = described;
        match (route, reply) {
            (Route::Priority, Reply::Action(action)) => {
                if action.player != player {
                    return Err(Rejection::new(RejectionCode::WrongSeat, "the action names a different player"));
                }
                let ReplyShape::Action { entries } = &shape else {
                    unreachable!("route and shape come from the same pause");
                };
                check::check_action(entries, &action.kind)?;
                self.submit_action(action).map_err(|e| match e {
                    ActionError::NotYourTurn => Rejection::new(RejectionCode::WrongSeat, "it is not your priority"),
                    other => invalid(other),
                })
            }
            (Route::DeclareAttackers, Reply::DeclareAttackers(attackers)) => {
                let ReplyShape::DeclareAttackers { attackers: entries } = &shape else {
                    unreachable!("route and shape come from the same pause");
                };
                check::check_attackers(entries, &attackers)?;
                self.submit_action(PlayerAction::new(player, PlayerActionKind::DeclareAttackers { attackers }))
                    .map_err(invalid)
            }
            (Route::DeclareBlockers, Reply::DeclareBlockers(blocks)) => {
                let ReplyShape::DeclareBlockers { blockers } = &shape else {
                    unreachable!("route and shape come from the same pause");
                };
                check::check_blockers(blockers, &blocks)?;
                self.submit_action(PlayerAction::new(player, PlayerActionKind::DeclareBlockers { blocks }))
                    .map_err(invalid)
            }
            (Route::AssignDamage, Reply::AssignDamage(assignments)) => {
                let ReplyShape::AssignDamage { splits } = &shape else {
                    unreachable!("route and shape come from the same pause");
                };
                check::check_splits(splits, &assignments)?;
                self.submit_action(PlayerAction::new(player, PlayerActionKind::AssignCombatDamage { assignments }))
                    .map_err(invalid)
            }

            (Route::Mulligan, Reply::YesNo(take_mulligan)) => {
                let kind = if take_mulligan { PlayerActionKind::TakeMulligan } else { PlayerActionKind::KeepHand };
                self.submit_mulligan_decision(PlayerAction::new(player, kind)).map_err(invalid)
            }
            (Route::BottomCards, Reply::Pick(chosen)) => {
                let cards = pick_objects(&shape, &chosen)?;
                self.submit_bottom_cards(PlayerAction::new(player, PlayerActionKind::BottomCards { cards }))
                    .map_err(invalid)
            }
            (Route::CleanupDiscard, Reply::Pick(chosen)) => {
                let cards = pick_objects(&shape, &chosen)?;
                self.submit_discard(cards).map_err(invalid)
            }

            (Route::Payment, Reply::Payment(Some(plan))) => self.submit_payment(plan).map_err(invalid),
            (Route::Payment, Reply::Payment(None)) => self.unwind_payment().map_err(invalid),
            (Route::UnlessPaid, Reply::Payment(Some(plan))) => {
                self.submit_unless_paid_payment(player, plan).map_err(invalid)
            }
            (Route::UnlessPaid, Reply::Payment(None)) => self.decline_unless_paid(player).map_err(invalid),

            (Route::Madness, Reply::MayCast(None)) => self.decline_madness_cast().map_err(invalid),
            (Route::Madness, Reply::MayCast(Some(cast))) => {
                self.submit_madness_cast(cast.modes, cast.targets, cast.x_value).map_err(madness_error)
            }
            (Route::Cascade, Reply::MayCast(None)) => self.decline_cascade_cast().map_err(invalid),
            (Route::Cascade, Reply::MayCast(Some(cast))) => {
                self.submit_cascade_cast(cast.modes, cast.targets, cast.x_value).map_err(cascade_error)
            }

            (Route::CommanderReturn, Reply::YesNo(returned)) => {
                self.submit_commander_return_choice(returned).map_err(invalid)
            }
            (Route::ScriptMay, Reply::YesNo(chosen)) => self.submit_script_may_choice(chosen).map_err(invalid),

            (Route::SpellMode, Reply::Modes(modes)) => self.submit_mode_choice(modes).map_err(invalid),
            (Route::TriggerMode, Reply::Modes(modes)) => self.submit_trigger_mode_choice(modes).map_err(invalid),

            (Route::Scry, Reply::Partition { top, rest }) => self.submit_scry_choice(top, rest).map_err(invalid),
            (Route::Surveil, Reply::Partition { top, rest }) => self.submit_surveil_choice(top, rest).map_err(invalid),

            (Route::TriggerOrder(group), Reply::Order(indices)) => {
                let order = order_from_indices(&group, &indices)?;
                self.submit_trigger_order(order).map_err(invalid)
            }
            (Route::TriggerTargets, Reply::Targets(per_word)) => {
                let ReplyShape::Targets { candidates, .. } = &shape else {
                    unreachable!("route and shape come from the same pause");
                };
                if per_word.len() != candidates.len() {
                    return Err(Rejection::new(
                        RejectionCode::InvalidChoice,
                        format!("expected {} target lists, got {}", candidates.len(), per_word.len()),
                    ));
                }
                self.submit_trigger_targets(per_word.into_iter().flatten().collect()).map_err(invalid)
            }

            (Route::ReplacementChoice, Reply::Pick(chosen)) => {
                check_pick(&shape, &chosen)?;
                let Candidate::Replacement(effect) = chosen[0] else {
                    return Err(not_a_candidate());
                };
                self.submit_replacement_choice(effect).map_err(invalid)
            }
            (Route::LegendRule, Reply::Pick(chosen)) => {
                let kept = pick_objects(&shape, &chosen)?[0];
                self.submit_legend_rule_choice(kept).map_err(invalid)
            }
            (Route::AttachChoice, Reply::Pick(chosen)) => {
                let aura_target = pick_objects(&shape, &chosen)?[0];
                self.submit_attach_choice(aura_target).map_err(invalid)
            }
            (Route::EntryAttachChoice, Reply::Pick(chosen)) => {
                let target = pick_targets(&shape, &chosen)?[0];
                self.submit_entry_attach_choice(target).map_err(invalid)
            }
            (Route::TokenAttachChoice, Reply::Pick(chosen)) => {
                let target = pick_targets(&shape, &chosen)?[0];
                self.submit_token_attach_choice(target).map_err(invalid)
            }
            (Route::ScriptSelect, Reply::Pick(chosen)) => {
                let target = pick_targets(&shape, &chosen)?[0];
                self.submit_script_choice(target).map_err(invalid)
            }
            (Route::DiscardChoice, Reply::Pick(chosen)) => {
                let card = pick_objects(&shape, &chosen)?[0];
                self.submit_discard_choice(player, card).map_err(invalid)
            }
            (Route::CopyChoice, Reply::Pick(chosen)) => {
                let copied = pick_objects(&shape, &chosen)?.first().copied();
                self.submit_copy_choice(copied).map_err(invalid)
            }
            (Route::Devour, Reply::Pick(chosen)) => {
                self.submit_devour_choice(pick_objects(&shape, &chosen)?).map_err(invalid)
            }
            (Route::Amplify, Reply::Pick(chosen)) => {
                self.submit_amplify_choice(pick_objects(&shape, &chosen)?).map_err(invalid)
            }
            (Route::SearchDevour, Reply::Pick(chosen)) => {
                self.submit_search_devour_choice(pick_objects(&shape, &chosen)?).map_err(invalid)
            }
            (Route::SearchAmplify, Reply::Pick(chosen)) => {
                self.submit_search_amplify_choice(pick_objects(&shape, &chosen)?).map_err(invalid)
            }
            (Route::Search, Reply::Pick(chosen)) => {
                self.submit_search_choice(pick_objects(&shape, &chosen)?).map_err(invalid)
            }

            // `shape_matches` already confirmed the reply variant fits the
            // shape, and each route has exactly one shape.
            _ => unreachable!("route and reply were checked against the same shape"),
        }
    }
}

fn shape_matches(shape: &ReplyShape, reply: &Reply) -> bool {
    matches!(
        (shape, reply),
        (ReplyShape::Pick { .. }, Reply::Pick(_))
            | (ReplyShape::YesNo, Reply::YesNo(_))
            | (ReplyShape::Order { .. }, Reply::Order(_))
            | (ReplyShape::Modes { .. }, Reply::Modes(_))
            | (ReplyShape::Partition { .. }, Reply::Partition { .. })
            | (ReplyShape::Payment { .. }, Reply::Payment(_))
            | (ReplyShape::Targets { .. }, Reply::Targets(_))
            | (ReplyShape::MayCast { .. }, Reply::MayCast(_))
            | (ReplyShape::Action { .. }, Reply::Action(_))
            | (ReplyShape::DeclareAttackers { .. }, Reply::DeclareAttackers(_))
            | (ReplyShape::DeclareBlockers { .. }, Reply::DeclareBlockers(_))
            | (ReplyShape::AssignDamage { .. }, Reply::AssignDamage(_))
    )
}

/// The detail stays on the engine (`Engine::fatal_error`) because this text can
/// reach a Seat.
fn internal() -> Rejection {
    Rejection::new(RejectionCode::Internal, "the engine hit an internal error")
}

fn madness_error(error: MadnessChoiceError) -> Rejection {
    match error {
        MadnessChoiceError::Internal => internal(),
        other => invalid(other),
    }
}

fn cascade_error(error: CascadeChoiceError) -> Rejection {
    match error {
        CascadeChoiceError::Internal => internal(),
        other => invalid(other),
    }
}

// Every typed engine error that gets past the checks in `answer` is an invalid
// choice. The `NotAwaiting*`, `NotCurrentPlayer` and `WrongPlayer` variants
// cannot occur because the open pause and the player were already confirmed.
fn invalid<E: Debug>(error: E) -> Rejection {
    Rejection::new(RejectionCode::InvalidChoice, format!("the engine rejected the answer: {error:?}"))
}

fn not_a_candidate() -> Rejection {
    Rejection::new(RejectionCode::InvalidChoice, "the reply names something that is not a candidate")
}

/// A Pick reply holds between `min` and `max` entries, each a listed
/// candidate, with no entry twice.
fn check_pick(shape: &ReplyShape, chosen: &[Candidate]) -> Result<(), Rejection> {
    let ReplyShape::Pick { candidates, min, max } = shape else {
        unreachable!("Pick routes always have a Pick shape");
    };
    if chosen.len() < *min || chosen.len() > *max {
        return Err(Rejection::new(
            RejectionCode::InvalidChoice,
            format!("expected between {min} and {max} choices, got {}", chosen.len()),
        ));
    }
    for (i, choice) in chosen.iter().enumerate() {
        if !candidates.contains(choice) {
            return Err(not_a_candidate());
        }
        if chosen[..i].contains(choice) {
            return Err(Rejection::new(RejectionCode::InvalidChoice, "the reply names the same candidate twice"));
        }
    }
    Ok(())
}

fn pick_targets(shape: &ReplyShape, chosen: &[Candidate]) -> Result<Vec<Target>, Rejection> {
    check_pick(shape, chosen)?;
    chosen
        .iter()
        .map(|c| match c {
            Candidate::Target(t) => Ok(*t),
            Candidate::Replacement(_) => Err(not_a_candidate()),
        })
        .collect()
}

fn pick_objects(shape: &ReplyShape, chosen: &[Candidate]) -> Result<Vec<ObjectId>, Rejection> {
    pick_targets(shape, chosen)?
        .into_iter()
        .map(|t| match t {
            Target::Object(id) => Ok(id),
            _ => Err(not_a_candidate()),
        })
        .collect()
}

/// `indices` must be a permutation of `0..group.len()`.
fn order_from_indices(group: &[PendingTrigger], indices: &[usize]) -> Result<Vec<PendingTrigger>, Rejection> {
    let bad = || Rejection::new(RejectionCode::InvalidChoice, "the order must use each index exactly once");
    if indices.len() != group.len() {
        return Err(bad());
    }
    let mut seen = vec![false; group.len()];
    let mut order = Vec::with_capacity(group.len());
    for &i in indices {
        if i >= group.len() || std::mem::replace(&mut seen[i], true) {
            return Err(bad());
        }
        order.push(group[i]);
    }
    Ok(order)
}
