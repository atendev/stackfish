//! Carries out validated `EngineCommand`s against `GameState`. This file holds the
//! outcome types and the `execute_command` dispatcher; each arm's body lives in a
//! sibling module. See `DESIGN.md` for the rest.

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

// Re-exports keep `crate::commands::X` resolving for the engine and for the
// integration test that reaches `propose_attach` and `AttachProposal`.
pub use attach::{AttachProposal, propose_attach};
pub(crate) use attach::{
    apply_copy_additional_counters, attach_permanent, attach_permanent_to_player, find_ability,
    find_ability_by_id, grant_linked_copy_ability, is_aura, is_equipment,
    legal_aura_entry_candidates, legal_entry_attach_candidates, next_active_player,
    non_eliminated_players, purge_granted_abilities, remap_abilities, retire_object,
};
pub(crate) use combat::{
    attacker_entries, blocker_entries, damage_split_entries, execute_assign_combat_damage,
    mark_effect_blocked, next_blocking_player,
};
pub(crate) use entries::priority_entries;
pub(crate) use legality::{
    attachment_target_is_legal, combine_chosen_modes, effective_keywords, effective_power,
    is_ability_stack_entry, is_modal_placeholder_requirement, is_modal_resolvable,
    legal_target_candidates_for_spec, mode_can_join_others, solo_only_modes,
    split_targets_by_requirement, target_is_legal, target_spec_minimums, validate_targets,
};
pub(crate) use zone_ops::{
    apply_damage_event, collect_cost_zone_moves, commit_payment, commit_zone_move, propose_draw,
    propose_zone_move,
};

