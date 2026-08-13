use crate::types::cost::PaymentPlan;
use crate::types::ids::{CommandId, ObjectId, PlayerId, Target};
use crate::types::mana::Color;

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
        /// CR 702.27a Buyback: whether the caster is paying the card's
        /// `CardDefinition::buyback_cost` (if any) in addition to whatever
        /// cost already applies — same "announced at cast time" phase as
        /// `alternative_cost`/`x_value`. `commands.rs`'s `CastSpell` arm
        /// rejects `true` on a card with no `buyback_cost`.
        pay_buyback: bool,
    },
    ActivateAbility {
        source: ObjectId,
        ability_index: u32,
        targets: Vec<Target>,
        /// CR 602.2b (activation follows the CR 601.2b-i casting process):
        /// the color(s) chosen for an `AbilityEffect::AddManaChoice` mana
        /// ability ("add one mana of any color"), locked in as part of
        /// activation itself, not a mid-resolution pause - a mana ability
        /// never uses the stack (CR 605.3b), so there's no resolution step
        /// to pause during anyway. Empty for every other ability, same
        /// "always-present, usually-empty" convention `targets`/`modes`
        /// already follow on `CastSpell`. `commands.rs`'s `ActivateAbility`
        /// arm rejects a non-empty value against any ability that isn't
        /// `AddManaChoice`, and rejects a length mismatch against
        /// `AddManaChoice`'s own `count`.
        chosen_colors: Vec<Color>,
        /// CR 107.3a/107.3k/602.2b: this activation's own announced X, if
        /// this ability's activation cost has an `{X}` in it - independent
        /// of any other X on the same object (107.3k), and of any X
        /// announced for a different instance of this same ability already
        /// on the stack. Same "must agree with whether the cost actually
        /// has an `{X}`" contract as `CastSpell::x_value`.
        x_value: Option<u32>,
    },
    PlayLand { card: ObjectId },
    PassPriority,
    /// Each attacker paired with what it's attacking — a player, or a
    /// planeswalker/battle permanent (CR 506.4). Not split into a separate
    /// "defending players" list: which defender goes with which attacker is
    /// the declaration itself, not derivable from two parallel lists.
    DeclareAttackers { attackers: Vec<(ObjectId, Target)> },
    /// CR 508.1e/702.22c: banding - the active player groups one or more
    /// just-declared attacking creatures with banding (plus up to one
    /// attacking creature without banding) into a "band." A separate
    /// action from `DeclareAttackers` rather than a field on it (unlike CR
    /// 508.1e's own "as a player declares attackers" wording) so the
    /// overwhelmingly common no-banding case - every other `DeclareAttackers`
    /// call site in this codebase - never has to carry an always-empty
    /// field. Only legal in the same window as `DeclareAttackers` itself
    /// (`Step::DeclareAttackers`, after attackers have been declared this
    /// combat); each call replaces `CombatState::bands` wholesale, so a
    /// player can resubmit to fix a mistake before priority opens. Empty
    /// `bands` is a legal (if pointless) submission, same "declare zero"
    /// convention `DeclareBlockers` already allows.
    DeclareBands { bands: Vec<Vec<ObjectId>> },
    DeclareBlockers { blocks: Vec<(ObjectId, ObjectId)> },
    /// CR 510.1/510.2: assign + deal combat damage, collapsed into one
    /// action (510.2 - "no priority window between assign and deal"). Only
    /// the active player ever submits a real choice under this engine's
    /// block model - CR 702.22k already makes that the correct chooser even
    /// for banding's blocker-forced-onto-multiple-attackers case, so no
    /// separate defending-player-side submission is needed for that half.
    /// `assignments` is generic over which side needs a split: an attacker
    /// with 2+ blockers (510.1c) or exactly 1 with trample (702.19b) splits
    /// its power among blockers; a blocker forced onto 2+ attackers by band
    /// propagation (702.22h) splits its power among attackers instead - same
    /// `id -> (recipient, amount)` tuple shape either way, disambiguated at
    /// lookup time by which of `blockers_by_attacker`/`attackers_by_blocker`
    /// contains `id` (`commands.rs`). Every other attacker/blocker (0 or 1
    /// recipient, no trample) has exactly one legal place for its damage to
    /// go and needs no entry (510.1e atomicity - `assignments` must cover
    /// exactly the set that needs one). CR 702.22j (defending player, not
    /// active player, chooses an attacker's split when it's blocked by a
    /// creature with banding) is NOT modeled - this action's actor is always
    /// the active player, full stop; that half of banding stays a known
    /// deviation.
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
    pub fn new(_player: PlayerId, _kind: PlayerActionKind) -> Self {
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
