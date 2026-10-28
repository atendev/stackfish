use crate::script::ScriptRuntime;
use crate::types::ability::InterveningIfSpec;
use crate::types::card::{CardTypeSet, KeywordSet, PTValue, ScriptReference};
use crate::types::counter::CounterKind;
use crate::types::effect::{
    AffectedFilter, ContinuousEffect, EffectPayload, EffectPrimitive, EffectiveCharacteristics, Layer,
};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, SequenceNo};
use crate::types::object::GameObject;

/// CR 613: computes `object`'s live characteristics from its `CardDefinition`
/// plus every applicable `ContinuousEffect`, in layer order and, within a layer,
/// timestamp order. Recomputed on every call; nothing is cached.
///
/// Layers 3, 4, 5, 6, 7a, 7b, 7c and 7e are applied.
///
/// CR 613.4c: `+1/+1` and `-1/-1` counters fold into the 7c pass, interleaved by
/// timestamp with `ModifyPT` and `DoublePT`, so a counter added before a
/// `SetPT` still counts (CR 613.5).
///
/// CR 613.4d: 7e swaps after 7b and 7c settle, so a modify at a later timestamp
/// than the switch still counts before it.
///
/// A missing or failing script yields `None` rather than a guess, which keeps
/// the query infallible.
pub fn effective_characteristics(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
) -> EffectiveCharacteristics {
    let obj = state
        .object(object)
        .unwrap_or_else(|| panic!("effective_characteristics called with unknown ObjectId {object:?}"));
    // CR 613.1a: a copy takes its copiable values from a stored snapshot, not a layer 1 effect.
    let def = obj.copiable_override.as_ref().unwrap_or(&obj.definition);

    let mut power = resolve_base_pt(def.power.as_ref(), state, script_runtime, object, obj.controller);
    let mut toughness = resolve_base_pt(def.toughness.as_ref(), state, script_runtime, object, obj.controller);
    let mut keywords = def.keywords;
    let mut card_types = def.card_types;
    // CR 105.2a, 202.2: a color indicator if set, otherwise the mana cost symbols.
    let mut colors = def.colors();
    // Mutable so SetFullText (CR 612.6) can override them.
    let mut mana_cost = def.mana_cost.clone();
    let mut supertypes = def.supertypes;
    let mut subtypes = def.subtypes.clone();

    // Layer 3: CR 612.8 SetName and CR 612.6 SetFullText share one pass, and the later timestamp overrides.
    let mut name = def.name.clone();
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::TextChanging) {
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::SetName(new_name)) => {
                name = new_name;
            }
            Some(EffectPrimitive::SetFullText(donor)) => {
                name = donor.name.clone();
                mana_cost = donor.mana_cost.clone();
                colors = donor.colors();
                card_types = donor.card_types;
                supertypes = donor.supertypes;
                subtypes = donor.subtypes.clone();
                keywords = donor.keywords;
                power = resolve_base_pt(donor.power.as_ref(), state, script_runtime, object, obj.controller);
                toughness = resolve_base_pt(donor.toughness.as_ref(), state, script_runtime, object, obj.controller);
            }
            _ => {}
        }
    }

    // Layer 4 (CR 613.1d): type changes fold into one timestamp-ordered pass.
    // A Set wipes earlier Adds and later Adds stack on top (CR 613.7).
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::TypeChanging) {
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::AddCardType(added)) => card_types |= added,
            Some(EffectPrimitive::SetCardTypes(new_types)) => card_types = new_types,
            Some(EffectPrimitive::AddSupertype(added)) => supertypes |= added,
            Some(EffectPrimitive::AddSubtype(added)) => {
                for subtype in added {
                    if !subtypes.contains(&subtype) {
                        subtypes.push(subtype);
                    }
                }
            }
            _ => {}
        }
    }

    // Layer 5 (CR 613.1e): same shape as layer 4.
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::ColorChanging) {
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::AddColor(added)) => colors |= added,
            Some(EffectPrimitive::SetColors(new_colors)) => colors = new_colors,
            _ => {}
        }
    }

    // Layer 6 (CR 613.1f): additive and removing effects fold in timestamp order (CR 613.7).
    // RemoveAllAbilities also clears protection, landwalk and banding grants.
    let mut ignored_keywords = KeywordSet::empty();
    let mut protection_colors = crate::ColorSet::empty();
    let mut protection_card_types = CardTypeSet::empty();
    let mut protection_subtypes: Vec<crate::Subtype> = Vec::new();
    let mut protection_names: Vec<String> = Vec::new();
    let mut protection_everything = false;
    let mut protection_players: Vec<crate::types::cost::FilterController> = Vec::new();
    let mut landwalk_subtypes: Vec<crate::Subtype> = Vec::new();
    let mut landwalk_qualities: Vec<crate::types::effect::LandwalkQuality> = Vec::new();
    let mut bands_with_other_subtypes: Vec<crate::Subtype> = Vec::new();
    let mut bands_with_other_supertypes = crate::types::card::SupertypeSet::empty();
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::AbilityAdding) {
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::AddKeyword(added)) => keywords |= added,
            Some(EffectPrimitive::RemoveKeyword(removed)) => keywords &= !removed,
            Some(EffectPrimitive::IgnoreKeyword(ignored)) => ignored_keywords |= ignored,
            Some(EffectPrimitive::AddProtectionColor(added)) => protection_colors |= added,
            Some(EffectPrimitive::AddProtectionCardType(added)) => protection_card_types |= added,
            Some(EffectPrimitive::AddProtectionSubtype(added)) => {
                if !protection_subtypes.contains(&added) {
                    protection_subtypes.push(added);
                }
            }
            Some(EffectPrimitive::AddProtectionName(added)) => {
                if !protection_names.contains(&added) {
                    protection_names.push(added);
                }
            }
            Some(EffectPrimitive::AddProtectionEverything) => protection_everything = true,
            Some(EffectPrimitive::AddProtectionPlayer(added)) => {
                if !protection_players.contains(&added) {
                    protection_players.push(added);
                }
            }
            Some(EffectPrimitive::AddLandwalkSubtype(added)) => {
                if !landwalk_subtypes.contains(&added) {
                    landwalk_subtypes.push(added);
                }
            }
            Some(EffectPrimitive::AddLandwalkQuality(added)) => {
                if !landwalk_qualities.contains(&added) {
                    landwalk_qualities.push(added);
                }
            }
            Some(EffectPrimitive::AddBandsWithOther(added)) => {
                if !bands_with_other_subtypes.contains(&added) {
                    bands_with_other_subtypes.push(added);
                }
            }
            Some(EffectPrimitive::AddBandsWithOtherSupertype(added)) => {
                bands_with_other_supertypes |= added;
            }
            Some(EffectPrimitive::RemoveAllAbilities) => {
                keywords = KeywordSet::empty();
                ignored_keywords = KeywordSet::empty();
                protection_colors = crate::ColorSet::empty();
                protection_card_types = CardTypeSet::empty();
                protection_subtypes = Vec::new();
                protection_names = Vec::new();
                protection_everything = false;
                protection_players = Vec::new();
                landwalk_subtypes = Vec::new();
                landwalk_qualities = Vec::new();
                bands_with_other_subtypes = Vec::new();
                bands_with_other_supertypes = crate::types::card::SupertypeSet::empty();
            }
            _ => {}
        }
    }
    // CR 702.22b: losing banding also loses every "bands with other" ability.
    // Gated on the printed keyword so a granted "bands with other" on an object that never had banding survives.
    if def.keywords.contains(KeywordSet::BANDING) && !keywords.contains(KeywordSet::BANDING) {
        bands_with_other_subtypes = Vec::new();
        bands_with_other_supertypes = crate::types::card::SupertypeSet::empty();
    }

    // Layer 7b: the later timestamp overrides outright.
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessSetting) {
        if let Some(EffectPrimitive::SetPT { power: p, toughness: t }) =
            resolve_primitive(effect, state, script_runtime, obj.controller)
        {
            power = Some(p);
            toughness = Some(t);
        }
    }

    // Layer 7c (CR 613.4c): modifying is additive, and `applicable_effects_in_layer`
    // already orders `ModifyPT` before a dependent `DoublePT` (CR 613.8a).
    // Counters merge in by timestamp against that order without being re-sorted into it.
    let modify_effects = applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessModifying);
    let mut counter_deltas: Vec<(SequenceNo, i32, i32)> = obj
        .counters
        .batches(&CounterKind::PlusOnePlusOne)
        .map(|(amount, ts)| (ts, amount, amount))
        .chain(obj.counters.batches(&CounterKind::MinusOneMinusOne).map(|(amount, ts)| (ts, -amount, -amount)))
        .collect();
    counter_deltas.sort_by_key(|(ts, _, _)| *ts);
    let mut next_counter = 0;
    let apply_counters_up_to = |power: &mut Option<i32>, toughness: &mut Option<i32>, next_counter: &mut usize, before: SequenceNo| {
        while *next_counter < counter_deltas.len() && counter_deltas[*next_counter].0 < before {
            let (_, dp, dt) = counter_deltas[*next_counter];
            *power = power.map(|p| p + dp);
            *toughness = toughness.map(|t| t + dt);
            *next_counter += 1;
        }
    };
    for effect in modify_effects {
        apply_counters_up_to(&mut power, &mut toughness, &mut next_counter, effect.timestamp);
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::ModifyPT { power: dp, toughness: dt }) => {
                power = power.map(|p| p + dp);
                toughness = toughness.map(|t| t + dt);
            }
            Some(EffectPrimitive::DoublePT { power: double_power, toughness: double_toughness }) => {
                if double_power {
                    power = power.map(|p| p * 2);
                }
                if double_toughness {
                    toughness = toughness.map(|t| t * 2);
                }
            }
            _ => {}
        }
    }
    apply_counters_up_to(&mut power, &mut toughness, &mut next_counter, SequenceNo(u64::MAX));

    // Layer 7e (CR 613.4d): skipped when either side is unresolved.
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessSwitching) {
        if matches!(resolve_primitive(effect, state, script_runtime, obj.controller), Some(EffectPrimitive::SwitchPT)) {
            if let (Some(p), Some(t)) = (power, toughness) {
                power = Some(t);
                toughness = Some(p);
            }
        }
    }

    EffectiveCharacteristics {
        name,
        mana_cost,
        colors,
        card_types,
        supertypes,
        subtypes,
        keywords,
        ignored_keywords,
        protection_colors,
        protection_card_types,
        protection_subtypes,
        protection_names,
        protection_everything,
        protection_players,
        landwalk_subtypes,
        landwalk_qualities,
        bands_with_other_subtypes,
        bands_with_other_supertypes,
        power,
        toughness,
        loyalty: def.loyalty.as_ref().and_then(|l| l.as_fixed()),
        defense: def.defense.as_ref().and_then(|d| d.as_fixed()),
    }
}

