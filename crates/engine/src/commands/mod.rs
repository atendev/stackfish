//! Carries out already-validated `EngineCommand`s against `GameState`.
//!
//! Split out of a single `commands.rs` into this directory for readability:
//! this file keeps the small error/outcome types plus `execute_command`
//! itself as a dispatcher; each `PlayerActionKind` arm's real body lives in
//! a sibling module - `legality` (sorcery-speed/legality predicates, target
//! validation), `attach` (ability lookup, retirement, Aura/Equipment/
//! Fortification attach helpers), `zone_ops` (zone-move propose/commit,
//! cost payment, damage application), `land`/`cast`/`activate` (one arm
//! each), and `combat` (the four combat-related arms).

use crate::script::ScriptRuntime;
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::cost::CostSpec;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::mana::ManaUnit;

mod activate;
mod attach;
mod cast;
mod combat;
mod land;
mod legality;
#[cfg(test)]
mod tests;
mod zone_ops;

// Re-exports so `crate::commands::X` keeps resolving for every path
// `engine.rs` (and, for `propose_attach`/`AttachProposal`, an external
// integration test) already reaches through this module - see each
// submodule for the real definition and its own doc comment.
pub(crate) use attach::{
    apply_copy_additional_counters, attach_permanent, attach_permanent_to_player, find_ability,
    find_ability_by_id, grant_linked_copy_ability, is_aura, is_equipment, legal_entry_attach_candidates,
    next_active_player, non_eliminated_players, purge_granted_abilities, remap_abilities, retire_object,
};
pub use attach::{propose_attach, AttachProposal};
pub(crate) use combat::{execute_assign_combat_damage, mark_effect_blocked};
pub(crate) use legality::{
    attachment_target_is_legal, combine_chosen_modes, effective_keywords, is_modal_resolvable,
    legal_target_candidates_for_spec, split_targets_by_requirement, target_is_legal, target_spec_minimums,
    validate_targets,
};
pub(crate) use zone_ops::{
    apply_damage_event, collect_cost_zone_moves, commit_payment, commit_zone_move, propose_draw, propose_zone_move,
};

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
    /// CR 601.2a puts a spell on the stack *before* its cost is determined/
    /// paid (601.2f-h) — set by `CastSpell` once it's pushed `stack_id` onto
    /// `state.stack`, so `engine.rs` can pause the rest of this pipeline run
    /// on `ProcessState::AwaitingPayment` until `Engine::submit_payment`
    /// answers, rather than this function deducting mana synchronously
    /// (which would assume the cost is always payable/free, and skip the
    /// CR 118.5a "must explicitly acknowledge a {0} cost" requirement).
    /// `None` for every other variant — nothing else here puts something on
    /// the stack pending payment.
    pub requires_payment: Option<PendingPaymentRequest>,
    /// Object ids that just became new battlefield permanents this command
    /// (CR 400.7 mints a fresh id per zone change) — `engine.rs` runs each
    /// one's card script (`ScriptRuntime::run_card_script`) right after this
    /// outcome is applied. Empty for every variant except `PlayLand`, since
    /// `execute_command` (`state: &mut GameState` only) can't reach
    /// `ScriptRuntime` itself — same "one place decides, one place performs
    /// the side effect" split `priority_handoff`/`broke_pass_chain` already
    /// use. `ResolveStackTop`'s permanent-entry branch lives directly in
    /// `engine.rs`, so it calls `run_card_script` itself without needing
    /// this field.
    pub battlefield_entries: Vec<ObjectId>,
}

/// See `CommandOutcome::requires_payment`'s doc comment for why this exists
/// as its own pause rather than a synchronous deduction.
pub struct PendingPaymentRequest {
    pub stack_id: ObjectId,
    pub payer: PlayerId,
    pub cost: CostSpec,
    /// The object whose activation/cast this cost belongs to —
    /// `AdditionalCost::TapSelf` validates against this specifically (CR
    /// 602.5a: an activated ability's own `{T}` always taps *this* object,
    /// never a filter choice). For `CastSpell`, this is the spell's own
    /// `stack_id` — harmless as a placeholder since a spell's `CostSpec`
    /// never contains `TapSelf`.
    pub source: ObjectId,
    /// What to do once this cost fully commits — see `PaymentResolution`'s
    /// own doc comment. Almost always `StackObject` (a real `StackObject`
    /// is already sitting on `state.stack` at `stack_id`, waiting for normal
    /// stack resolution) — `ManaAbility` is the one exception (CR
    /// 605.1a/605.3b: a mana ability with a real cost still needs an
    /// explicit payment step, same as any other ability, but never reaches
    /// the stack at all).
    pub on_paid: PaymentResolution,
}

