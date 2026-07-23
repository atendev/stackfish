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

/// The continuous-effect layer engine's query (CR 613): computes `object`'s
/// live characteristics from its `CardDefinition` base values plus every
/// applicable `ContinuousEffect` in `state.continuous_effects`, applied in
/// layer order with timestamp ordering within each layer. Recomputed on
/// every call rather than cached — see the no-mutable-cache rationale on
/// `EffectiveCharacteristics` itself.
///
/// Layer 3 (`TextChanging`, narrowed to CR 612.8 name-setting only), 4
/// (`TypeChanging`), 5 (`ColorChanging`), 6 (`AbilityAdding`), 7a
/// (`PowerToughnessCDA`), 7b (`PowerToughnessSetting`), 7c
/// (`PowerToughnessModifying`), and 7e (`PowerToughnessSwitching`) are
/// applied — see `Layer`'s own doc comment for why the rest (copy/control)
/// aren't built yet.
///
/// Layer 7d/CR 613.4c's `+1/+1`/`-1/-1` counters are folded into the same
/// 7c pass as `ModifyPT`/`DoublePT`, interleaved by timestamp against them
/// (`CounterSet::batches`) — matching the CR's own literal text ("Effects
/// *and counters* that modify power and/or toughness ... are applied" in
/// the same sublayer) rather than pre-netting counters into the layer 7a
/// base the way an earlier version of this function did. That earlier
/// shape was a real bug, not just an approximation: CR 613.5's own Gray
/// Ogre example shows a `+1/+1` counter added *before* a `SetPT`
/// ("becomes 0/1") effect must still contribute *after* it — pre-netting
/// baked the counter into a base that `SetPT`'s override then discarded
/// outright, silently losing it. `resolve_base_pt` no longer touches
/// counters at all (CDA/printed base only); see the 7c loop below for
/// where they're read back in.
///
/// 7e is applied as its own pass *after* 7b/7c have fully resolved, not
/// timestamp-interleaved with them — this is what makes CR 613.4d's own
/// worked example (a modify effect at a later timestamp than a switch still
/// contributes to the *pre-switch* sum) fall out correctly for free: every
/// `ModifyPT`/counter in 7c is summed into one number regardless of a 7e
/// effect's relative timestamp, then 7e's swap(s) apply once each, in
/// timestamp order, atop that already-settled result.
///
/// Layer 7a (characteristic-defining power/toughness, CR 613.4a) is
/// resolved via `resolve_base_pt` below, *before* 7b/7c apply — a `SetPT`
/// still overrides a CDA base, a `ModifyPT` still stacks on top of it, per
/// CR 613.4a→b→c's real ordering. A `CharacteristicDefining` value with no
/// script, or whose script errors, still resolves to `None` — same "skip
/// rather than guess" stance every other unresolvable value in this engine
/// already takes, kept infallible here rather than making this whole query
/// (and every caller) return a `Result`.
///
/// `&ScriptRuntime` is threaded through for layer 7a (`resolve_base_pt`) and,
/// as of the continuous-effects-script slice, also for `AffectedFilter::Script`
/// (evaluated in `applicable_effects_in_layer`) and `EffectPayload::Script`
/// (evaluated via `resolve_primitive` below, called from each per-layer loop)
/// — see docs/implementation.md's continuous-effects-script slice notes for
/// the calling convention (an affects-predicate takes `(object, source,
/// controller) -> bool`; an effect-shape decode takes `(source, controller)
/// -> table`, same argument shape `invoke_cda`/`invoke_operation` already
/// use, decoding one of seven recognized keys into an `EffectPrimitive`).
pub fn effective_characteristics(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
) -> EffectiveCharacteristics {
    let obj = state
        .object(object)
        .unwrap_or_else(|| panic!("effective_characteristics called with unknown ObjectId {object:?}"));
    // CR 613.1a/707 (layer 1): a copy's copiable values come from the
    // frozen `copiable_override` snapshot, not the object's own printed
    // `definition`, when one is present - see `GameObject.copiable_override`'s
    // own doc comment for why this is a stored snapshot rather than a layer
    // 1 `ContinuousEffect` query like every layer below.
    let def = obj.copiable_override.as_ref().unwrap_or(&obj.definition);

    let mut power = resolve_base_pt(def.power.as_ref(), state, script_runtime, object, obj.controller);
    let mut toughness = resolve_base_pt(def.toughness.as_ref(), state, script_runtime, object, obj.controller);
    let mut keywords = def.keywords;
    let mut card_types = def.card_types;
    // CR 105.2a/202.2: color comes from color_indicator if set, otherwise
    // the mana cost's own colored symbols - CardDefinition::colors() already
    // implements exactly that split. Previously this read color_indicator
    // alone (a stopgap from when CardDefinition::colors()/ManaCost::colors()
    // were still todo!() stubs) - real color, printed mana cost symbols
    // included, now derived correctly.
    let mut colors = def.colors();

    // Layer 3 (CR 612.8): override-wins-outright, same shape as layer 2's
    // GainControl/layer 7b's SetPT - a later-timestamp SetName replaces an
    // earlier one entirely, doesn't combine.
    let mut name = def.name.clone();
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::TextChanging) {
        if let Some(EffectPrimitive::SetName(new_name)) = resolve_primitive(effect, state, script_runtime, obj.controller) {
            name = new_name;
        }
    }

    // Layer 4: additive only (CR 613.1, general "gains types" text) - see
    // EffectPrimitive::AddCardType's doc comment for the deferred overwrite
    // shape.
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::TypeChanging) {
        if let Some(EffectPrimitive::AddCardType(added)) = resolve_primitive(effect, state, script_runtime, obj.controller) {
            card_types |= added;
        }
    }

    // Layer 5: additive only, same shape as layer 4 above.
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::ColorChanging) {
        if let Some(EffectPrimitive::AddColor(added)) = resolve_primitive(effect, state, script_runtime, obj.controller) {
            colors |= added;
        }
    }

    // Layer 6 (CR 613.1f): additive (AddKeyword/AddProtectionColor/
    // AddProtectionCardType/AddProtectionSubtype/AddProtectionName/
    // AddProtectionEverything/AddProtectionPlayer) and removing
    // (RemoveAllAbilities) effects fold in the same CR 613.7 timestamp order
    // `applicable_effects_in_layer` already returns - a RemoveAllAbilities
    // clears whatever's accumulated so far in all seven fields (protection
    // is an ability too, CR 613.1f), but a later-timestamp add-shape still
    // applies on top of that, same as a real strip-then-regrant interaction
    // (see RemoveAllAbilities's own doc comment).
    let mut protection_colors = crate::ColorSet::empty();
    let mut protection_card_types = CardTypeSet::empty();
    let mut protection_subtypes: Vec<crate::Subtype> = Vec::new();
    let mut protection_names: Vec<String> = Vec::new();
    let mut protection_everything = false;
    let mut protection_players: Vec<crate::types::cost::FilterController> = Vec::new();
    let mut landwalk_subtypes: Vec<crate::Subtype> = Vec::new();
    let mut landwalk_qualities: Vec<crate::types::effect::LandwalkQuality> = Vec::new();
    let mut bands_with_other_subtypes: Vec<crate::Subtype> = Vec::new();
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::AbilityAdding) {
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::AddKeyword(added)) => keywords |= added,
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
            Some(EffectPrimitive::RemoveAllAbilities) => {
                keywords = KeywordSet::empty();
                protection_colors = crate::ColorSet::empty();
                protection_card_types = CardTypeSet::empty();
                protection_subtypes = Vec::new();
                protection_names = Vec::new();
                protection_everything = false;
                protection_players = Vec::new();
                landwalk_subtypes = Vec::new();
                landwalk_qualities = Vec::new();
                bands_with_other_subtypes = Vec::new();
            }
            _ => {}
        }
    }

    // Layer 7b: setting replaces whatever came before this layer entirely -
    // later timestamp wins outright, it doesn't stack with an earlier SetPT.
    for effect in applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessSetting) {
        if let Some(EffectPrimitive::SetPT { power: p, toughness: t }) =
            resolve_primitive(effect, state, script_runtime, obj.controller)
        {
            power = Some(p);
            toughness = Some(t);
        }
    }

    // Layer 7c: modifying is additive across every applicable effect
    // (order doesn't change the sum for a plain add), and doubling (CR
    // 613.8a-dependent on any modify touching the same field - see
    // `applicable_effects_in_layer`'s ordering, which already places
    // `ModifyPT` before a dependent `DoublePT`).
    //
    // CR 613.4c/121.3: `+1/+1`/`-1/-1` counters apply in this same sublayer,
    // interleaved by timestamp against the real effects above rather than
    // pre-netted into the layer 7a base (see this module's own doc comment
    // for the CR 613.5 worked example this makes correct). A counter add
    // has no CR 613.8a dependency of its own - only the real effects above
    // can - so it's merged in purely by comparing timestamps against
    // `applicable_effects_in_layer`'s already dependency-resolved order,
    // not re-sorted into it.
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

    // Layer 7e (CR 613.4d): swaps power/toughness, applied once per matching
    // effect in timestamp order after 7b/7c have fully settled (see this
    // function's own doc comment for why that ordering matches the CR's
    // worked example). Skipped if either side is still unresolved (e.g. no
    // CDA/SetPT/printed base at all) - same "skip rather than guess" stance
    // every other unresolvable value in this query already takes.
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
        mana_cost: def.mana_cost.clone(),
        colors,
        card_types,
        supertypes: def.supertypes,
        subtypes: def.subtypes.clone(),
        keywords,
        protection_colors,
        protection_card_types,
        protection_subtypes,
        protection_names,
        protection_everything,
        protection_players,
        landwalk_subtypes,
        landwalk_qualities,
        bands_with_other_subtypes,
        power,
        toughness,
        loyalty: def.loyalty.as_ref().and_then(|l| l.as_fixed()),
        defense: def.defense.as_ref().and_then(|d| d.as_fixed()),
    }
}

