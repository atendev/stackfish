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
    // TODO: match command.action (PlayerActionKind) and handle each variant:
    //   CastSpell: move card ObjectId from hand to stack zone, create a
    //     StackObject (needs the card's registered "cast" operation
    //     ScriptHandle - where that comes from for a plain card-as-spell,
    //     not just abilities, isn't settled yet), deduct cost (after
    //     SubmitPayment has separately validated/paid it).
    //   ActivateAbility: similar, but source stays where it is (CR 602) -
    //     only the ability goes on the stack, look up the RegisteredAbility
    //     by source+ability_index in state.abilities.
    //   PlayLand: move card to battlefield directly, no stack involved
    //     (CR 305.1 - lands aren't spells).
    //   PassPriority: append a PriorityPassed event; turn/phase advancement
    //     consequences belong in engine.rs's step(), not here.
    //   DeclareAttackers/DeclareBlockers: validate legality (CR 508/509)
    //     and record the declarations into combat state - GameState doesn't
    //     have a dedicated combat-state field yet, may need one.
    //   SubmitPayment: validate via PaymentPlan::validate, commit if valid.
    //   MakeChoice: generic - depends entirely on what asked for the choice.
    // Every branch ends in GameEvent::new(...) appended to state.ledger.
    todo!()
}
