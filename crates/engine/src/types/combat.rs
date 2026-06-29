use crate::types::ids::{ObjectId, Target};

/// Transient combat-tracking state (CR 506-509), `None` outside combat.
/// `attackers`/`blocks` deliberately reuse the exact tuple shapes
/// `PlayerActionKind::DeclareAttackers`/`DeclareBlockers` already carry
/// (`command.rs`) — no separate wrapper type needed, the declaration *is*
/// the state once accepted. `blockers_declared` exists because `blocks`
/// being empty is ambiguous on its own — CR 509 allows declaring zero
/// blockers, which has to be distinguishable from "blockers not declared
/// yet" so `DeclareBlockers` can't be submitted twice.
pub struct CombatState {
    pub attackers: Vec<(ObjectId, Target)>,
    pub blocks: Vec<(ObjectId, ObjectId)>,
    pub blockers_declared: bool,
}