/// CR 613.1b: `object`'s live controller, the last applicable `GainControl`
/// in layer 2 over `GameObject.controller`.
///
// TODO: read the effective controller at the combat, targeting, cost-filter and summoning-sickness sites
pub fn effective_controller(state: &GameState, script_runtime: &ScriptRuntime, object: ObjectId) -> PlayerId {
    let obj = state
        .object(object)
        .unwrap_or_else(|| panic!("effective_controller called with unknown ObjectId {object:?}"));
    let mut controller = obj.controller;

    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::ControlChangingEffects) {
        if let Some(EffectPrimitive::GainControl(new_controller)) =
            resolve_primitive(effect, state, script_runtime, controller)
        {
            controller = new_controller;
        }
    }

    controller
}

/// Every `ContinuousEffect` in `layer` whose `affected` filter matches
/// `object`, ordered by timestamp (CR 613.7) with dependencies applied (CR 613.8).
///
/// `AffectedFilter::Script` is evaluated here so `types/effect.rs` need not
/// depend on `ScriptRuntime`. An erroring script means "does not match".
///
/// CR 611.2c: an effect whose condition does not hold drops out for this call.
/// "You" is the source's raw controller, because `effective_controller` calls
/// this function.
fn applicable_effects_in_layer<'a>(
    state: &'a GameState,
    script_runtime: &ScriptRuntime,
    object: &GameObject,
    layer: Layer,
) -> Vec<&'a ContinuousEffect> {
    let effects: Vec<&ContinuousEffect> = state
        .continuous_effects
        .iter()
        .filter(|e| e.layer == layer)
        .filter(|e| {
            // A source that no longer resolves cannot supply a controller, so the effect is skipped.
            let Some(source_obj) = state.object(e.source) else { return false };
            // Script errors are not logged: this hot query path holds only `&GameState`.
            let condition_holds = match &e.condition {
                None => true,
                Some(InterveningIfSpec::Primitive(p)) => p.holds(state, source_obj.controller, e.source, None),
                Some(InterveningIfSpec::Script(handle)) => script_runtime
                    .invoke_intervening_if(*handle, state, e.source, source_obj.controller)
                    .unwrap_or(false),
            };
            if !condition_holds {
                return false;
            }
            match &e.affected {
                // CR 303.4m
                AffectedFilter::Primitive(_)
                | AffectedFilter::EnchantedOnly
                | AffectedFilter::EnchantedPlayerControls => e.applies_to(object, source_obj),
                AffectedFilter::Script(handle) => {
                    let enchanted = source_obj.attached_to;
                    script_runtime
                        .invoke_continuous_affects(
                            *handle,
                            state,
                            object.object_id,
                            e.source,
                            source_obj.controller,
                            enchanted,
                        )
                        .unwrap_or(false)
                }
            }
        })
        .collect();
    order_by_dependency(effects, state, script_runtime, object.controller)
}