/// Why a command was turned down before it changed anything (CR 733.1, 733.2).
/// Only validation builds one: `validate_*` returns `Result<_, Rejected>` and
/// `apply_*` returns `Result<_, Internal>`, so a mutating step cannot report
/// "rejected" by type. The player keeps priority and may try again.
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
pub(crate) enum CommandError {
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

/// How priority moves after one `EngineCommand`. Returned instead of written to
/// `state.priority_player` so the engine is the only place that writes it.
#[derive(Debug)]
pub(crate) enum PriorityHandoff {
    /// Most variants: priority stays with whoever currently has it.
    Unchanged,
    /// `PassPriority` (CR 117.3c): priority moves to the next player in
    /// `state.turn_order`. Carries the resolved player so `execute_command` is
    /// the only place that knows the rotation rule.
    To(PlayerId),
}

/// What carrying out one `EngineCommand` produced, for the engine to act on.
/// Returned explicitly rather than inferred from `state` afterward.
#[derive(Debug)]
pub(crate) struct CommandOutcome {
    /// Proposed events, not yet final. The engine runs them through
    /// `ReplacementCheck` and `CommitEvents`, so this function never writes the
    /// ledger.
    pub events: Vec<GameEventKind>,
    /// `false` only for `PassPriority` (CR 117.3c). The engine owns
    /// `state.consecutive_passes` and counts passes from this flag, so no match
    /// arm has to remember to update it.
    pub broke_pass_chain: bool,
    /// See `PriorityHandoff`. `engine.rs` applies this to
    /// `state.priority_player` itself.
    pub priority_handoff: PriorityHandoff,
    /// Set by `CastSpell` once the spell is on the stack, which CR 601.2a does
    /// before the cost is paid (CR 601.2f-h). The engine pauses on
    /// `ProcessState::AwaitingPayment` until `Engine::submit_payment` answers, so
    /// a {0} cost is still acknowledged (CR 118.5a). `None` for every other
    /// command.
    pub requires_payment: Option<PendingPaymentRequest>,
    /// Ids that just became battlefield permanents (CR 400.7 gives each a fresh
    /// id). The engine runs their card scripts because `execute_command` holds
    /// only `GameState` and cannot reach `ScriptRuntime`. Empty except for
    /// `PlayLand`; `ResolveStackTop` runs scripts itself.
    pub battlefield_entries: Vec<ObjectId>,
}

/// A cost that waits on `Engine::submit_payment` instead of being deducted
/// inside the command. See `CommandOutcome::requires_payment`.
#[derive(Debug)]
pub(crate) struct PendingPaymentRequest {
    pub stack_id: ObjectId,
    pub payer: PlayerId,
    pub cost: CostSpec,
    /// The object whose cast or activation this cost belongs to.
    /// `AdditionalCost::TapSelf` validates against it (CR 602.5a). For
    /// `CastSpell` it is the spell's `stack_id`, which is harmless because a
    /// spell's `CostSpec` never has `TapSelf`.
    pub source: ObjectId,
    /// What to do once the cost commits. Almost always `StackObject`, whose
    /// object is already on `state.stack` at `stack_id`. `ManaAbility` is the
    /// exception (CR 605.1a, 605.3b): it pays like any ability but never reaches
    /// the stack.
    pub on_paid: PaymentResolution,
}

/// `stack_id` on the enclosing `PendingPaymentRequest` is a resolvable stack
/// object only for `StackObject`. For `ManaAbility` it is a freshly allocated
/// handle (CR 400.7) that nothing on `state.stack` matches.
#[derive(Debug)]
pub(crate) enum PaymentResolution {
    StackObject,
    /// CR 605's mana units to add once the cost commits. They are resolved at
    /// execution time (a fixed `AddMana` list, or an `AddManaChoice` with
    /// validated `chosen_colors`), since paying does not change which mana comes
    /// out.
    ManaAbility {
        controller: PlayerId,
        units: Vec<ManaUnit>,
    },
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
/// of a command comes from here, so `Engine::submit_action` and
/// `execute_command` cannot disagree.
pub(crate) fn validate_command(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    action: PlayerActionKind,
) -> Result<Validated, Rejected> {
    match action {
        PlayerActionKind::PassPriority => {
            land::validate_pass_priority(state, script_runtime, actor)
        }
        PlayerActionKind::MakeChoice { .. } => Ok(Validated::MakeChoice),
        PlayerActionKind::PlayLand { card } => land::validate_play_land(state, actor, card),
        PlayerActionKind::CastSpell {
            card,
            modes,
            targets,
            x_value,
            alternative_cost,
            pay_buyback,
        } => cast::validate_cast_spell(
            state,
            script_runtime,
            actor,
            card,
            modes,
            targets,
            x_value,
            alternative_cost,
            pay_buyback,
        ),
        PlayerActionKind::ActivateAbility {
            source,
            ability_index,
            targets,
            chosen_colors,
            x_value,
        } => activate::validate_activate_ability(
            state,
            script_runtime,
            actor,
            source,
            ability_index,
            targets,
            chosen_colors,
            x_value,
        ),
        PlayerActionKind::DeclareAttackers { attackers } => {
            combat::validate_declare_attackers(state, script_runtime, actor, attackers)
        }
        PlayerActionKind::DeclareBands { bands } => {
            combat::validate_declare_bands(state, script_runtime, actor, bands)
        }
        PlayerActionKind::DeclareBlockers { blocks } => {
            combat::validate_declare_blockers(state, script_runtime, actor, blocks)
        }
        PlayerActionKind::AssignCombatDamage { assignments } => {
            combat::validate_assign_combat_damage(state, script_runtime, actor, assignments)
        }
        // `submit_action` turns the pregame actions away before a command is
        // built, and a payment is answered through `Engine::submit_payment`,
        // never as a `PlayerActionKind`.
        PlayerActionKind::KeepHand
        | PlayerActionKind::TakeMulligan
        | PlayerActionKind::BottomCards { .. } => Err(Rejected::IllegalAction(
            "pregame actions use submit_mulligan_decision / submit_bottom_cards".to_string(),
        )),
        PlayerActionKind::SubmitPayment { .. } => Err(Rejected::IllegalAction(
            "a payment is answered through submit_payment".to_string(),
        )),
    }
}

/// Carries out a command that passed `validate_command`. The only failure is
/// `Internal`: a broken invariant, since validation already ruled out every
/// bad input.
pub(crate) fn apply_command(
    state: &mut GameState,
    validated: Validated,
) -> Result<CommandOutcome, Internal> {
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

/// Executes one accepted `EngineCommand` against `state`, mutating non-ledger
/// state directly and returning a `CommandOutcome`. Runs `validate_command`
/// first and `apply_command` second. A `Rejected` result has changed nothing
/// and an `Internal` one is fatal.
///
/// It does not append to `state.ledger`, allocate `EventId`s, or touch
/// `state.consecutive_passes` or `state.priority_player`. The engine owns those
/// (replacement checks CR 614-616, then `CommitEvents`) so one place writes the
/// ledger. It does not split `command` into sub-commands: the sub-steps are
/// events.
pub(crate) fn execute_command(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    let validated = validate_command(state, script_runtime, command.actor, command.action)?;
    Ok(apply_command(state, validated)?)
}
