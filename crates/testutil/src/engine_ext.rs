use stackfish_engine::engine::{Engine, StepStatus};

/// Runs `engine.step()` until it stops making progress and returns the terminal `StepStatus`.
pub fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}