/// CR 108.4/613.1 layer 2: `object`'s live controller — its plain
/// `GameObject.controller` field (still the sole source of truth; nothing
/// mutates it), overridden by the last (highest-timestamp) applicable
/// `EffectPrimitive::GainControl` in `Layer::ControlChangingEffects`, same
/// override-not-additive shape as `effective_characteristics`'s own 7b
/// (`SetPT`) loop. Not yet consulted by any real gameplay site (combat,
/// targeting, cost filters, `EffectFilter::applies_to`/`FilterSpec::matches`,
/// `has_summoning_sickness` all still read `GameObject.controller` directly)
/// — see docs/implementation.md's `phase5/layer-engine-control` slice notes
/// for why that migration is a deliberate follow-up, not bundled here.
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
/// `object`, ordered per CR 613.7 (timestamp) and CR 613.8 (dependency
/// overrides timestamp where it applies) — see `order_by_dependency`.
/// Ordering here rather than at each call site keeps that contract in one
/// place regardless of which layer's caller in `effective_characteristics`
/// is walking it.
///
/// `AffectedFilter::Script` is evaluated here (not in
/// `ContinuousEffect::applies_to`, which stays `Primitive`-only so
/// `types/effect.rs` never needs to depend on `ScriptRuntime` — same
/// separation `resolve_base_pt` already keeps for CDA). A script that errors
/// resolves to "doesn't match" rather than guessed, same "skip rather than
/// guess" stance every other unresolvable script value in this engine
/// already takes.
///
/// CR 611.2c: `e.condition` (if set) is checked here too, before the
/// affected-filter match — an effect whose condition doesn't currently hold
/// contributes nothing this call, the same "recompute everything, no cache"
/// shape as the rest of this query. "You" resolves to `source_obj.controller`
/// (the raw field, not `layers::effective_controller`) — same Layer 2 gap
/// `e.applies_to`'s own `FilterController` check already has one line below,
/// and avoids this function recursing into `effective_controller` (which
/// itself calls this function) to evaluate a condition.
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
            // An effect whose own source has already left the battlefield
            // (and has no remap - see commands::retire_object) can't resolve
            // a controller to check `You`/`Opponent` against. Skip rather
            // than guess, same stance collect_triggers already takes for a
            // trigger whose source no longer resolves (CR 603.10 gap,
            // flagged there too).
            let Some(source_obj) = state.object(e.source) else { return false };
            // A script error here still falls back to the same
            // conservative "doesn't hold"/"doesn't apply" default as
            // engine.rs/triggers.rs's own script call sites - but unlike
            // those, it's never logged as a real ScriptError event
            // (`phase5/script-error-surfacing`'s own deliberate, named
            // exception): this function only has `&GameState`, called from
            // the hot-path characteristic-query graph
            // (`effective_characteristics`'s many callers, several times
            // per engine step) - no `&mut Engine` to log through without
            // threading error-reporting through that entire call graph, a
            // much larger change than this slice's own scope.
            let condition_holds = match &e.condition {
                None => true,
                Some(InterveningIfSpec::Primitive(p)) => p.holds(state, source_obj.controller),
                Some(InterveningIfSpec::Script(handle)) => script_runtime
                    .invoke_intervening_if(*handle, state, e.source, source_obj.controller)
                    .unwrap_or(false),
            };
            if !condition_holds {
                return false;
            }
            match &e.affected {
                AffectedFilter::Primitive(_) => e.applies_to(object, source_obj),
                AffectedFilter::Script(handle) => script_runtime
                    .invoke_continuous_affects(*handle, state, object.object_id, e.source, source_obj.controller)
                    .unwrap_or(false),
            }
        })
        .collect();
    order_by_dependency(effects)
}