/// CR 613.8: within one layer, a dependent effect applies after the effect it
/// depends on, and otherwise timestamp order holds. A dependency loop falls
/// back to timestamp order (CR 613.8b). Only `DoublePT` on a same-field modify
/// can depend on another effect.
fn order_by_dependency<'a>(
    mut remaining: Vec<&'a ContinuousEffect>,
    state: &GameState,
    script_runtime: &ScriptRuntime,
    controller: PlayerId,
) -> Vec<&'a ContinuousEffect> {
    let mut placed: Vec<&ContinuousEffect> = Vec::with_capacity(remaining.len());

    while !remaining.is_empty() {
        let ready: Vec<usize> = (0..remaining.len())
            .filter(|&i| {
                (0..remaining.len())
                    .all(|j| i == j || !depends_on(remaining[i], remaining[j], state, script_runtime, controller))
            })
            .collect();

        if ready.is_empty() {
            // CR 613.8b: a dependency loop ignores dependency.
            remaining.sort_by_key(|e| e.timestamp);
            placed.extend(remaining);
            return placed;
        }

        let next = *ready.iter().min_by_key(|&&i| remaining[i].timestamp).unwrap();
        placed.push(remaining.remove(next));
    }

    placed
}

/// CR 613.8a: does `effect` depend on `other`? Both share a layer. Script
/// payloads resolve through `resolve_primitive` so they take part too, and an
/// unresolvable one reports no dependency.
fn depends_on(
    effect: &ContinuousEffect,
    other: &ContinuousEffect,
    state: &GameState,
    script_runtime: &ScriptRuntime,
    controller: PlayerId,
) -> bool {
    let (Some(this), Some(that)) = (
        resolve_primitive(effect, state, script_runtime, controller),
        resolve_primitive(other, state, script_runtime, controller),
    ) else {
        return false;
    };
    match (this, that) {
        (EffectPrimitive::DoublePT { power, toughness }, EffectPrimitive::ModifyPT { .. }) => power || toughness,
        (
            EffectPrimitive::DoublePT { power: p1, toughness: t1 },
            EffectPrimitive::DoublePT { power: p2, toughness: t2 },
        ) => (p1 && p2) || (t1 && t2),
        _ => false,
    }
}