/// See `PendingPaymentRequest::on_paid`'s doc comment. `stack_id` on the
/// enclosing `PendingPaymentRequest` is only a real, resolvable `StackObject`
/// id for the `StackObject` case — for `ManaAbility` it's an opaque handle
/// (freshly allocated, CR 400.7-style) that nothing on `state.stack` will
/// ever match, since a mana ability with a real cost still pauses for
/// payment exactly like any other ability, but nothing is ever pushed onto
/// the stack for it to resolve later.
pub enum PaymentResolution {
    StackObject,
    /// CR 605's mana units to add once this cost commits — already fully
    /// resolved (a fixed `AddMana` list, or an `AddManaChoice`'s
    /// `chosen_colors` already validated and converted) at command-execution
    /// time, same as the free (no-cost) mana-ability path already did before
    /// this existed; paying a cost doesn't change *which* mana comes out.
    ManaAbility { controller: PlayerId, units: Vec<ManaUnit> },
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
    script_runtime: &ScriptRuntime,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    match command.action {
        PlayerActionKind::PassPriority => {
            return land::execute_pass_priority(state, command.actor);
        }
        PlayerActionKind::MakeChoice { .. } => {
            return land::execute_make_choice();
        }
        PlayerActionKind::PlayLand { card } => {
            return land::execute_play_land(state, script_runtime, command.actor, card);
        }
        PlayerActionKind::CastSpell { card, modes, targets, x_value, alternative_cost, pay_buyback } => {
            return cast::execute_cast_spell(
                state,
                script_runtime,
                command.actor,
                card,
                modes,
                targets,
                x_value,
                alternative_cost,
                pay_buyback,
            );
        }
        PlayerActionKind::ActivateAbility { source, ability_index, targets, chosen_colors, x_value } => {
            return activate::execute_activate_ability(
                state,
                script_runtime,
                command.actor,
                source,
                ability_index,
                targets,
                chosen_colors,
                x_value,
            );
        }
        PlayerActionKind::DeclareAttackers { attackers } => {
            return combat::execute_declare_attackers(state, script_runtime, command.actor, attackers);
        }
        PlayerActionKind::DeclareBands { bands } => {
            return combat::execute_declare_bands(state, script_runtime, command.actor, bands);
        }
        PlayerActionKind::DeclareBlockers { blocks } => {
            return combat::execute_declare_blockers(state, script_runtime, command.actor, blocks);
        }
        PlayerActionKind::AssignCombatDamage { assignments } => {
            return combat::execute_assign_combat_damage(state, script_runtime, command.actor, assignments);
        }
        _ => {}
    }
    // Every remaining `PlayerActionKind` variant reaching this point is a
    // should-never-happen case, not a "not implemented yet" one - every
    // variant `submit_action` actually lets through has its own explicit arm
    // above (CastSpell, ActivateAbility, PlayLand, PassPriority, MakeChoice,
    // DeclareAttackers, DeclareBands, DeclareBlockers, AssignCombatDamage):
    //   SubmitPayment never reaches here at all - Engine::submit_payment
    //     takes a bare PaymentPlan, not a PlayerActionKind::SubmitPayment
    //     wrapped in an EngineCommand (see CommandOutcome::requires_payment's
    //     doc comment above for how a payment pause is actually answered).
    //   KeepHand/TakeMulligan/BottomCards are rejected by submit_action
    //     itself before an EngineCommand is ever built - see its own
    //     "pregame actions use submit_mulligan_decision / submit_bottom_cards"
    //     error.
    todo!()
}
