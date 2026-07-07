use crate::types::effect::{ContinuousEffect, EffectPayload, EffectPrimitive, EffectiveCharacteristics, Layer};
use crate::types::game::GameState;
use crate::types::ids::ObjectId;
use crate::types::object::GameObject;

/// The continuous-effect layer engine's query (CR 613): computes `object`'s
/// live characteristics from its `CardDefinition` base values plus every
/// applicable `ContinuousEffect` in `state.continuous_effects`, applied in
/// layer order with timestamp ordering within each layer. Recomputed on
/// every call rather than cached — see the no-mutable-cache rationale on
/// `EffectiveCharacteristics` itself.
///
/// Layers 4 (`TypeChanging`), 5 (`ColorChanging`), 6 (`AbilityAdding`), 7b
/// (`PowerToughnessSetting`), and 7c (`PowerToughnessModifying`) are
/// applied — see `Layer`'s own doc comment for why the rest (copy/control/
/// text, plus 7a/7d/7e) aren't built yet. Layer 7d (counters) is folded in
/// from `GameObject.counters` directly via `PTValue::net_with_counters`
/// before any continuous effect is applied, same as `sba.rs`'s pre-existing
/// toughness check already did — this is a deliberate simplification, not
/// CR 613.4's literal timestamp-
/// interleaving of counters alongside other layer-7 effects (flagged, not
/// silently wrong: it gives the same answer as long as nothing on the board
/// needs counters and a `SetPT`/`ModifyPT` effect ordered relative to each
/// other by timestamp — which no test in this slice exercises).
///
/// Layer 7a (characteristic-defining power/toughness) is not resolved here —
/// same "skip rather than guess" stance `sba.rs` already takes: an object
/// with `PTValue::CharacteristicDefining` power/toughness gets `None` for
/// that field, exactly as `PTValue::as_fixed`/`net_with_counters` already
/// return `None` for it. Resolving a CDA needs the script bridge, not
/// designed into this query yet.
///
/// No `&ScriptRuntime` parameter: nothing this slice can construct
/// (`AffectedFilter::Script`/`EffectPayload::Script`) is actually
/// evaluated — see those types' doc comments. A future slice threading real
/// script-backed continuous effects through here will need to add one.
pub fn effective_characteristics(state: &GameState, object: ObjectId) -> EffectiveCharacteristics {
    let obj = state
        .object(object)
        .unwrap_or_else(|| panic!("effective_characteristics called with unknown ObjectId {object:?}"));
    let def = &obj.definition;

    let mut power = def.power.as_ref().and_then(|p| p.net_with_counters(&obj.counters));
    let mut toughness = def.toughness.as_ref().and_then(|t| t.net_with_counters(&obj.counters));
    let mut keywords = def.keywords;
    let mut card_types = def.card_types;
    // TODO: CardDefinition::colors()/ManaCost::colors() are both still
    // todo!() (see mana.rs) - once filled, derive this from def.colors()
    // instead of reading color_indicator directly (which ignores colors
    // contributed by mana_cost symbols per CR 202.2). Carried forward from
    // the layer-engine-basics slice; this slice's layer 5 union applies on
    // top of whatever this stopgap produces, not blocked on fixing it.
    let mut colors = def.color_indicator.unwrap_or(crate::ColorSet::empty());

    // Layer 4: additive only (CR 613.1, general "gains types" text) - see
    // EffectPrimitive::AddCardType's doc comment for the deferred overwrite
    // shape.
    for effect in applicable_effects_in_layer(state, obj, Layer::TypeChanging) {
        if let EffectPayload::Primitive(EffectPrimitive::AddCardType(added)) = &effect.effect {
            card_types |= *added;
        }
    }

    // Layer 5: additive only, same shape as layer 4 above.
    for effect in applicable_effects_in_layer(state, obj, Layer::ColorChanging) {
        if let EffectPayload::Primitive(EffectPrimitive::AddColor(added)) = &effect.effect {
            colors |= *added;
        }
    }

    for effect in applicable_effects_in_layer(state, obj, Layer::AbilityAdding) {
        if let EffectPayload::Primitive(EffectPrimitive::AddKeyword(added)) = &effect.effect {
            keywords |= *added;
        }
    }

    // Layer 7b: setting replaces whatever came before this layer entirely -
    // later timestamp wins outright, it doesn't stack with an earlier SetPT.
    for effect in applicable_effects_in_layer(state, obj, Layer::PowerToughnessSetting) {
        if let EffectPayload::Primitive(EffectPrimitive::SetPT { power: p, toughness: t }) = &effect.effect {
            power = Some(*p);
            toughness = Some(*t);
        }
    }

    // Layer 7c: modifying is additive across every applicable effect, in
    // timestamp order (order doesn't actually change the sum for a plain
    // add, but the sort is kept anyway - see applicable_effects_in_layer's
    // doc comment on why the sort lives there regardless).
    for effect in applicable_effects_in_layer(state, obj, Layer::PowerToughnessModifying) {
        if let EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: dp, toughness: dt }) = &effect.effect {
            power = power.map(|p| p + dp);
            toughness = toughness.map(|t| t + dt);
        }
    }

    EffectiveCharacteristics {
        name: def.name.clone(),
        mana_cost: def.mana_cost.clone(),
        colors,
        card_types,
        supertypes: def.supertypes,
        subtypes: def.subtypes.clone(),
        keywords,
        power,
        toughness,
        loyalty: def.loyalty.as_ref().and_then(|l| l.as_fixed()),
        defense: def.defense.as_ref().and_then(|d| d.as_fixed()),
    }
}

/// Every `ContinuousEffect` in `layer` whose `affected` filter matches
/// `object`, sorted by `timestamp` (CR 613.4's ordering within a layer — see
/// `Layer`'s doc comment for why this is timestamp-only, no dependency
/// detection). Sorting here rather than at each call site keeps that
/// contract in one place regardless of which layer's caller in
/// `effective_characteristics` is walking it.
fn applicable_effects_in_layer<'a>(
    state: &'a GameState,
    object: &GameObject,
    layer: Layer,
) -> Vec<&'a ContinuousEffect> {
    let mut effects: Vec<&ContinuousEffect> = state
        .continuous_effects
        .iter()
        .filter(|e| e.layer == layer)
        .filter(|e| {
            // An effect whose own source has already left the battlefield
            // (and has no remap - see commands::retire_object) can't resolve
            // a controller to check `You`/`Opponent` against. Skip rather
            // than guess, same stance collect_triggers already takes for a
            // trigger whose source no longer resolves (CR 603.10 gap,
            // flagged there too).
            state.object(e.source).is_some_and(|source| e.applies_to(object, source))
        })
        .collect();
    effects.sort_by_key(|e| e.timestamp);
    effects
}
