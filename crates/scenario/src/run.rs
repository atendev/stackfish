use std::panic::{self, AssertUnwindSafe};

use stackfish_engine::*;
use stackfish_engine::engine::{Engine, StepStatus};


pub enum StepOutcome {
    Ran,
    NotImplemented(String),
    /// The engine refused the action or hit `StepStatus::Error`. Unlike `NotImplemented`, real
    /// code ran and turned the action down.
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

/// Steps `engine` until it pauses for a decision, errors or ends the game.
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

/// Drives one action through `engine`. A `todo!()` panic becomes `NotImplemented` and does
/// not abort the run.
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

// A two-player turn takes about 20 passes, so this covers dozens of turns. It makes an
// unreachable target fail instead of hanging the run.
const MAX_ADVANCE_PASSES: u32 = 2000;

/// Passes priority for whoever holds it until `reached` is true. It does not reuse `drain()`,
/// which treats every pause as `Ran`; any pause other than a priority window stops the loop
/// and is reported, not answered.
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

// Bottoms the first `first_n` cards of the hand by position, since `new_game` library cards
// have no TOML keys to name.
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

