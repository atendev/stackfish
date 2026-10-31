//! The synchronous `AbilityEffect` arms: life, damage, fight, counters, pump, control,
//! regenerate, tap and untap. They never pause. `Modal` lives in `modal.rs`. See `DESIGN.md`.

use std::ops::ControlFlow;

use crate::commands;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::counter::CounterKind;
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter,
    EffectPayload, EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload,
    ReplacementSpec, matches_effect_filter,
};
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::StackObject;
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

use super::super::{Engine, StepStatus};

/// The player a `Mill` or `LoseLife` acts on. Decided by whether the ability
/// declared a target spec: none means the controller; a spec means the first legal
/// player target, or `None` when it is gone, so an illegal target never becomes
/// a controller effect.
pub(super) fn affected_player(top: &StackObject, legal_targets: &[Target]) -> Option<PlayerId> {
    if top.target_requirement.is_none() {
        return Some(top.controller);
    }
    legal_targets.iter().find_map(|t| match t {
        Target::Player(player) => Some(*player),
        _ => None,
    })
}

impl Engine {
    pub(super) fn resolve_gain_life_effect(
        &mut self,
        top: &StackObject,
        amount: i32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(player) = self.state.player_mut(top.controller) {
            player.gain_life(amount);
        }
        ControlFlow::Continue(events)
    }

