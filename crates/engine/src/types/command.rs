use crate::types::cost::PaymentPlan;
use crate::types::ids::{CommandId, ObjectId, PlayerId, Target};

/// `EngineCommand` deliberately mirrors `PlayerActionKind` 1:1 rather than
/// decomposing into a lower tier ("pay cost", "push onto stack", ...). The
/// reason: a `Command` represents one validated *player decision*; the
/// internal sub-steps of carrying it out (move card to stack zone, deduct
/// mana, create the `StackObject`) aren't further decisions — they're facts
/// the engine produces while executing the one decision it was given, i.e.
/// `GameEvent`s, not additional `Command`s.
///
/// When casting genuinely requires *another* player decision mid-flight
/// (e.g. "sacrifice a creature" as an additional cost — which creature?),
/// that's not a lower-level command either: it's the next `PlayerAction` in
/// the stream, at the same tier as `CastSpell`. The engine just pauses
/// between `step()` calls expecting `SubmitPayment` next, sequenced by
/// engine state rather than by a command-type hierarchy. This is also why
/// `SubmitPayment` exists as a `PlayerActionKind` variant rather than being
/// folded into `CastSpell`/`ActivateAbility` — payment is its own decision
/// point, decoupled from the action that triggered the need to pay.

/// High-level player intent, as submitted by the host application. Hidden-
/// information choices must always arrive as an explicit `PlayerAction`,
/// never as an implicit default chosen by the engine.
pub enum PlayerActionKind {
    CastSpell { card: ObjectId, modes: Vec<u32>, targets: Vec<Target>, x_value: Option<u32> },
    ActivateAbility { source: ObjectId, ability_index: u32, targets: Vec<Target> },
    PlayLand { card: ObjectId },
    PassPriority,
    /// Each attacker paired with what it's attacking — a player, or a
    /// planeswalker/battle permanent (CR 506.4). Not split into a separate
    /// "defending players" list: which defender goes with which attacker is
    /// the declaration itself, not derivable from two parallel lists.
    DeclareAttackers { attackers: Vec<(ObjectId, Target)> },
    DeclareBlockers { blocks: Vec<(ObjectId, ObjectId)> },
    SubmitPayment { plan: PaymentPlan },
    MakeChoice { description: String, selection: Vec<Target> },
}

pub struct PlayerAction {
    pub player: PlayerId,
    pub kind: PlayerActionKind,
}

impl PlayerAction {
    pub fn new(player: PlayerId, kind: PlayerActionKind) -> Self {
        todo!()
    }
}

/// Fully specified, validated executable unit produced from a `PlayerAction`.
pub struct EngineCommand {
    pub command_id: CommandId,
    pub actor: PlayerId,
    pub action: PlayerActionKind,
}
