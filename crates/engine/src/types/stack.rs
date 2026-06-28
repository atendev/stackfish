use crate::types::cost::CostPayment;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
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
    pub targets: Vec<Target>,
    pub x_value: Option<u32>,
    pub costs_paid: Vec<CostPayment>,
    /// Which script callback to invoke on resolution. Present in the
    /// original adaptation doc's `StackObject` spec ("resolution callback
    /// reference") but dropped in Phase 1 before the script bridge existed
    /// to give it a concrete type — restored now as a `ScriptHandle`.
    pub operation: ScriptHandle,
}

impl StackObject {
    pub fn new(
        stack_id: ObjectId,
        kind: StackEntryKind,
        source: ObjectId,
        controller: PlayerId,
        operation: ScriptHandle,
    ) -> Self {
        todo!()
    }
}
