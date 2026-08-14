//! `resolve_ability_effect`'s `Counter` arm (CR 701.6a) — see
//! `AbilityEffect::Counter`'s own doc comment in `types/ability.rs` for the
//! full shape and named TODOs. Unlike every other single-target arm in this
//! subdirectory, the object this mutates isn't reached through
//! `legal_targets`'s usual "battlefield object, zone-move via
//! `continue_scripted_zone_move`" path — it's a *different*, still-present
//! `StackObject` living in `state.stack` itself, found and removed by
//! matching `stack_id` directly.

use std::ops::ControlFlow;

use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::StackEntryKind;
use crate::types::zone::ZoneKind;

use super::super::{Engine, StepStatus};

impl Engine {
    /// CR 701.6a: `legal_targets`' first (only) entry is another
    /// `StackObject`'s `stack_id` — a `Target::Object` at `zone: Stack` for
    /// "counter target spell," or a `Target::Ability` for "counter target
    /// activated or triggered ability" (CR 113.9, Stifle/Trickbind). Finds
    /// and removes that entry from
    /// `state.stack` (a plain `Vec`, no dedicated removal primitive existed
    /// for anything but the top - `resolve_stack_top`'s own `pop()` only
    /// ever touches the last element). A countered ability "just stops
    /// existing" (`StackEntryKind::ActivatedAbility`/`TriggeredAbility` -
    /// same as a fizzled one, no `ZoneChange`); a countered spell's own card
    /// goes to its owner's graveyard (or exile, if *that* spell's own
    /// `exile_on_leaving_stack` says so - CR 702.34a) via the exact same
    /// `Engine::move_resolving_spell` helper the CR 608.2b fizzle branch
    /// already reuses for the identical zone-move shape - no
    /// `apply_replacements` call, same "not a `WouldDie`-shaped
    /// replacement-checked move" stance that branch takes.
    ///
    /// If the target already left the stack by the time this runs (a
    /// structural impossibility here - CR 608.2b's own recheck in
    /// `resolve_stack_top` would have already fizzled *this* Counter spell
    /// instead of reaching this arm, since a target whose `zone` is no
    /// longer `Stack` fails `target_is_legal`), the `position` lookup below
    /// simply finds nothing and this is a silent no-op - defensive, not
    /// expected to ever actually happen.
    pub(super) fn resolve_counter_effect(
        &mut self,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let target_id = match legal_targets.first().copied() {
            // CR 113.9: "counter target spell" (Target::Object, zone: Stack)
            // and "counter target activated or triggered ability"
            // (Target::Ability) both just resolve to the same stack_id here
            // - the branch below already dispatches on the found
            // StackObject's own kind, not on which Target variant named it.
            Some(Target::Object(id)) | Some(Target::Ability(id)) => Some(id),
            Some(Target::Player(_)) | None => None,
        };
        if let Some(target_id) = target_id {
            if let Some(pos) = self.state.stack.iter().position(|so| so.stack_id == target_id) {
                let countered = self.state.stack.remove(pos);
                if matches!(countered.kind, StackEntryKind::Spell) {
                    let destination =
                        if countered.exile_on_leaving_stack { ZoneKind::Exile } else { ZoneKind::Graveyard };
                    if let Some(event) = self.move_resolving_spell(target_id, destination, false, false) {
                        events.push(event);
                    }
                }
                events.push(GameEventKind::Countered { stack_id: target_id });
            }
        }
        ControlFlow::Continue(events)
    }
}
