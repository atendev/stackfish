use std::collections::HashMap;

use super::*;
use super::attach::retire_object;
use crate::types::ability::{AbilityEffect, AbilityKind, RegisteredAbility};
use crate::types::card::ModeChoice;
use crate::types::counter::CounterSet;
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload,
    EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::game::GameState;
use crate::types::game::RngState;
use crate::types::ids::{AbilityId, ObjectId, PlayerId, ScriptHandle, SequenceNo};
use crate::types::mana::ManaPool;
use crate::types::player::PlayerState;
use crate::types::turn::{Phase, Step};

fn minimal_state() -> GameState {
    let p0 = PlayerId(0);
    GameState {
        players: vec![PlayerState {
            player_id: p0,
            life: 20,
            mana_pool: ManaPool::default(),
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: std::collections::HashMap::new(),
        }],
        turn_order: vec![p0],
        active_player: p0,
        turn_number: 1,
        phase: Phase::PreGame,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: Vec::new(),
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        hand_reveal_modifiers: Vec::new(),
        draw_limit_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 0,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
        linked_ability_generation: std::collections::HashMap::new(),
    }
}

fn dummy_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn dummy_continuous_effect(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter { 
            controller: None,
            card_types: None,
            self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

/// `phase5/static-regeneration`: a `Repeatable` `WouldDie`/`Regenerate`
/// shield, the shape `Game.CreateReplacementEffect` produces — the first
/// durable `state.replacement_effects` entry that ever needed
/// `retire_object` to purge it (every prior entry was `OneShot`/
/// `LimitedUses` and self-consumed via `apply_one_replacement`).
fn dummy_replacement_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::Regenerate,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn retire_object_purges_only_the_retired_source() {
    let mut state = minimal_state();
    state.abilities.push(dummy_ability(ObjectId(1)));
    state.abilities.push(dummy_ability(ObjectId(2)));
    state.continuous_effects.push(dummy_continuous_effect(ObjectId(1)));
    state.continuous_effects.push(dummy_continuous_effect(ObjectId(2)));
    state.replacement_effects.push(dummy_replacement_effect(ObjectId(1)));
    state.replacement_effects.push(dummy_replacement_effect(ObjectId(2)));

    let departed = retire_object(&mut state, ObjectId(1));

    assert_eq!(departed.len(), 1, "the retired object's own Triggered ability is returned");
    assert_eq!(departed[0].source, ObjectId(1));
    assert_eq!(state.abilities.len(), 1, "only ObjectId(1)'s ability should be purged");
    assert_eq!(state.abilities[0].source, ObjectId(2), "ObjectId(2)'s ability must survive untouched");
    assert_eq!(
        state.continuous_effects.len(),
        1,
        "only ObjectId(1)'s continuous effect should be purged"
    );
    assert_eq!(
        state.continuous_effects[0].source,
        ObjectId(2),
        "ObjectId(2)'s continuous effect must survive untouched"
    );
    assert_eq!(
        state.replacement_effects.len(),
        1,
        "only ObjectId(1)'s replacement effect should be purged"
    );
    assert_eq!(
        state.replacement_effects[0].source,
        ObjectId(2),
        "ObjectId(2)'s replacement effect must survive untouched"
    );
}
