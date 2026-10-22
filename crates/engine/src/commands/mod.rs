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
mod entries;
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
    find_ability_by_id, grant_linked_copy_ability, is_aura, is_equipment, legal_aura_entry_candidates, legal_entry_attach_candidates,
    next_active_player, non_eliminated_players, purge_granted_abilities, remap_abilities, retire_object,
};
pub use attach::{propose_attach, AttachProposal};
pub(crate) use combat::{
    attacker_entries, blocker_entries, damage_split_entries, execute_assign_combat_damage, mark_effect_blocked,
    next_blocking_player,
};
pub(crate) use entries::priority_entries;
pub(crate) use legality::{
    attachment_target_is_legal, combine_chosen_modes, effective_keywords, effective_power, is_ability_stack_entry, is_modal_placeholder_requirement, mode_can_join_others, solo_only_modes,
    legal_target_candidates_for_spec, split_targets_by_requirement, target_is_legal, target_spec_minimums,
    validate_targets,
};
pub(crate) use zone_ops::{
    apply_damage_event, collect_cost_zone_moves, commit_payment, commit_zone_move, propose_draw, propose_zone_move,
};

/// Why a command was turned down before it changed anything (CR 733.1,
/// 733.2). Only the validation half of a command builds one: every `validate_*`
/// function returns `Result<_, Rejected>`, and every `apply_*` function returns
/// `Result<_, Internal>`, so a mutating step cannot report "rejected" by type.
/// The player keeps priority and may try again.
#[derive(Debug, Clone, PartialEq)]
pub enum Rejected {
    InvalidTarget,
    IllegalAction(String),
}

/// A broken engine invariant found while carrying out a command that already
/// passed validation, for example a zone or object that validation just
/// confirmed is gone. Fatal: the state may be half-changed. The text is for the
/// host's logs and never goes on the wire (it can name a hidden card).
#[derive(Debug, Clone, PartialEq)]
pub struct Internal(pub String);

impl Internal {
    pub fn new(message: impl Into<String>) -> Self {
        Internal(message.into())
    }
}

/// What `execute_command` can fail with. `Rejected` leaves everything as it
/// was; `Internal` is fatal.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandError {
    Rejected(Rejected),
    Internal(Internal),
}

impl From<Rejected> for CommandError {
    fn from(rejected: Rejected) -> Self {
        CommandError::Rejected(rejected)
    }
}

impl From<Internal> for CommandError {
    fn from(internal: Internal) -> Self {
        CommandError::Internal(internal)
    }
}

/// Whether/how priority should move as a result of one `EngineCommand`.
/// Returned explicitly by `execute_command` rather than the command
/// mutating `state.priority_player` itself, so `engine.rs` is the one
/// place that actually writes that field — same reasoning as
/// `CommandOutcome::broke_pass_chain` not mutating `consecutive_passes`
/// directly.
#[derive(Debug)]
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
#[derive(Debug)]
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
#[derive(Debug)]
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
#[derive(Debug)]
pub enum PaymentResolution {
    StackObject,
    /// CR 605's mana units to add once this cost commits — already fully
    /// resolved (a fixed `AddMana` list, or an `AddManaChoice`'s
    /// `chosen_colors` already validated and converted) at command-execution
    /// time, same as the free (no-cost) mana-ability path already did before
    /// this existed; paying a cost doesn't change *which* mana comes out.
    ManaAbility { controller: PlayerId, units: Vec<ManaUnit> },
}

/// A command that passed validation, holding everything its apply step needs
/// so the apply step reads nothing that could still reject it.
#[derive(Debug)]
pub(crate) enum Validated {
    PassPriority { actor: PlayerId },
    MakeChoice,
    PlayLand(land::LandPlan),
    CastSpell(Box<cast::CastPlan>),
    ActivateAbility(Box<activate::ActivatePlan>),
    DeclareAttackers(combat::AttackersPlan),
    DeclareBands(combat::BandsPlan),
    DeclareBlockers(combat::BlockersPlan),
    AssignCombatDamage(combat::DamagePlan),
}

/// Checks `action` against `state` without changing anything. Every rejection
/// of a command comes from here (024, 035), so `Engine::submit_action` and
/// `execute_command` cannot disagree.
pub(crate) fn validate_command(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    action: PlayerActionKind,
) -> Result<Validated, Rejected> {
    match action {
        PlayerActionKind::PassPriority => land::validate_pass_priority(state, script_runtime, actor),
        PlayerActionKind::MakeChoice { .. } => Ok(Validated::MakeChoice),
        PlayerActionKind::PlayLand { card } => land::validate_play_land(state, actor, card),
        PlayerActionKind::CastSpell { card, modes, targets, x_value, alternative_cost, pay_buyback } => {
            cast::validate_cast_spell(
                state,
                script_runtime,
                actor,
                card,
                modes,
                targets,
                x_value,
                alternative_cost,
                pay_buyback,
            )
        }
        PlayerActionKind::ActivateAbility { source, ability_index, targets, chosen_colors, x_value } => {
            activate::validate_activate_ability(
                state,
                script_runtime,
                actor,
                source,
                ability_index,
                targets,
                chosen_colors,
                x_value,
            )
        }
        PlayerActionKind::DeclareAttackers { attackers } => {
            combat::validate_declare_attackers(state, script_runtime, actor, attackers)
        }
        PlayerActionKind::DeclareBands { bands } => combat::validate_declare_bands(state, script_runtime, actor, bands),
        PlayerActionKind::DeclareBlockers { blocks } => {
            combat::validate_declare_blockers(state, script_runtime, actor, blocks)
        }
        PlayerActionKind::AssignCombatDamage { assignments } => {
            combat::validate_assign_combat_damage(state, script_runtime, actor, assignments)
        }
        // `submit_action` turns the pregame actions away before a command is
        // built, and a payment is answered through `Engine::submit_payment`,
        // never as a `PlayerActionKind`.
        PlayerActionKind::KeepHand | PlayerActionKind::TakeMulligan | PlayerActionKind::BottomCards { .. } => {
            Err(Rejected::IllegalAction("pregame actions use submit_mulligan_decision / submit_bottom_cards".to_string()))
        }
        PlayerActionKind::SubmitPayment { .. } => {
            Err(Rejected::IllegalAction("a payment is answered through submit_payment".to_string()))
        }
    }
}

/// Carries out a command that passed `validate_command`. The only failure is
/// `Internal`: a broken invariant, since validation already ruled out every
/// bad input.
pub(crate) fn apply_command(state: &mut GameState, validated: Validated) -> Result<CommandOutcome, Internal> {
    match validated {
        Validated::PassPriority { actor } => land::apply_pass_priority(state, actor),
        Validated::MakeChoice => land::apply_make_choice(),
        Validated::PlayLand(plan) => land::apply_play_land(state, plan),
        Validated::CastSpell(plan) => cast::apply_cast_spell(state, *plan),
        Validated::ActivateAbility(plan) => activate::apply_activate_ability(state, *plan),
        Validated::DeclareAttackers(plan) => combat::apply_declare_attackers(state, plan),
        Validated::DeclareBands(plan) => combat::apply_declare_bands(state, plan),
        Validated::DeclareBlockers(plan) => combat::apply_declare_blockers(state, plan),
        Validated::AssignCombatDamage(plan) => combat::apply_assign_combat_damage(state, plan),
    }
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
///
/// 024/035: runs `validate_command` first and `apply_command` second. A
/// `Rejected` result has changed nothing; an `Internal` one is fatal.
pub fn execute_command(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    let validated = validate_command(state, script_runtime, command.actor, command.action)?;
    Ok(apply_command(state, validated)?)
}
