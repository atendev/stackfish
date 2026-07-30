//! `resolve_ability_effect`'s synchronous-only `AbilityEffect` arms — the
//! ones that mutate `GameState`/push events and fall straight through to the
//! shared tail, never pausing mid-resolution: `Modal`, `GainLife`, `Damage`,
//! `Counters`, `Pump`, `GainControl`, `Regenerate`. `Modal` belongs here
//! since it's a miniature re-implementation of these same six kinds inline
//! (see its own doc comment below).

use std::ops::ControlFlow;

use crate::commands;
use crate::types::ability::AbilityEffect;
use crate::types::card::KeywordSet;
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload,
    EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::StackObject;

use super::super::{Engine, StepStatus};

impl Engine {
    /// CR 700.2d — 2+ chosen modes, combined. See `AbilityEffect::Modal`'s
    /// own doc comment for the shape invariant this relies on (one entry per
    /// chosen mode, `modal_chunk_legals` precomputed by the caller with one
    /// legal-target chunk per entry). Narrowed to the synchronous subset
    /// that never pauses mid-resolution (GainLife/Damage/Counters/Pump/
    /// GainControl/Shuffle) - `commands::execute_command`'s `CastSpell` arm
    /// already rejects combining any other effect kind into a Modal at cast
    /// time, so an unsupported kind reaching the fallback arm below would
    /// mean that check has a gap; skip rather than guess/panic, same stance
    /// every other unreachable shape in this engine already takes.
    pub(super) fn resolve_modal_effect(
        &mut self,
        top: &StackObject,
        effects: &[Option<AbilityEffect>],
        modal_chunk_legals: &[Vec<Target>],
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for (effect, legal) in effects.iter().zip(modal_chunk_legals.iter()) {
            let Some(effect) = effect else { continue };
            match effect {
                AbilityEffect::GainLife(amount) => {
                    if let Some(player) = self.state.player_mut(top.controller) {
                        player.gain_life(*amount);
                    }
                }
                AbilityEffect::Damage(amount) => {
                    for target in legal {
                        events.push(GameEventKind::DamageDealt {
                            source: top.source,
                            target: *target,
                            amount: *amount,
                            is_combat_damage: false,
                        });
                    }
                }
                AbilityEffect::Counters { kind, amount } => {
                    for target in legal {
                        let Target::Object(id) = target else { continue };
                        events.push(GameEventKind::CounterAdded {
                            object: *id,
                            kind: kind.clone(),
                            amount: *amount,
                        });
                    }
                }
                AbilityEffect::Pump { power, toughness, duration } => {
                    let timestamp = self.state.allocate_sequence_no();
                    for target in legal {
                        let Target::Object(id) = target else { continue };
                        self.state.continuous_effects.push(ContinuousEffect {
                            source: *id,
                            layer: Layer::PowerToughnessModifying,
                            affected: AffectedFilter::Primitive(EffectFilter { 
                                controller: None,
                                card_types: None,
                                subtypes: None,
                                excluded_subtypes: None,
                                self_only: true,
                             }),
                            effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT {
                                power: *power,
                                toughness: *toughness,
                            }),
                            timestamp,
                            duration: duration.clone(),
                            condition: None,
                        });
                    }
                }
                AbilityEffect::GainControl(duration) => {
                    let timestamp = self.state.allocate_sequence_no();
                    let new_controller = top.controller;
                    for target in legal {
                        let Target::Object(id) = target else { continue };
                        self.state.continuous_effects.push(ContinuousEffect {
                            source: *id,
                            layer: Layer::ControlChangingEffects,
                            affected: AffectedFilter::Primitive(EffectFilter { 
                                controller: None,
                                card_types: None,
                                subtypes: None,
                                excluded_subtypes: None,
                                self_only: true,
                             }),
                            effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
                            timestamp,
                            duration: duration.clone(),
                            condition: None,
                        });
                    }
                }
                AbilityEffect::Shuffle => {
                    self.shuffle_library(top.controller);
                }
                // Unsupported within Modal (see this
                // arm's own doc comment) - skip
                // rather than guess.
                _ => {}
            }
        }
        ControlFlow::Continue(events)
    }

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
                    excluded_subtypes: None,
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
                        affected: EffectFilter {  controller: None, card_types: None, subtypes: None, excluded_subtypes: None, self_only: true  },
                    },
                    payload: ReplacementPayload::Regenerate,
                },
                consumption: ConsumptionPolicy::OneShot,
            });
        }
        ControlFlow::Continue(events)
    }
}
