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
    CastSpell {
        card: ObjectId,
        modes: Vec<u32>,
        targets: Vec<Target>,
        x_value: Option<u32>,
        /// CR 601.2b: index into the cast card's `CardDefinition::alternative_costs`,
        /// announced as part of casting (same phase as `x_value`) — `None`
        /// means casting normally for the printed `mana_cost`. `commands.rs`'s
        /// `CastSpell` arm rejects an out-of-range index, and requires the
        /// card to actually be in the chosen alternative's `from_zone`.
        alternative_cost: Option<usize>,
    },
    ActivateAbility { source: ObjectId, ability_index: u32, targets: Vec<Target> },
    PlayLand { card: ObjectId },
    PassPriority,
    /// Each attacker paired with what it's attacking — a player, or a
    /// planeswalker/battle permanent (CR 506.4). Not split into a separate
    /// "defending players" list: which defender goes with which attacker is
    /// the declaration itself, not derivable from two parallel lists.
    DeclareAttackers { attackers: Vec<(ObjectId, Target)> },
    DeclareBlockers { blocks: Vec<(ObjectId, ObjectId)> },
    /// CR 510.1/510.2: assign + deal combat damage, collapsed into one
    /// action (510.2 - "no priority window between assign and deal"). Only
    /// the active player ever submits a real choice under this engine's
    /// block model (`CombatState.blocks` is one blocker/one attacker, no
    /// banding) - a blocker always deals 100% of its power to the one
    /// attacker it blocks (510.1d, never ambiguous), and an attacker with
    /// 0 or 1 blockers has nowhere to split either (510.1b/c). The only
    /// real choice is an attacker with 2+ blockers splitting its damage
    /// among them (510.1c) - `assignments` covers exactly that subset,
    /// attacker -> (blocker, amount) pairs summing to the attacker's power
    /// (510.1e atomicity). Engine computes every other attacker's and every
    /// blocker's damage automatically.
    AssignCombatDamage { assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)> },
    SubmitPayment { plan: PaymentPlan },
    MakeChoice { description: String, selection: Vec<Target> },
    /// CR 103.5: player declares they are keeping their current opening hand.
    /// Only valid while `ProcessState::AwaitingMulliganDecision` is active for
    /// this player.
    KeepHand,
    /// CR 103.5: player declares a mulligan — their hand is shuffled back into
    /// their library and they draw `starting_hand_size` cards again. Only valid
    /// while `ProcessState::AwaitingMulliganDecision` is active for this player,
    /// and only if they have not reached `GameSettings::max_mulligans`.
    TakeMulligan,
    /// CR 103.5: after keeping with N mulligans taken, player puts exactly N
    /// cards (minus any free mulligans) from their hand on the bottom of their
    /// library in the chosen order. Only valid while
    /// `ProcessState::AwaitingBottomCardChoice` is active for this player.
    BottomCards { cards: Vec<ObjectId> },
}

pub struct PlayerAction {
    pub player: PlayerId,
    pub kind: PlayerActionKind,
}

impl PlayerAction {
    pub fn new(player: PlayerId, kind: PlayerActionKind) -> Self {
        // TODO: PlayerAction { player, kind }.
        todo!()
    }
}

/// Fully specified, validated executable unit produced from a `PlayerAction`.
pub struct EngineCommand {
    pub command_id: CommandId,
    pub actor: PlayerId,
    pub action: PlayerActionKind,
}
