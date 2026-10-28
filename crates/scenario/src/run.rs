use std::panic::{self, AssertUnwindSafe};

use stackfish_engine::*;
use stackfish_engine::engine::{Engine, StepStatus};


pub enum StepOutcome {
    Ran,
    NotImplemented(String),
    /// The engine refused the action (`submit_action` returned an `Err`, with
    /// nothing changed) or broke an invariant while carrying it out
    /// (`StepStatus::Error`). Distinct from a panic (`NotImplemented`): real code
    /// ran and turned the action down.
    Errored(String),
}

pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Steps `engine` until it pauses for the next player decision (or errors/
/// ends the game) - shared by `run_action` (after `submit_action`) and
/// `run_payment` (after `submit_payment`), since both resume the same
/// pipeline-stepping loop once their respective submission succeeds.
pub(crate) fn drain(engine: &mut Engine) -> StepOutcome {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            StepStatus::AwaitingAction { .. }
            | StepStatus::AwaitingReplacementChoice { .. }
            | StepStatus::AwaitingLegendRuleChoice { .. }
            | StepStatus::AwaitingCommanderReturnChoice { .. }
            | StepStatus::AwaitingAttachChoice { .. }
            | StepStatus::AwaitingEntryAttachChoice { .. }
            | StepStatus::AwaitingTokenAttachChoice { .. }
            | StepStatus::AwaitingDevourChoice { .. }
            | StepStatus::AwaitingAmplifyChoice { .. }
            | StepStatus::AwaitingMadnessChoice { .. }
            | StepStatus::AwaitingCascadeChoice { .. }
            | StepStatus::AwaitingUnlessPaidChoice { .. }
            | StepStatus::AwaitingDiscardChoice { .. }
            | StepStatus::AwaitingSearchDevourChoice { .. }
            | StepStatus::AwaitingSearchAmplifyChoice { .. }
            | StepStatus::AwaitingCopyChoice { .. }
            | StepStatus::AwaitingScriptChoice { .. }
            | StepStatus::AwaitingScryChoice { .. }
            | StepStatus::AwaitingSurveilChoice { .. }
            | StepStatus::AwaitingSearchChoice { .. }
            | StepStatus::AwaitingTriggerOrderChoice { .. }
            | StepStatus::AwaitingTriggerTargets { .. }
            | StepStatus::AwaitingTriggerModeChoice { .. }
            | StepStatus::AwaitingPayment { .. }
            | StepStatus::AwaitingModeChoice { .. }
            | StepStatus::AwaitingDiscard { .. }
            | StepStatus::AwaitingMulliganDecision { .. }
            | StepStatus::AwaitingBottomCardChoice { .. }
            | StepStatus::GameOver { .. } => {
                return StepOutcome::Ran;
            }
            StepStatus::Error(err) => return StepOutcome::Errored(format!("{err:?}")),
        }
    }
}

/// Drives one action through `engine`, catching any `todo!()` panic
/// (expected until Phase 5/7) rather than letting it abort the whole run.
pub(crate) fn run_action(engine: &mut Engine, action: PlayerAction) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_action(action) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Generous but finite cap on `run_advance`'s own pass loop - a two-player
/// turn is on the order of 20 priority passes, so this covers dozens of full
/// turns. Exists purely so a target that's never reached (a typo, or a
/// scenario missing a setup step the target depends on) fails loudly instead
/// of hanging the run.
const MAX_ADVANCE_PASSES: u32 = 2000;

