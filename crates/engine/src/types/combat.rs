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
}