/// CR 613.8's dependency system: within the single layer `applicable_effects_in_layer`
/// already filtered down to, reorders effects so a dependent effect (CR
/// 613.8a) applies after whatever it depends on (CR 613.8b), falling back to
/// plain `timestamp` order for effects with no unresolved dependency and, per
/// 613.8b's last sentence, for a dependency loop (detected as "no remaining
/// effect is ever un-blocked" - see the `is_empty()` fallback below).
///
/// Only `depends_on` below can report a real dependency today (`DoublePT` on
/// a same-field `ModifyPT`/`DoublePT` in layer 7c - see `EffectPrimitive::
/// DoublePT`'s own doc comment for why no other primitive combination can);
/// for every other layer/primitive pairing this produces the exact same
/// order plain timestamp sorting would, since every candidate is
/// immediately "ready" (CR 613.8c's re-evaluation is architecturally free
/// here too - this whole query, and this reorder, is recomputed from
/// scratch on every call, never cached).
fn order_by_dependency(mut remaining: Vec<&ContinuousEffect>) -> Vec<&ContinuousEffect> {
    let mut placed: Vec<&ContinuousEffect> = Vec::with_capacity(remaining.len());

    while !remaining.is_empty() {
        let ready: Vec<usize> = (0..remaining.len())
            .filter(|&i| {
                (0..remaining.len()).all(|j| i == j || !depends_on(remaining[i], remaining[j]))
            })
            .collect();

        if ready.is_empty() {
            // CR 613.8b: a dependency loop among everything still
            // unresolved - ignore dependency for the rest and fall back to
            // timestamp order.
            remaining.sort_by_key(|e| e.timestamp);
            placed.extend(remaining);
            return placed;
        }

        let next = *ready.iter().min_by_key(|&&i| remaining[i].timestamp).unwrap();
        placed.push(remaining.remove(next));
    }

    placed
}

