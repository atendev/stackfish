use crate::types::cost::CostPayment;
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::object::GameObject;

pub enum StackEntryKind {
    Spell,
    ActivatedAbility,
    TriggeredAbility,
    Copy { of: Box<StackEntryKind> },
}

/// A spell, activated ability, triggered ability, or copy thereof on the stack.
pub struct StackObject {
    pub stack_id: ObjectId,
    pub kind: StackEntryKind,
    pub source: ObjectId,
    /// Last-known-information snapshot, captured if `source` may leave the
    /// zone it was in before this resolves (CR 608.2b).
    pub source_lki: Option<GameObject>,
    pub controller: PlayerId,
    pub chosen_modes: Vec<u32>,
    pub targets: Vec<ObjectId>,
    pub x_value: Option<u32>,
    pub costs_paid: Vec<CostPayment>,
}
