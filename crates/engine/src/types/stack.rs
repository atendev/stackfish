use crate::types::ability::AbilityEffect;
use crate::types::cost::CostPayment;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::object::GameObject;
use crate::types::target::TargetSpec;

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
    /// Baked in at push time from whatever produced this `StackObject`
    /// (`CardDefinition::target_requirement` for a spell,
    /// `RegisteredAbility::target` for an ability) — same "primitive
    /// stand-in, decided once at push, not re-derived" contract `effect`
    /// already established. `ResolveStackTop` rechecks `targets` against
    /// this at resolution time (CR 608.2b); it does *not* need the original
    /// `count`, only the `filter`, since CR 601.2c fixes the count at cast
    /// time and it can't change afterward — but the full `TargetSpec` is
    /// carried anyway (not just `TargetFilter`) so the `Script` case has
    /// somewhere to eventually live once that's evaluable.
    pub target_requirement: Option<TargetSpec>,
    pub x_value: Option<u32>,
    pub costs_paid: Vec<CostPayment>,
    /// Which script callback to invoke on resolution. Present in the
    /// original adaptation doc's `StackObject` spec ("resolution callback
    /// reference") but dropped in Phase 1 before the script bridge existed
    /// to give it a concrete type — restored now as a `ScriptHandle`.
    pub operation: ScriptHandle,
    /// Same "primitive stand-in when Some, script fallback when None"
    /// contract as `RegisteredAbility::effect` — baked in at push time
    /// (`PutTriggersApnap`) from the ability that produced this
    /// `StackObject`, so resolution can apply it directly without a Lua
    /// call. `None` falls back to invoking `operation` for real.
    pub effect: Option<AbilityEffect>,
}

impl StackObject {
    /// `chosen_modes`/`targets`/`x_value` are taken as params (rather than
    /// always starting empty/`None`) since the first real caller
    /// (`commands::execute_command`'s `CastSpell` arm) already has these
    /// from `PlayerActionKind::CastSpell` by the time it builds a
    /// `StackObject` — deferring them to a separate mutation step would just
    /// mean constructing this in a momentarily-inconsistent state.
    pub fn new(
        stack_id: ObjectId,
        kind: StackEntryKind,
        source: ObjectId,
        controller: PlayerId,
        chosen_modes: Vec<u32>,
        targets: Vec<Target>,
        target_requirement: Option<TargetSpec>,
        x_value: Option<u32>,
        operation: ScriptHandle,
        effect: Option<AbilityEffect>,
    ) -> Self {
        StackObject {
            stack_id,
            kind,
            source,
            // Not captured at construction - only meaningful once `source`
            // might actually leave its zone before this resolves (CR
            // 608.2b), which is a later mutation, not part of building this.
            source_lki: None,
            controller,
            chosen_modes,
            targets,
            target_requirement,
            x_value,
            costs_paid: Vec::new(),
            operation,
            effect,
        }
    }
}
