use crate::types::ids::{CommandId, EventId};

/// Carried across one pipeline run — from `PendingAction`/`ResolveStackTop`
/// through to `CommitEvents` — so committed events keep correct causal
/// references and trigger collection knows where in the ledger this run
/// started. Without this, `CollectTriggers`'s `since` had no real source
/// except a hardcoded `0`, which is wrong for every action after the first
/// in a game.
pub struct PipelineContext {
    pub since_ledger: usize,
    pub caused_by_command: Option<CommandId>,
    pub parent_event: Option<EventId>,
}

/// Which `ProcessState` to return to once the current pipeline run's SBA/
/// trigger machinery settles. Decided at the point priority passes resolve
/// (all players passed in a row) or a command commits, then consumed once
/// the trigger-placement stage finishes.
pub enum PipelineExit {
    BackToPriority,
    ResolveStackTop,
    AdvanceTurnStep,
}
