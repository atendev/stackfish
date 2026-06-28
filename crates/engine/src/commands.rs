use crate::types::command::EngineCommand;
use crate::types::game::GameState;

/// Carrying out an already-validated `EngineCommand` failed partway —
/// distinct from `ActionError` (engine.rs), which rejects an action before
/// it's even accepted. Reaching a `CommandError` mid-execution should be
/// rare/exceptional once cost/target validation happens up front; this
/// exists mainly for cases validation can't fully rule out ahead of time.
pub enum CommandError {
    CostNotPaid,
    InvalidTarget,
    IllegalAction(String),
}

/// Executes one already-accepted `EngineCommand` against `state`, mutating
/// it and appending whatever `GameEvent`s resulted to `state.ledger`. Per
/// the rationale in `command.rs`: this does not decompose `command` into
/// further sub-commands — the sub-steps of carrying it out are events, not
/// commands. Returns nothing on success because the ledger append is the
/// observable result; callers read `state.ledger` for what happened.
pub fn execute_command(state: &mut GameState, command: EngineCommand) -> Result<(), CommandError> {
    todo!()
}