/// Resolves a `ContinuousEffect` to its `EffectPrimitive`, invoking the script
/// for a `Script` payload. An erroring script yields `None` and is not logged,
/// for the reason given on `applicable_effects_in_layer`.
fn resolve_primitive(
    effect: &ContinuousEffect,
    state: &GameState,
    script_runtime: &ScriptRuntime,
    controller: PlayerId,
) -> Option<EffectPrimitive> {
    match &effect.effect {
        EffectPayload::Primitive(p) => Some(p.clone()),
        EffectPayload::Script(handle) => {
            // CR 303.4m: the enchanted permanent's id is passed so a script can reference it.
            let enchanted = state.object(effect.source).and_then(|o| o.attached_to);
            script_runtime
                .invoke_continuous_effect(*handle, state, effect.source, controller, enchanted)
                .ok()
                .flatten()
        }
    }
}

/// CR 613.4a: resolves `value`'s base number from the printed value or a
/// characteristic-defining script. Counters are not included; layer 7c adds them.
/// A missing script, an erroring script or a `ByKey` reference yields `None`.
fn resolve_base_pt(
    value: Option<&PTValue>,
    state: &GameState,
    script_runtime: &ScriptRuntime,
    source: ObjectId,
    controller: PlayerId,
) -> Option<i32> {
    if let Some(PTValue::CharacteristicDefining { script: Some(ScriptReference::Inline(source_code)), .. }) =
        value
    {
        return script_runtime.invoke_cda(source_code, state, source, controller).ok();
    }
    value.and_then(|v| v.as_fixed())
}
