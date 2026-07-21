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
}
