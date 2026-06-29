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
    // TODO: 1. Look up object's GameObject (state.object(object)) and start
    //          from its CardDefinition's base values (copiable values, CR
    //          613.2).
    //       2. Collect every ContinuousEffect in state.continuous_effects
    //          where ContinuousEffect::applies_to(object's GameObject) is
    //          true (that method itself isn't implementable yet - needs a
    //          ScriptRuntime, see effect.rs's TODO).
    //       3. Group by Layer, sort within each layer by timestamp (and
    //          dependency order where effects depend on each other's
    //          results - CR 613.6-613.8's dependency rule isn't designed
    //          yet at all, this will need real thought, not just a sort).
    //       4. Apply each layer's effects in order (1 through 7e) to build
    //          up the final EffectiveCharacteristics.
    //       5. For power.toughness/loyalty/defense starting from
    //          PTValue::CharacteristicDefining (layer 7a), invoke the card's
    //          CDA script callback via ScriptRuntime - needs a
    //          &ScriptRuntime parameter added to this signature.
    todo!()
}
