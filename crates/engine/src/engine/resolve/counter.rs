//! `resolve_ability_effect`'s `Counter`/`UnlessPaid` arms (CR 701.6a/
//! 118.12) — see `AbilityEffect::Counter`/`UnlessPaid`'s own doc
//! comments in `types/ability.rs` for the full shape and named TODOs. Unlike
//! every other single-target arm in this subdirectory, the object this
//! mutates isn't reached through `legal_targets`'s usual "battlefield object,
//! zone-move via `continue_scripted_zone_move`" path — it's a *different*,
//! still-present `StackObject` living in `state.stack` itself, found and
//! removed by matching `stack_id` directly.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::types::ability::UnlessPaidConsequence;
use crate::types::cost::CostSpec;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, Target};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    /// `legal_targets`' first (only) entry, resolved to the `StackObject`'s
    /// own `stack_id` it names - shared by `resolve_counter_effect` and
    /// `resolve_unless_paid_effect` (CR 113.9: `Target::Object` at
    /// `zone: Stack` for "counter target spell," `Target::Ability` for
    /// "counter target activated or triggered ability," Stifle/Trickbind -
    /// both just name the same `stack_id` here, downstream code dispatches
    /// on the found `StackObject`'s own `kind`, not on which `Target`
    /// variant named it).
    fn counter_target_id(legal_targets: &[Target]) -> Option<ObjectId> {
        match legal_targets.first().copied() {
            Some(Target::Object(id)) | Some(Target::Ability(id)) => Some(id),
            Some(Target::Player(_)) | None => None,
        }
    }

    /// Finds and removes `target_id` from `state.stack` (a plain `Vec`, no
    /// dedicated removal primitive existed for anything but the top -
    /// `resolve_stack_top`'s own `pop()` only ever touches the last
    /// element), pushing the resulting `Countered`/`ZoneChange` event(s)
    /// onto `events`. Shared tail for `resolve_counter_effect` (unconditional
    /// CR 701.6a) and `decline_unless_paid`'s own `Counter` dispatch (CR
    /// 118.12's "if [the player] doesn't" branch) - same removal either way,
    /// only *how* this point was reached differs.
    ///
    /// A countered ability "just stops existing"
    /// (`StackEntryKind::ActivatedAbility`/`TriggeredAbility` - same as a
    /// fizzled one, no `ZoneChange`); a countered spell's own card goes to
    /// its owner's graveyard (or exile, if *that* spell's own
    /// `exile_on_leaving_stack` says so - CR 702.34a) via the exact same
    /// `Engine::move_resolving_spell` helper the CR 608.2b fizzle branch
    /// already reuses for the identical zone-move shape - no
    /// `apply_replacements` call, same "not a `WouldDie`-shaped
    /// replacement-checked move" stance that branch takes. No CR 616.1
    /// ordering pause either - a single stack entry being removed, not
    /// several simultaneous battlefield deaths.
    ///
    /// If the target already left the stack by the time this runs, the
    /// `position` lookup below simply finds nothing and this is a silent
    /// no-op - a structural impossibility for `resolve_counter_effect` (CR
    /// 608.2b's own recheck in `resolve_stack_top` would have already
    /// fizzled the Counter spell itself instead of reaching this far), but a
    /// real, reachable case for `decline_unless_paid`'s own `Counter`
    /// dispatch: nothing stops the targeted spell from itself resolving/
    /// fizzling/being countered by something else while this engine sits
    /// paused on `AwaitingUnlessPaidChoice`. TODO: that's not actually
    /// preventable yet (no re-legality-check on resume), named here rather
    /// than guessed at.
    pub(in crate::engine) fn counter_stack_object(&mut self, target_id: ObjectId, events: &mut Vec<GameEventKind>) {
        let Some(pos) = self.state.stack.iter().position(|so| so.stack_id == target_id) else {
            return;
        };
        let countered: StackObject = self.state.stack.remove(pos);
        if matches!(countered.kind, StackEntryKind::Spell) {
            let destination = if countered.exile_on_leaving_stack { ZoneKind::Exile } else { ZoneKind::Graveyard };
            if let Some(event) = self.move_resolving_spell(target_id, destination, false, false) {
                events.push(event);
            }
        }
        events.push(GameEventKind::Countered { stack_id: target_id });
    }

    /// CR 701.6a: unconditionally counters `legal_targets`' own target - see
    /// `counter_stack_object`'s own doc comment for the actual removal.
    pub(super) fn resolve_counter_effect(
        &mut self,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(target_id) = Self::counter_target_id(legal_targets) {
            self.counter_stack_object(target_id, &mut events);
        }
        ControlFlow::Continue(events)
    }

    /// CR 118.12/118.12a: pauses on `ProcessState::AwaitingUnlessPaidChoice`
    /// instead of resolving synchronously - see `AbilityEffect::UnlessPaid`/
    /// `UnlessPaidConsequence`'s own doc comments for the full shape shared
    /// across every real keyword built on this. `payer_queue` is a single
    /// element for both real producers here (`Counter`: the *targeted*
    /// spell/ability's own controller, not necessarily this resolving
    /// ability's; `SacrificeSelf`: this ability's own controller) - see
    /// `UnlessPaidConsequence`'s own doc comment for why the queue shape
    /// exists anyway. `Counter` with no legal target (already fizzled via
    /// `target_is_legal` upstream, or this ability's own target was never
    /// validly assigned) is a silent no-op, same stance `resolve_counter_
    /// effect` above already takes for its own defensive case - nothing to
    /// ask a payer about if there's no target at all.
    pub(super) fn resolve_unless_paid_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        cost: &CostSpec,
        otherwise: &UnlessPaidConsequence,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let (target_id, payer) = match otherwise {
            UnlessPaidConsequence::Counter => {
                let Some(target_id) = Self::counter_target_id(legal_targets) else {
                    return ControlFlow::Continue(events);
                };
                // Structurally the same "already left the stack"
                // defensiveness counter_stack_object's own doc comment
                // names - shouldn't happen this early (target_is_legal
                // already rechecked it), but there's no StackObject to read
                // a controller off if it somehow did.
                let Some(payer) =
                    self.state.stack.iter().find(|so| so.stack_id == target_id).map(|so| so.controller)
                else {
                    return ControlFlow::Continue(events);
                };
                (Some(target_id), payer)
            }
            // CR 702.30a Echo: "unless you pay" - always this ability's own
            // controller, no target at all.
            UnlessPaidConsequence::SacrificeSelf => (None, top.controller),
        };
        self.process_state = ProcessState::AwaitingUnlessPaidChoice {
            payer_queue: VecDeque::from([payer]),
            cost: cost.clone(),
            otherwise: otherwise.clone(),
            target_id,
            source: top.source,
            stack_id: top.stack_id,
            events_so_far: events,
        };
        ControlFlow::Break(StepStatus::AwaitingUnlessPaidChoice { payer, cost: cost.clone(), otherwise: otherwise.clone() })
    }
}
