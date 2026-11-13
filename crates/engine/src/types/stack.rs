use crate::types::ability::{AbilityEffect, InterveningIfSpec};
use crate::types::cost::CostPayment;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::object::GameObject;
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

#[derive(Clone, PartialEq, Debug)]
pub enum StackEntryKind {
    Spell,
    ActivatedAbility,
    TriggeredAbility,
    // TODO: a copy that chooses new targets re-announces its division (CR 707.10c)
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
    /// CR 601.2d: the announced shares, index for index with `targets`. Empty
    /// for an undivided effect or a single target, which takes the whole total.
    /// A 0 marks a position outside a divided word of two or more targets.
    pub division: Vec<u32>,
    /// Set at push from `CardDefinition::target_requirement` or
    /// `RegisteredAbility::target`. `ResolveStackTop` rechecks `targets`
    /// against it (CR 608.2b). The target count is fixed at cast (CR 601.2c).
    pub target_requirement: Option<TargetSpec>,
    pub x_value: Option<u32>,
    /// CR 702.34a, 608.2n: set at push from the chosen `AlternativeCost` or
    /// the `ZonePermission` the spell was cast through, never for an ability
    /// or a copy. Read only through `leave_destination`.
    pub leave_stack_rider: Option<LeaveStackRider>,
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
    /// The ability's position among its source's abilities at push, as
    /// `find_ability` counts them. `None` for a spell or a departed source.
    pub ability_index: Option<u32>,
    /// CR 405.4, 113.7a: the ability's text, captured at push because the
    /// ability exists apart from its source. `None` for a spell.
    pub ability_text: Option<String>,
}

/// CR 702.34a, 608.2n: where a spell goes in place of its usual zone when it
/// leaves the stack. `ExileInstead` is flashback's wording and covers any
/// destination, and `ExileInsteadOfGraveyard` replaces only the graveyard.
// TODO: no "if it would die or leave exile this way" rider and no battlefield-side exile such as unearth
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeaveStackRider {
    ExileInstead,
    ExileInsteadOfGraveyard,
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
    #[allow(clippy::too_many_arguments)]
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
        leave_stack_rider: Option<LeaveStackRider>,
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
            ability_index: None,
            ability_text: None,
            stack_id,
            kind,
            source,
            source_lki,
            owner,
            controller,
            chosen_modes,
            targets,
            division: Vec::new(),
            target_requirement,
            x_value,
            leave_stack_rider,
            bought_back,
            escaped,
            costs_paid: Vec::new(),
            operation,
            effect,
            intervening_if,
            pending_mode_choice,
        }
    }

    /// CR 608.2n, 702.34a: the zone this spell moves to when it leaves the
    /// stack and would otherwise go to `default`.
    pub fn leave_destination(&self, default: ZoneKind) -> ZoneKind {
        match self.leave_stack_rider {
            Some(LeaveStackRider::ExileInstead) => ZoneKind::Exile,
            Some(LeaveStackRider::ExileInsteadOfGraveyard) if default == ZoneKind::Graveyard => {
                ZoneKind::Exile
            }
            _ => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spell(rider: Option<LeaveStackRider>) -> StackObject {
        StackObject::new(
            ObjectId(1),
            StackEntryKind::Spell,
            ObjectId(1),
            PlayerId(0),
            PlayerId(0),
            Vec::new(),
            Vec::new(),
            None,
            None,
            rider,
            false,
            false,
            ScriptHandle(0),
            None,
            None,
            None,
            None,
        )
    }

    // CR 608.2n, 702.34a
    #[test]
    fn leave_destination_applies_each_rider() {
        use ZoneKind::{Battlefield, Exile, Graveyard, Hand};
        for (rider, default, expected) in [
            (None, Graveyard, Graveyard),
            (None, Hand, Hand),
            (Some(LeaveStackRider::ExileInstead), Graveyard, Exile),
            (Some(LeaveStackRider::ExileInstead), Battlefield, Exile),
            (
                Some(LeaveStackRider::ExileInsteadOfGraveyard),
                Graveyard,
                Exile,
            ),
            (Some(LeaveStackRider::ExileInsteadOfGraveyard), Hand, Hand),
        ] {
            assert_eq!(
                spell(rider).leave_destination(default),
                expected,
                "{rider:?}"
            );
        }
    }
}
