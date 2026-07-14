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
    /// CR 514.3: cleanup's own exit, reached once `CollectTriggers`/
    /// `PutTriggersApnap` finish placing whatever a cleanup-step SBA pass
    /// raised. Unlike the other three variants (a fixed destination), the
    /// actual next `ProcessState` depends on whether anything happened:
    /// `since_ledger` (the ledger length before this pass's events were
    /// appended) is compared against the ledger's length once triggers have
    /// been placed — if it grew (an SBA event committed, or a triggered
    /// ability got pushed onto the stack), CR 514.3 says open a priority
    /// window; if not, the turn advances silently instead. See
    /// `Engine::end_pipeline_run`'s own match arm.
    CleanupCheck { since_ledger: usize },
    /// CR 514.1/514.3: `Engine::submit_discard`'s own exit — unlike
    /// `CleanupCheck` above, this is unconditional: the cleanup-discard
    /// turn-based action always falls through to a fresh SBA check
    /// afterward regardless of whether anything triggered off it, same
    /// sequencing `begin_cleanup_step` itself already follows for the
    /// discard-then-SBA-check order.
    CleanupSbaCheck,
}
