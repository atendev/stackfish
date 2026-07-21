use stackfish_engine::engine::{Engine, StepStatus};

/// Runs `engine.step()` until it stops making progress (a priority window
/// opens, the game ends, ...) and returns that terminal `StepStatus`.
/// Identical copy-pasted into ~100 test files before this crate existed.
pub fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}