/// CR 613.8a: does `effect` depend on `other` (must apply after it)? Both
/// arguments are already known to share a layer (the only caller,
/// `order_by_dependency`, is always given a single layer's effects), so this
/// only needs the "applying the other would change what this one does"
/// clause - the CDA carve-out (613.8a clause c) is moot here since a CDA
/// (layer 7a) is resolved inline via `resolve_base_pt`, never stored as a
/// `ContinuousEffect`. A `Script`-payload effect is always independent (see
/// `Layer`'s doc comment) - only `Primitive` pairs can produce a real
/// dependency yet.
fn depends_on(effect: &ContinuousEffect, other: &ContinuousEffect) -> bool {
    let (EffectPayload::Primitive(this), EffectPayload::Primitive(that)) = (&effect.effect, &other.effect) else {
        return false;
    };
    match (this, that) {
        (EffectPrimitive::DoublePT { power, toughness }, EffectPrimitive::ModifyPT { .. }) => *power || *toughness,
        (
            EffectPrimitive::DoublePT { power: p1, toughness: t1 },
            EffectPrimitive::DoublePT { power: p2, toughness: t2 },
        ) => (*p1 && *p2) || (*t1 && *t2),
        _ => false,
    }
}

/// Resolves a `ContinuousEffect`'s actual `EffectPrimitive` for this query —
/// `Primitive` needs no further evaluation, `Script` invokes
/// `ScriptRuntime::invoke_continuous_effect` (the "Lua asks, Rust decides"
/// table-decode convention — see that function's own doc comment for the six
/// recognized keys). A script that errors, or whose decoded shape doesn't
/// match what the caller's own layer loop expects (checked by the caller,
/// not here — see this slice's own UNRESOLVED note in
/// docs/implementation.md), resolves to `None`, same "skip rather than
/// guess" stance every other unresolvable value in this query already takes.
/// The error itself is never logged - same deliberate, named exception
/// `applicable_effects_in_layer`'s own script call sites already document
/// (`phase5/script-error-surfacing`): this is the same hot-path, `&mut
/// Engine`-less characteristic-query call graph.
fn resolve_primitive(
    effect: &ContinuousEffect,
    state: &GameState,
    script_runtime: &ScriptRuntime,
    controller: PlayerId,
) -> Option<EffectPrimitive> {
    match &effect.effect {
        EffectPayload::Primitive(p) => Some(p.clone()),
        EffectPayload::Script(handle) => {
            script_runtime.invoke_continuous_effect(*handle, state, effect.source, controller).ok().flatten()
        }
    }
}

/// CR 613.4a (layer 7a): resolves `value`'s base number - CDA/printed only,
/// no counter involvement (see this module's own doc comment for why
/// counters moved to the 7c fold instead). A `Fixed` value resolves to
/// itself; a `CharacteristicDefining` with an `Inline` script gets that
/// script invoked (`ScriptRuntime::invoke_cda`) for the base number. A
/// script that errors, or a `ByKey` reference (not resolvable by this
/// engine — see `ScriptRuntime::run_card_script`'s doc comment for the same
/// stance), or a `CharacteristicDefining` with no script at all, resolves to
/// `None` - same "skip rather than guess" stance every other unresolvable
/// value in this engine already takes.
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
