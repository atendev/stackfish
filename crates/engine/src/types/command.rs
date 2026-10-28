use crate::types::cost::PaymentPlan;
use crate::types::ids::{CommandId, ObjectId, PlayerId, Target};
use crate::types::mana::Color;

/// High-level player intent from the host. Hidden-information choices always
/// arrive as an explicit action, never as an engine default.
///
/// `EngineCommand` mirrors this 1:1. A command is one player decision, and
/// the sub-steps of carrying it out are `GameEvent`s. A further decision
/// mid-cast, such as which creature to sacrifice, is the next action in the
/// stream (`SubmitPayment`), and the engine pauses between `step()` calls.
#[derive(Clone, PartialEq, Debug)]
pub enum PlayerActionKind {
    CastSpell {
        card: ObjectId,
        modes: Vec<u32>,
        targets: Vec<Target>,
        x_value: Option<u32>,
        /// CR 601.2b: index into `CardDefinition::alternative_costs`, announced
        /// with `x_value`. `None` pays the printed cost. The card must be in
        /// the alternative's `from_zone`.
        alternative_cost: Option<usize>,
        /// CR 702.27a: also pay `CardDefinition::buyback_cost`. Rejected when
        /// the card has none.
        pay_buyback: bool,
    },
    ActivateAbility {
        source: ObjectId,
        ability_index: u32,
        targets: Vec<Target>,
        /// CR 602.2b: colors for an `AddManaChoice` mana ability, chosen at
        /// activation because a mana ability never uses the stack (CR 605.3b).
        /// Empty for other abilities, and the length must match `count`.
        chosen_colors: Vec<Color>,
        /// CR 107.3a, 107.3k, 602.2b: this activation's own X, present only
        /// when the cost has `{X}`. Independent of any other X.
        x_value: Option<u32>,
    },
    PlayLand { card: ObjectId },
    PassPriority,
    /// Each attacker paired with what it attacks: a player, planeswalker or
    /// battle (CR 506.4).
    DeclareAttackers { attackers: Vec<(ObjectId, Target)> },
    /// CR 508.1e, 702.22c: groups just-declared attackers into bands. A
    /// separate action so ordinary `DeclareAttackers` calls carry no empty
    /// field. Legal in `Step::DeclareAttackers` once attackers are declared,
    /// and each call replaces `CombatState::bands`. Empty is legal.
    DeclareBands { bands: Vec<Vec<ObjectId>> },
    DeclareBlockers { blocks: Vec<(ObjectId, ObjectId)> },
    /// CR 510.1, 510.2: assign and deal combat damage in one action.
    /// `assignments` has an entry for each attacker with 2+ blockers
    /// (CR 510.1c) or one blocker and trample (CR 702.19b), and for each
    /// blocker a band forces onto 2+ attackers (CR 702.22h). The shape is the
    /// same for both, told apart by `blockers_by_attacker` and
    /// `attackers_by_blocker`. Everyone else has one legal target and needs
    /// no entry (CR 510.1e).
    // TODO: the defending player chooses the split when a banding creature blocks (CR 702.22j)
    AssignCombatDamage { assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)> },
    SubmitPayment { plan: PaymentPlan },
    MakeChoice { description: String, selection: Vec<Target> },
    /// CR 103.5: keep the opening hand. Valid only in
    /// `AwaitingMulliganDecision` for this player.
    KeepHand,
    /// CR 103.5: shuffle the hand back and redraw `starting_hand_size`. Valid
    /// only in `AwaitingMulliganDecision`, until `GameSettings::max_mulligans`.
    TakeMulligan,
    /// CR 103.5: after keeping, bottom one card per mulligan taken (less free
    /// mulligans) in the chosen order. Valid only in `AwaitingBottomCardChoice`.
    BottomCards { cards: Vec<ObjectId> },
}

#[derive(Clone, PartialEq, Debug)]
pub struct PlayerAction {
    pub player: PlayerId,
    pub kind: PlayerActionKind,
}

impl PlayerAction {
    pub fn new(player: PlayerId, kind: PlayerActionKind) -> Self {
        PlayerAction { player, kind }
    }
}

/// Fully specified, validated executable unit produced from a `PlayerAction`.
#[derive(Clone, PartialEq, Debug)]
pub struct EngineCommand {
    pub command_id: CommandId,
    pub actor: PlayerId,
    pub action: PlayerActionKind,
}
