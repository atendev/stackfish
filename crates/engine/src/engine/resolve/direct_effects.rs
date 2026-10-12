//! `resolve_ability_effect`'s synchronous-only `AbilityEffect` arms — the
//! ones that mutate `GameState`/push events and fall straight through to the
//! shared tail, never pausing mid-resolution: `GainLife`, `Damage`,
//! `Fight`, `Bite`, `Counters`, `Pump`, `GainControl`, `Regenerate`, `LoseLife`, and the tap/untap family
//! (`Tap`, `Untap`, `TapSelf`, `UntapSelf`, `TapAll`, `UntapAll`). `Modal` lives in `modal.rs`.

use std::ops::ControlFlow;

use crate::commands;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::target::TargetSpec;
use crate::types::effect::{
    matches_effect_filter, AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter,
    EffectPayload, EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, StepStatus};

/// Which player a `Mill`/`LoseLife` acts on. Decided by whether the ability
/// declared a target spec, not by whether `legal_targets` is empty: with no
/// spec the player is the controller ("you mill 2"); with a spec it is the
/// first legal `Target::Player` ("target player mills 2"), and `None` when
/// that target is gone or the spec chose nothing, so a target that became
/// illegal never turns into a controller effect.
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

    /// CR 119.3: `Game.Ops.LoseLife(n)`. Subtracts `n` from the affected
    /// player (see `affected_player`) without clamping, and pushes a
    /// `LifeChanged` event already applied to state. Zero life lost is not a
    /// life change, so it pushes nothing.
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

    /// Applies one life loss and records it. Shared with
    /// `ForEachPlayer { LoseLife }`.
    pub(in crate::engine) fn lose_life_now(&mut self, player: PlayerId, amount: u32, events: &mut Vec<GameEventKind>) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        if delta == 0 {
            return;
        }
        if let Some(p) = self.state.player_mut(player) {
            p.lose_life(delta);
            events.push(GameEventKind::LifeChanged { player, delta: -delta });
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

    /// CR 701.14 fight / bite: `Game.Ops.Fight` (`mutual: true`) and
    /// `Game.Ops.Bite` (`mutual: false`). See `AbilityEffect::Fight` for the
    /// three target-spec shapes that pick the two creatures.
    ///
    /// The pair comes from the per-word legal chunks (`modal_chunk_legals`)
    /// for a two-word spec, so an illegal word leaves its chunk empty and
    /// the whole effect does nothing (CR 701.14b). A single word uses the
    /// flattened `legal_targets`, which only holds the still-legal picks.
    /// Both creatures must still be creatures on the battlefield. Each
    /// amount is read through the layer engine before any event is pushed,
    /// then the events go out together so the damage is simultaneous and
    /// every keyword reads the creature as the damage source.
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
                match (modal_chunk_legals.first().map(Vec::as_slice), modal_chunk_legals.get(1).map(Vec::as_slice)) {
                    (Some([first]), Some([second])) => object_of(first).zip(object_of(second)),
                    _ => None,
                }
            }
            Some(TargetSpec::Primitive(req)) if !req.up_to && !req.at_least => match (req.count, legal_targets) {
                // "Two target creatures fight each other." A bite has no
                // dealer in this shape.
                (2, [first, second]) if mutual => object_of(first).zip(object_of(second)),
                // "It fights / deals damage to target creature": the
                // ability's own source is the first creature.
                (1, [target]) => object_of(target).map(|id| (top.source, id)),
                _ => None,
            },
            _ => None,
        };
        let Some((first, second)) = pair else { return ControlFlow::Continue(events) };

        // CR 701.14b: both must still be creatures on the battlefield.
        let is_creature_on_battlefield = |id: ObjectId| {
            self.state.object(id).is_some_and(|object| object.zone == ZoneKind::Battlefield)
                && crate::layers::effective_characteristics(&self.state, &self.script_runtime, id)
                    .card_types
                    .contains(CardTypeSet::CREATURE)
        };
        if !is_creature_on_battlefield(first) || !is_creature_on_battlefield(second) {
            return ControlFlow::Continue(events);
        }

        // Both amounts are fixed before any event goes out. Power of 0 or
        // less, or an unresolved power, deals no damage (CR 120.8).
        let power_of = |id: ObjectId| {
            commands::effective_power(&self.state, &self.script_runtime, id).filter(|power| *power > 0).map(|power| power as u32)
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
            // CR 701.14c: fighting itself is one event of twice its power.
            // Bite never doubles: it is a single damage instance.
            if let Some(power) = first_power {
                push_damage(first, first, if mutual { power * 2 } else { power });
            }
        } else {
            if let Some(power) = first_power {
                push_damage(first, second, power);
            }
            if mutual {
                if let Some(power) = second_power {
                    push_damage(second, first, power);
                }
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
            events.push(GameEventKind::CounterAdded { object: *id, kind: kind.clone(), amount });
        }
        ControlFlow::Continue(events)
    }

    /// CR 701.26a/b: flips `id` to `tap` (true = tapped) if it is a
    /// battlefield permanent currently in the opposite state, and pushes the
    /// matching `Tapped`/`Untapped` event. Anything else is a no-op with no
    /// event ("only untapped permanents can be tapped", "only tapped
    /// permanents can be untapped"; CR 110.5d: cards off the battlefield
    /// have no tapped status), so a later "becomes tapped" trigger can't fire
    /// on a no-op (CR 603.2e).
    fn flip_tapped(&mut self, id: ObjectId, tap: bool, events: &mut Vec<GameEventKind>) {
        let Some(object) = self.state.object_mut(id) else { return };
        if object.zone != ZoneKind::Battlefield || object.tapped == tap {
            return;
        }
        object.tapped = tap;
        events.push(if tap { GameEventKind::Tapped { object: id } } else { GameEventKind::Untapped { object: id } });
    }

    /// CR 701.26: `Game.Ops.Tap` / `Game.Ops.Untap` - every legal object
    /// target of the resolving ability. A `Target::Player` is skipped, same
    /// stance as `Counters`/`Pump`.
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

    /// CR 701.26/400.7: `Game.Ops.TapSelf` / `Game.Ops.UntapSelf` - the
    /// ability's own source. A source that already left the battlefield is a
    /// new object, so the stale id finds nothing.
    pub(super) fn resolve_tap_self_effect(
        &mut self,
        top: &StackObject,
        tap: bool,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        self.flip_tapped(top.source, tap, &mut events);
        ControlFlow::Continue(events)
    }

    /// CR 701.26/601.2c: `Game.Ops.TapAll(filter)` / `UntapAll(filter)` -
    /// every battlefield object matching `filter`, no targets. `filter` is
    /// matched against the raw `controller` field (see
    /// `matches_effect_filter`), not the layer-resolved one.
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
                        self.state
                            .object(*id)
                            .is_some_and(|object| matches_effect_filter(filter, object, top.source, top.controller))
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
                    excluded_subtypes: None, supertypes: None, excluded_supertypes: None,
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

    /// CR 509.1h/702.22i: every legal target independently becomes blocked
    /// via `commands::mark_effect_blocked` — see that function's own doc
    /// comment for the CR 702.22i band-propagation and CR 603.2e idempotency
    /// details. A `Target::Player` (shouldn't be reachable — this effect's
    /// `target_requirement` only ever asks for a creature) is skipped, same
    /// "silent no-op" stance every other arm here takes.
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
                    excluded_subtypes: None, supertypes: None, excluded_supertypes: None,
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
                        affected: EffectFilter {  controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: true  },
                    },
                    payload: ReplacementPayload::Regenerate,
                },
                consumption: ConsumptionPolicy::OneShot,
            });
        }
        ControlFlow::Continue(events)
    }
}
