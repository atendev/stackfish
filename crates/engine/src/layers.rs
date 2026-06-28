use crate::types::effect::EffectiveCharacteristics;
use crate::types::game::GameState;
use crate::types::ids::ObjectId;

/// The continuous-effect layer engine's query (CR 613): computes `object`'s
/// live characteristics from its `CardDefinition` base values plus every
/// applicable `ContinuousEffect` in `state.continuous_effects`, applied in
/// layer order with timestamp/dependency ordering within each layer.
/// Recomputed on every call rather than cached — see the no-mutable-cache
/// rationale on `EffectiveCharacteristics` itself.
///
/// Resolving a `PTValue::CharacteristicDefining` value (layer 7a) requires
/// invoking the card's script callback, which means this signature will
/// likely need a script-runtime handle threaded through once that exists —
/// not added yet since the script bridge hasn't been designed.
pub fn effective_characteristics(state: &GameState, object: ObjectId) -> EffectiveCharacteristics {
    todo!()
}
