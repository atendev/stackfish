use std::collections::HashMap;

use crate::types::ids::{ObjectId, PlayerId, Target};

/// Combat state (CR 506-509), `None` outside combat.
/// `attackers` and `blocks` reuse the tuple shapes of the `DeclareAttackers`
/// and `DeclareBlockers` actions. `blocks` accumulates across submissions,
/// one per defending player (CR 508.5a). `declared_defenders` blocks a repeat
/// submission and separates "declared no blockers" from "not declared yet".
#[derive(Clone, PartialEq, Debug)]
pub struct CombatState {
    pub attackers: Vec<(ObjectId, Target)>,
    pub blocks: Vec<(ObjectId, ObjectId)>,
    pub declared_defenders: Vec<PlayerId>,
    /// CR 702.22c, 702.22e: bands declared with the attackers, one inner
    /// `Vec` per band. Set once by `DeclareBands` and never edited, since a
    /// band lasts the rest of combat. Readers filter against `attackers` for
    /// CR 702.22f instead of pruning this list.
    // TODO: "bands with other [quality]" is not modeled (CR 702.22b)
    pub bands: Vec<Vec<ObjectId>>,
    /// CR 509.1h: attackers made blocked by an effect, with no specific
    /// blocker. `blocks` cannot say "blocked by nothing". Blocked attackers
    /// with no living blocker deal no damage unless they trample (CR 510.1c).
    /// CR 702.22i extends this to the rest of the band. Cleared with
    /// `attackers` and `blocks` when an object leaves combat.
    pub effect_blocked: Vec<ObjectId>,
    /// CR 510.1, 510.2: damage events for the current first-strike or regular
    /// damage step are already built. Reset on entering each such step. A
    /// repeated empty `AssignCombatDamage` after the automatic assignment is
    /// then a no-op, so damage is never dealt twice.
    pub damage_assigned_this_step: bool,
    /// CR 510.1c, 510.1d, 702.22j, 702.22k: validated splits submitted so far
    /// this damage step, keyed by attacker or blocker. Two players can owe
    /// splits in one step (a banding-blocked attacker next to a plain
    /// multi-blocked one), and damage is dealt as one batch (CR 510.2) once
    /// every id needing a split is covered. Cleared on entering a damage step.
    pub pending_damage_splits: HashMap<ObjectId, Vec<(ObjectId, u32)>>,
}
