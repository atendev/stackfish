use crate::types::ids::{ObjectId, PlayerId, Target};

/// Transient combat-tracking state (CR 506-509), `None` outside combat.
/// `attackers`/`blocks` deliberately reuse the exact tuple shapes
/// `PlayerActionKind::DeclareAttackers`/`DeclareBlockers` already carry
/// (`command.rs`) — no separate wrapper type needed, the declaration *is*
/// the state once accepted. `blocks` accumulates across multiple
/// `DeclareBlockers` submissions (CR 508.5a: multiplayer can have more than
/// one defending player this combat, each declaring their own blocks
/// separately) - `declared_defenders` tracks which defending players have
/// already submitted, both to stop a repeat submission and to distinguish
/// "declared zero blockers" from "hasn't declared yet" (CR 509 allows the
/// former).
pub struct CombatState {
    pub attackers: Vec<(ObjectId, Target)>,
    pub blocks: Vec<(ObjectId, ObjectId)>,
    pub declared_defenders: Vec<PlayerId>,
    /// CR 702.22c: attacking "bands" the active player declared alongside
    /// this combat's attackers - each inner `Vec` is one band's member
    /// object ids. Populated once, by `PlayerActionKind::DeclareBands`
    /// (only legal during `Step::DeclareAttackers`, same window as
    /// `DeclareAttackers` itself), and never mutated afterward - CR 702.22e:
    /// a band lasts the rest of combat even if banding is later removed
    /// from a member. CR 702.22f ("a creature removed from combat leaves
    /// its band") is handled by filtering against `attackers` wherever this
    /// is consulted, not by pruning this list. Empty for the overwhelming
    /// majority of combats (no banding creature attacking). Only plain
    /// banding is representable - CR 702.22b/c's "bands with other
    /// [quality]" variant isn't modeled (see `KeywordSet::BANDING`'s own
    /// doc comment).
    pub bands: Vec<Vec<ObjectId>>,
    /// CR 509.1h: attackers made blocked by an effect ("target attacking
    /// creature becomes blocked") rather than a declared blocker - blocked
    /// by no specific creature. `blocks` (above) can't represent this: it's
    /// a flat list of real `(blocker, attacker)` pairs, with no way to say
    /// "blocked, but by nothing." Consulted alongside `blocks` everywhere
    /// "is this attacker blocked" is decided (currently just `damage.rs`'s
    /// `declared_blocked_attackers`) - CR 510.1c already means a blocked
    /// attacker with zero *living* blockers deals no combat damage (unless
    /// trample), the exact same rule this reuses, just reached with zero
    /// blockers from the start rather than every real one having died.
    /// Populated by `commands::combat::mark_effect_blocked` (CR 702.22i:
    /// also propagates to every other living member of `target`'s own
    /// band). Cleared per-object by the same regeneration/combat-removal
    /// path that already prunes `attackers`/`blocks`.
    pub effect_blocked: Vec<ObjectId>,
    /// `phase5/automatic-combat-damage`, CR 510.1/510.2: whether combat
    /// damage has already been assigned+dealt for the *current*
    /// `Step::FirstStrikeDamage`/`Step::CombatDamage` step - set once
    /// `commands::execute_assign_combat_damage` actually builds damage
    /// events (whether reached via the auto-fire attempt `engine::turn`
    /// makes on entering the step with no real split to choose, or via a
    /// normal explicit `AssignCombatDamage` submission for a step that
    /// does need one). Reset to `false` by `advance_turn_step` on every
    /// transition *into* either step - a game with both a first-strike and
    /// a regular damage step needs its own flag value for each. Lets a
    /// redundant explicit `AssignCombatDamage{assignments: vec![]}` after
    /// the zero-split auto-fire already ran stay a harmless no-op instead
    /// of double-dealing damage - see that function's own guard.
    pub damage_assigned_this_step: bool,
}
