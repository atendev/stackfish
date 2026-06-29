use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::PlayerId;

/// Carrying out an already-validated `EngineCommand` failed partway —
/// distinct from `ActionError` (engine.rs), which rejects an action before
/// it's even accepted. Reaching a `CommandError` mid-execution should be
/// rare/exceptional once cost/target validation happens up front; this
/// exists mainly for cases validation can't fully rule out ahead of time.
#[derive(Debug)]
pub enum CommandError {
    CostNotPaid,
    InvalidTarget,
    IllegalAction(String),
}

/// Whether/how priority should move as a result of one `EngineCommand`.
/// Returned explicitly by `execute_command` rather than the command
/// mutating `state.priority_player` itself, so `engine.rs` is the one
/// place that actually writes that field — same reasoning as
/// `CommandOutcome::broke_pass_chain` not mutating `consecutive_passes`
/// directly.
pub enum PriorityHandoff {
    /// Most variants: priority stays with whoever currently has it.
    Unchanged,
    /// `PassPriority` (CR 117.3c): priority moves to the next player in
    /// `state.turn_order`. Carries the resolved target player rather than
    /// engine.rs re-deriving "next" from `turn_order`, so `execute_command`
    /// (which already has `state` to hand) stays the single place that
    /// knows the rotation rule.
    To(PlayerId),
}

/// What carrying out one `EngineCommand` produced, for `engine.rs` to act
/// on. Returned explicitly rather than left for `engine.rs` to infer from
/// `state` afterward — see `broke_pass_chain`/`priority_handoff` below.
pub struct CommandOutcome {
    /// Proposed events, not yet final — `engine.rs` runs these through
    /// `ReplacementCheck` -> `CommitEvents` to allocate ids and append to
    /// `state.ledger`. See `execute_command`'s doc comment for why this
    /// function doesn't write the ledger itself.
    pub events: Vec<GameEventKind>,
    /// `false` only for `PassPriority`; `true` for every other variant.
    /// CR 117.3c: taking any action other than passing breaks a
    /// consecutive-pass count. `engine.rs` owns `state.consecutive_passes`
    /// entirely (increment on `false`, reset on `true`) and decides "did
    /// everyone just pass" from that — this flag is the one thing it needs
    /// from execution to do so, rather than relying on every match arm
    /// here remembering to mutate `consecutive_passes` itself correctly.
    pub broke_pass_chain: bool,
    /// See `PriorityHandoff`. `engine.rs` applies this to
    /// `state.priority_player` itself.
    pub priority_handoff: PriorityHandoff,
}

/// Executes one already-accepted `EngineCommand` against `state`, mutating
/// non-ledger state directly (zones, etc.) and returning a
/// `CommandOutcome`. Per the rationale in `command.rs`: this does not
/// decompose `command` into further sub-commands — the sub-steps of
/// carrying it out are events, not commands.
///
/// Does NOT append to `state.ledger` and does NOT allocate `EventId`s/
/// `SequenceNo`s — those, plus replacement/prevention checks (CR 614-616),
/// are engine.rs's job (`ReplacementCheck` -> `CommitEvents`) so there is
/// exactly one place that writes the ledger. Does NOT mutate
/// `state.consecutive_passes` or `state.priority_player` either — see
/// `CommandOutcome::broke_pass_chain`/`priority_handoff`.
pub fn execute_command(
    state: &mut GameState,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    match command.action {
        PlayerActionKind::PassPriority => {
            // CR 117.3c: priority moves to the next player in turn_order
            // after whoever just passed. Read-only against turn_order -
            // does not touch state.priority_player/consecutive_passes,
            // both of which engine.rs owns (see PriorityHandoff/
            // broke_pass_chain doc comments above).
            let current_idx = state
                .turn_order
                .iter()
                .position(|p| *p == command.actor)
                .ok_or_else(|| {
                    CommandError::IllegalAction("actor not in turn_order".to_string())
                })?;
            let next_idx = (current_idx + 1) % state.turn_order.len();
            return Ok(CommandOutcome {
                events: vec![GameEventKind::PriorityPassed { player: command.actor }],
                broke_pass_chain: false,
                priority_handoff: PriorityHandoff::To(state.turn_order[next_idx]),
            });
        }
        PlayerActionKind::MakeChoice { .. } => {
            // Real choice application is genuinely generic - "depends
            // entirely on what asked for the choice" (see the TODO block
            // below) - no fixed event shape exists yet, so this produces
            // none. Exists as a minimal non-PassPriority stub so
            // broke_pass_chain's reset wiring (CR 117.3c) has something
            // other than PassPriority to exercise in tests; not a real
            // implementation of MakeChoice itself.
            return Ok(CommandOutcome {
                events: Vec::new(),
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
            });
        }
        _ => {}
    }
    // TODO: match command.action (PlayerActionKind) and handle each variant:
    //   CastSpell: move card ObjectId from hand to stack zone, create a
    //     StackObject (needs the card's registered "cast" operation
    //     ScriptHandle - where that comes from for a plain card-as-spell,
    //     not just abilities, isn't settled yet), deduct cost (after
    //     SubmitPayment has separately validated/paid it).
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   ActivateAbility: similar, but source stays where it is (CR 602) -
    //     only the ability goes on the stack, look up the RegisteredAbility
    //     by source+ability_index in state.abilities.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   PlayLand: move card to battlefield directly, no stack involved
    //     (CR 305.1 - lands aren't spells). broke_pass_chain = true,
    //     priority_handoff = Unchanged.
    //   PassPriority is implemented above (the match arm before this
    //     comment block) - everything else here is still unimplemented.
    //   DeclareAttackers/DeclareBlockers: validate legality (CR 508/509)
    //     and record the declarations into combat state - GameState doesn't
    //     have a dedicated combat-state field yet, may need one.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   SubmitPayment: validate via PaymentPlan::validate, commit if valid.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   MakeChoice: generic - depends entirely on what asked for the
    //     choice. broke_pass_chain = true, priority_handoff = Unchanged.
    // Every branch ends in returning
    // Ok(CommandOutcome { events, broke_pass_chain, priority_handoff }).
    todo!()
}