/// Shared driver for `AdvanceToPhase`/`AdvanceToStep`: repeatedly passes
/// priority for whichever player currently holds it (`state.priority_player`
/// - never the caller's choice, unlike every other action here) until
/// `reached` is true. Deliberately doesn't reuse `drain()` - that helper
/// collapses every pause kind into one `StepOutcome::Ran`, but this loop
/// needs to tell "still just passing priority" (`AwaitingAction`) apart from
/// everything else. If a pass ever lands on any other pause - a payment, a
/// discard, a script choice, `GameOver`, an engine error - it stops
/// immediately and reports that instead of guessing an answer, same "state
/// the limitation, don't fake it" stance `resolve_auto_pay` takes for a cost
/// shape it can't handle.
pub(crate) fn run_advance(engine: &mut Engine, reached: impl Fn(&GameState) -> bool, label: &str) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        for _ in 0..MAX_ADVANCE_PASSES {
            if reached(engine.state()) {
                return StepOutcome::Ran;
            }
            let player = engine.state().priority_player;
            if let Err(err) =
                engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority })
            {
                return StepOutcome::Errored(format!("{err:?}"));
            }
            loop {
                match engine.step().status {
                    StepStatus::Progressed => continue,
                    StepStatus::AwaitingAction { .. } => break,
                    other => {
                        return StepOutcome::Errored(format!(
                            "advance stopped before reaching {label}: engine is now waiting on \
                             {other:?}, not a plain priority window - pass_priority alone can't \
                             push through this, handle it explicitly first"
                        ));
                    }
                }
            }
        }
        StepOutcome::Errored(format!("advance did not reach {label} after {MAX_ADVANCE_PASSES} passes"))
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Same shape as `run_action`, but for the `SubmitPayment` action - answers
/// a `ProcessState::AwaitingPayment` pause via `Engine::submit_payment`
/// rather than `submit_action` (see `ActionSpec::SubmitPayment`'s doc
/// comment for why it can't go through `to_player_action`/`run_action`).
pub(crate) fn run_payment(engine: &mut Engine, plan: PaymentPlan) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_payment(plan) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Same shape as `run_payment`, but for the `UnwindPayment` action — answers
/// a `ProcessState::AwaitingPayment` pause via `Engine::unwind_payment`
/// instead of `Engine::submit_payment`.
pub(crate) fn run_unwind_payment(engine: &mut Engine) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.unwind_payment() {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Same shape as `run_payment`, but for the `SubmitScriptChoice` action —
/// answers a `ProcessState::AwaitingScriptChoice { request: SelectTarget,
/// .. }` pause via `Engine::submit_script_choice`. Generic over whatever
/// `TargetFilter` the pending request carries - works for a bare
/// `Game.SelectTargets` ask or any iteration of a `Game.ChooseN` loop alike,
/// since both raise the identical pause shape and `submit_script_choice`
/// itself doesn't distinguish them.
pub(crate) fn run_script_choice(engine: &mut Engine, target: Target) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_script_choice(target) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Same shape as `run_payment`, but for the `SubmitDiscard` action — answers
/// a `ProcessState::AwaitingDiscard` pause via `Engine::submit_discard`.
pub(crate) fn run_discard(engine: &mut Engine, cards: Vec<ObjectId>) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_discard(cards) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Answers a `ProcessState::AwaitingMulliganDecision` pause via
/// `Engine::submit_mulligan_decision`.
pub(crate) fn run_mulligan(engine: &mut Engine, action: PlayerAction) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_mulligan_decision(action) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Answers a `ProcessState::AwaitingBottomCardChoice` pause via
/// `Engine::submit_bottom_cards`. Takes the first `first_n` cards from the
/// player's hand zone (in order) rather than asking the caller to name
/// specific `ObjectId`s — library cards created by `new_game` don't have
/// TOML keys, so positional selection is the practical choice here.
pub(crate) fn run_bottom_cards(engine: &mut Engine, player: PlayerId, first_n: usize) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        let cards: Vec<ObjectId> = engine
            .state()
            .zone(ZoneKind::Hand, Some(player))
            .map_or(Vec::new(), |z| z.contents.iter().copied().take(first_n).collect());
        let action = PlayerAction {
            player,
            kind: PlayerActionKind::BottomCards { cards },
        };
        if let Err(err) = engine.submit_bottom_cards(action) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

