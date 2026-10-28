use crate::types::ability::{AbilityEffect, InterveningIfSpec};
use crate::types::cost::CostPayment;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::object::GameObject;
use crate::types::target::TargetSpec;

#[derive(Clone, PartialEq, Debug)]
pub enum StackEntryKind {
    Spell,
    ActivatedAbility,
    TriggeredAbility,
    Copy { of: Box<StackEntryKind> },
}

/// A spell, activated ability, triggered ability, or copy thereof on the stack.
#[derive(Clone, PartialEq, Debug)]
pub struct StackObject {
    pub stack_id: ObjectId,
    pub kind: StackEntryKind,
    pub source: ObjectId,
    /// Last-known-information snapshot, captured if `source` may leave the
    /// zone it was in before this resolves (CR 608.2b).
    pub source_lki: Option<GameObject>,
    /// CR 800.4a: owner of what this represents. A spell's is its card's
    /// owner, an ability's is its source's owner, either of which may differ
    /// from `controller`. A leaving player takes owned spells but only
    /// controlled abilities; see `sba::remove_players_objects`.
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub chosen_modes: Vec<u32>,
    pub targets: Vec<Target>,
    /// Set at push from `CardDefinition::target_requirement` or
    /// `RegisteredAbility::target`. `ResolveStackTop` rechecks `targets`
    /// against it (CR 608.2b). The target count is fixed at cast (CR 601.2c).
    pub target_requirement: Option<TargetSpec>,
    pub x_value: Option<u32>,
    /// CR 702.34a: set at push from the chosen `AlternativeCost`, never for an
    /// ability. A resolving or fizzling spell then goes to exile, not the
    /// graveyard.
    pub exile_on_leaving_stack: bool,
    /// CR 702.27a: set at push when buyback was paid. A resolving instant or
    /// sorcery returns to hand. The fizzle path ignores it, since the rule
    /// covers only resolution.
    pub bought_back: bool,
    /// CR 702.138b: set at push from `AlternativeCost::Replacement::escape`.
    /// `Engine::move_resolving_spell` copies it to
    /// `GameObject::entered_via_escape` on entering the battlefield.
    pub escaped: bool,
    pub costs_paid: Vec<CostPayment>,
    /// Lua callback to invoke on resolution.
    pub operation: ScriptHandle,
    /// Set at push from the producing ability and applied directly at
    /// resolution. `None` calls `operation` instead.
    pub effect: Option<AbilityEffect>,
    /// CR 603.4, 608.2a: set at push from `RegisteredAbility::intervening_if`,
    /// only for triggered abilities. `ResolveStackTop` rechecks it against
    /// current state.
    pub intervening_if: Option<InterveningIfSpec>,
    /// CR 700.2e: set for a modal spell whose mode an opponent chooses and has
    /// not yet. `chosen_modes`, `target_requirement` and `effect` stay
    /// placeholders until `Engine::submit_mode_choice` fills them in and
    /// clears this. `ResolveStackTop` pauses on `AwaitingModeChoice` while set.
    pub pending_mode_choice: Option<PendingModeChoice>,
}

/// See `StackObject::pending_mode_choice`.
#[derive(Clone, PartialEq, Debug)]
pub struct PendingModeChoice {
    pub chooser: PlayerId,
    pub min: u32,
    pub max: u32,
}

impl StackObject {
    /// Takes `chosen_modes`, `targets` and `x_value` because a cast already
    /// has them, which avoids building an inconsistent object first.
    pub fn new(
        stack_id: ObjectId,
        kind: StackEntryKind,
        source: ObjectId,
        owner: PlayerId,
        controller: PlayerId,
        chosen_modes: Vec<u32>,
        targets: Vec<Target>,
        target_requirement: Option<TargetSpec>,
        x_value: Option<u32>,
        exile_on_leaving_stack: bool,
        bought_back: bool,
        escaped: bool,
        operation: ScriptHandle,
        effect: Option<AbilityEffect>,
        intervening_if: Option<InterveningIfSpec>,
        pending_mode_choice: Option<PendingModeChoice>,
        // CR 603.4, 608.2a: set for a trigger pushed from a departed source whose
        // `intervening_if` needs it (`triggers::find_departed_lki`).
        source_lki: Option<GameObject>,
    ) -> Self {
        StackObject {
            stack_id,
            kind,
            source,
            source_lki,
            owner,
            controller,
            chosen_modes,
            targets,
            target_requirement,
            x_value,
            exile_on_leaving_stack,
            bought_back,
            escaped,
            costs_paid: Vec::new(),
            operation,
            effect,
            intervening_if,
            pending_mode_choice,
        }
    }
}