    /// CR 119.3: subtracts `n` from the affected player without clamping and pushes an
    /// applied `LifeChanged`. Zero life lost pushes nothing.
    pub(super) fn resolve_lose_life_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        amount: u32,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(player) = affected_player(top, legal_targets) {
            self.lose_life_now(player, amount, &mut events);
        }
        ControlFlow::Continue(events)
    }

    /// Applies one life loss and records it. Shared with `ForEachPlayer`.
    pub(in crate::engine) fn lose_life_now(
        &mut self,
        player: PlayerId,
        amount: u32,
        events: &mut Vec<GameEventKind>,
    ) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        if delta == 0 {
            return;
        }
        if let Some(p) = self.state.player_mut(player) {
            p.lose_life(delta);
            events.push(GameEventKind::LifeChanged {
                player,
                delta: -delta,
            });
        }
    }

    pub(super) fn resolve_damage_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        amount: u32,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for target in legal_targets {
            events.push(GameEventKind::DamageDealt {
                source: top.source,
                target: *target,
                amount,
                is_combat_damage: false,
            });
        }
        ControlFlow::Continue(events)
    }

    /// CR 701.14: fight (`mutual`) and bite. A two-word spec reads the per-word legal
    /// chunks, so an illegal word voids the effect (CR 701.14b); one word uses the
    /// flattened list. Amounts are read through the layer engine before any event is
    /// pushed so the damage is simultaneous.
    pub(super) fn resolve_fight_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        modal_chunk_legals: &[Vec<Target>],
        mutual: bool,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let object_of = |target: &Target| match target {
            Target::Object(id) => Some(*id),
            _ => None,
        };
        let pair: Option<(ObjectId, ObjectId)> = match &top.target_requirement {
            Some(TargetSpec::Multiple(reqs)) if reqs.len() == 2 => {
                match (
                    modal_chunk_legals.first().map(Vec::as_slice),
                    modal_chunk_legals.get(1).map(Vec::as_slice),
                ) {
                    (Some([first]), Some([second])) => object_of(first).zip(object_of(second)),
                    _ => None,
                }
            }
            Some(TargetSpec::Primitive(req)) if !req.up_to && !req.at_least => {
                match (req.count, legal_targets) {
                    // Two target creatures fight; a bite has no dealer in this shape.
                    (2, [first, second]) if mutual => object_of(first).zip(object_of(second)),
                    // The ability's own source is the first creature.
                    (1, [target]) => object_of(target).map(|id| (top.source, id)),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some((first, second)) = pair else {
            return ControlFlow::Continue(events);
        };

        // CR 701.14b
        let is_creature_on_battlefield = |id: ObjectId| {
            self.state
                .object(id)
                .is_some_and(|object| object.zone == ZoneKind::Battlefield)
                && crate::layers::effective_characteristics(&self.state, &self.script_runtime, id)
                    .card_types
                    .contains(CardTypeSet::CREATURE)
        };
        if !is_creature_on_battlefield(first) || !is_creature_on_battlefield(second) {
            return ControlFlow::Continue(events);
        }

        // Amounts are fixed up front; power of 0 or less, or unresolved, deals none (CR 120.8).
        let power_of = |id: ObjectId| {
            commands::effective_power(&self.state, &self.script_runtime, id)
                .filter(|power| *power > 0)
                .map(|power| power as u32)
        };
        let first_power = power_of(first);
        let second_power = power_of(second);

        let mut push_damage = |source: ObjectId, target: ObjectId, amount: u32| {
            events.push(GameEventKind::DamageDealt {
                source,
                target: Target::Object(target),
                amount,
                is_combat_damage: false,
            });
        };
        if first == second {
            // CR 701.14c: fighting itself is one event of twice its power; a bite never doubles.
            if let Some(power) = first_power {
                push_damage(first, first, if mutual { power * 2 } else { power });
            }
        } else {
            if let Some(power) = first_power {
                push_damage(first, second, power);
            }
            if mutual && let Some(power) = second_power {
                push_damage(second, first, power);
            }
        }
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_counters_effect(
        &mut self,
        legal_targets: &[Target],
        kind: &CounterKind,
        amount: i32,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for target in legal_targets {
            let Target::Object(id) = target else { continue };
            events.push(GameEventKind::CounterAdded {
                object: *id,
                kind: kind.clone(),
                amount,
            });
        }
        ControlFlow::Continue(events)
    }

    /// CR 701.26: flips `id` if it is a battlefield permanent in the opposite state,
    /// pushing the event. Anything else is a no-op so a "becomes tapped" trigger
    /// can't fire (CR 603.2e).
    fn flip_tapped(&mut self, id: ObjectId, tap: bool, events: &mut Vec<GameEventKind>) {
        let Some(object) = self.state.object_mut(id) else {
            return;
        };
        if object.zone != ZoneKind::Battlefield || object.tapped == tap {
            return;
        }
        object.tapped = tap;
        events.push(if tap {
            GameEventKind::Tapped { object: id }
        } else {
            GameEventKind::Untapped { object: id }
        });
    }

    /// CR 701.26: every legal object target; players are skipped.
    pub(super) fn resolve_tap_effect(
        &mut self,
        legal_targets: &[Target],
        tap: bool,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for target in legal_targets {
            let Target::Object(id) = target else { continue };
            self.flip_tapped(*id, tap, &mut events);
        }
        ControlFlow::Continue(events)
    }

    /// CR 701.26: the ability's own source; a stale id finds nothing (CR 400.7).
    pub(super) fn resolve_tap_self_effect(
        &mut self,
        top: &StackObject,
        tap: bool,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        self.flip_tapped(top.source, tap, &mut events);
        ControlFlow::Continue(events)
    }

    /// CR 701.26: every battlefield object matching `filter`, no targets. `filter`
    /// matches the raw `controller` field, not the layer-resolved one.
    pub(super) fn resolve_tap_all_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        tap: bool,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let ids: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Battlefield, None)
            .map(|zone| {
                zone.contents
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.state.object(*id).is_some_and(|object| {
                            matches_effect_filter(filter, object, top.source, top.controller)
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        for id in ids {
            self.flip_tapped(id, tap, &mut events);
        }
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_pump_effect(
        &mut self,
        legal_targets: &[Target],
        power: i32,
        toughness: i32,
        duration: &EffectDuration,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let timestamp = self.state.allocate_sequence_no();
        for target in legal_targets {
            let Target::Object(id) = target else { continue };
            self.state.continuous_effects.push(ContinuousEffect {
                source: *id,
                layer: Layer::PowerToughnessModifying,
                affected: AffectedFilter::Primitive(EffectFilter {
                    controller: None,
                    card_types: None,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: true,
                }),
                effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power, toughness }),
                timestamp,
                duration: duration.clone(),
                condition: None,
            });
        }
        ControlFlow::Continue(events)
    }

    /// CR 509.1h: every legal target becomes blocked via `mark_effect_blocked`
    /// (band propagation, CR 702.22i). Players are skipped.
    pub(super) fn resolve_become_blocked_effect(
        &mut self,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for target in legal_targets {
            let Target::Object(id) = target else { continue };
            commands::mark_effect_blocked(&mut self.state, *id);
        }
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_gain_control_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        duration: &EffectDuration,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let timestamp = self.state.allocate_sequence_no();
        let new_controller = top.controller;
        for target in legal_targets {
            let Target::Object(id) = target else { continue };
            self.state.continuous_effects.push(ContinuousEffect {
                source: *id,
                layer: Layer::ControlChangingEffects,
                affected: AffectedFilter::Primitive(EffectFilter {
                    controller: None,
                    card_types: None,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: true,
                }),
                effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
                timestamp,
                duration: duration.clone(),
                condition: None,
            });
        }
        ControlFlow::Continue(events)
    }

    pub(super) fn resolve_regenerate_effect(
        &mut self,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for target in legal_targets {
            let Target::Object(id) = target else { continue };
            if commands::effective_keywords(&self.state, &self.script_runtime, *id)
                .contains(KeywordSet::CANT_BE_REGENERATED)
            {
                continue;
            }
            self.state.replacement_effects.push(ReplacementEffect {
                source: *id,
                spec: ReplacementSpec::Primitive {
                    filter: EventFilter::WouldDie {
                        affected: EffectFilter {
                            controller: None,
                            card_types: None,
                            subtypes: None,
                            excluded_subtypes: None,
                            supertypes: None,
                            excluded_supertypes: None,
                            self_only: true,
                        },
                    },
                    payload: ReplacementPayload::Regenerate,
                },
                consumption: ConsumptionPolicy::OneShot,
            });
        }
        ControlFlow::Continue(events)
    }
}
