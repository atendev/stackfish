use crate::types::ids::{CommandId, EventId};

/// State carried through one pipeline run so committed events keep their
/// causal references and trigger collection knows where the run began.
#[derive(Debug)]
pub struct PipelineContext {
    pub since_ledger: usize,
    pub caused_by_command: Option<CommandId>,
    pub parent_event: Option<EventId>,
}

/// Where the pipeline goes once the current run's SBA and trigger stages settle.
#[derive(Debug)]
pub enum PipelineExit {
    BackToPriority,
    ResolveStackTop,
    AdvanceTurnStep,
    /// CR 514.3: priority opens only if the ledger grew past `since_ledger`
    /// during the pass; otherwise the turn advances silently.
    CleanupCheck { since_ledger: usize },
    /// CR 514.1: always falls through to a fresh SBA check after the cleanup discard.
    CleanupSbaCheck,
}
