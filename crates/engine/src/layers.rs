use crate::script::ScriptRuntime;
use crate::types::ability::InterveningIfSpec;
use crate::types::card::{CardTypeSet, KeywordSet, PTValue, ScriptReference};
use crate::types::counter::CounterKind;
use crate::types::effect::{
    AffectedFilter, ContinuousEffect, EffectPayload, EffectPrimitive, EffectiveCharacteristics,
    Layer,
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
    let obj = state.object(object).unwrap_or_else(|| {
        panic!("effective_characteristics called with unknown ObjectId {object:?}")
    });
    // CR 613.1a: a copy takes its copiable values from a stored snapshot, not a layer 1 effect.
    let def = obj.copiable_override.as_ref().unwrap_or(&obj.definition);

    let mut power = resolve_base_pt(
        def.power.as_ref(),
        state,
        script_runtime,
        object,
        obj.controller,
    );
    let mut toughness = resolve_base_pt(
        def.toughness.as_ref(),
        state,
        script_runtime,
        object,
        obj.controller,
    );
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
                power = resolve_base_pt(
                    donor.power.as_ref(),
                    state,
                    script_runtime,
                    object,
                    obj.controller,
                );
                toughness = resolve_base_pt(
                    donor.toughness.as_ref(),
                    state,
                    script_runtime,
                    object,
                    obj.controller,
                );
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
    for effect in
        applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessSetting)
    {
        if let Some(EffectPrimitive::SetPT {
            power: p,
            toughness: t,
        }) = resolve_primitive(effect, state, script_runtime, obj.controller)
        {
            power = Some(p);
            toughness = Some(t);
        }
    }

    // Layer 7c (CR 613.4c): modifying is additive, and `applicable_effects_in_layer`
    // already orders `ModifyPT` before a dependent `DoublePT` (CR 613.8a).
    // Counters merge in by timestamp against that order without being re-sorted into it.
    let modify_effects =
        applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessModifying);
    let mut counter_deltas: Vec<(SequenceNo, i32, i32)> = obj
        .counters
        .batches(&CounterKind::PlusOnePlusOne)
        .map(|(amount, ts)| (ts, amount, amount))
        .chain(
            obj.counters
                .batches(&CounterKind::MinusOneMinusOne)
                .map(|(amount, ts)| (ts, -amount, -amount)),
        )
        .collect();
    counter_deltas.sort_by_key(|(ts, _, _)| *ts);
    let mut next_counter = 0;
    let apply_counters_up_to = |power: &mut Option<i32>,
                                toughness: &mut Option<i32>,
                                next_counter: &mut usize,
                                before: SequenceNo| {
        while *next_counter < counter_deltas.len() && counter_deltas[*next_counter].0 < before {
            let (_, dp, dt) = counter_deltas[*next_counter];
            *power = power.map(|p| p + dp);
            *toughness = toughness.map(|t| t + dt);
            *next_counter += 1;
        }
    };
    for effect in modify_effects {
        apply_counters_up_to(
            &mut power,
            &mut toughness,
            &mut next_counter,
            effect.timestamp,
        );
        match resolve_primitive(effect, state, script_runtime, obj.controller) {
            Some(EffectPrimitive::ModifyPT {
                power: dp,
                toughness: dt,
            }) => {
                power = power.map(|p| p + dp);
                toughness = toughness.map(|t| t + dt);
            }
            Some(EffectPrimitive::DoublePT {
                power: double_power,
                toughness: double_toughness,
            }) => {
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
    apply_counters_up_to(
        &mut power,
        &mut toughness,
        &mut next_counter,
        SequenceNo(u64::MAX),
    );

    // Layer 7e (CR 613.4d): skipped when either side is unresolved.
    for effect in
        applicable_effects_in_layer(state, script_runtime, obj, Layer::PowerToughnessSwitching)
    {
        if matches!(
            resolve_primitive(effect, state, script_runtime, obj.controller),
            Some(EffectPrimitive::SwitchPT)
        )
            && let (Some(p), Some(t)) = (power, toughness) {
                power = Some(t);
                toughness = Some(p);
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
pub fn effective_controller(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
) -> PlayerId {
    let obj = state
        .object(object)
        .unwrap_or_else(|| panic!("effective_controller called with unknown ObjectId {object:?}"));
    let mut controller = obj.controller;

    for effect in
        applicable_effects_in_layer(state, script_runtime, obj, Layer::ControlChangingEffects)
    {
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
            let Some(source_obj) = state.object(e.source) else {
                return false;
            };
            // Script errors are not logged: this hot query path holds only `&GameState`.
            let condition_holds = match &e.condition {
                None => true,
                Some(InterveningIfSpec::Primitive(p)) => {
                    p.holds(state, source_obj.controller, e.source, None)
                }
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
                (0..remaining.len()).all(|j| {
                    i == j
                        || !depends_on(
                            remaining[i],
                            remaining[j],
                            state,
                            script_runtime,
                            controller,
                        )
                })
            })
            .collect();

        if ready.is_empty() {
            // CR 613.8b: a dependency loop ignores dependency.
            remaining.sort_by_key(|e| e.timestamp);
            placed.extend(remaining);
            return placed;
        }

        let next = *ready
            .iter()
            .min_by_key(|&&i| remaining[i].timestamp)
            .unwrap();
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
        (EffectPrimitive::DoublePT { power, toughness }, EffectPrimitive::ModifyPT { .. }) => {
            power || toughness
        }
        (
            EffectPrimitive::DoublePT {
                power: p1,
                toughness: t1,
            },
            EffectPrimitive::DoublePT {
                power: p2,
                toughness: t2,
            },
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
            // `effective_characteristics` holds only `&GameState`, so a script error here is
            // logged but cannot be propagated as a result. The conservative fallback is to treat
            // the effect as absent for this computation.
            match script_runtime.invoke_continuous_effect(
                *handle,
                state,
                effect.source,
                controller,
                enchanted,
            ) {
                Ok(Some(p)) => Some(p),
                Ok(None) => None,
                Err(e) => {
                    eprintln!(
                        "continuous effect script error for object {:?}: {e:?}",
                        effect.source
                    );
                    None
                }
            }
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
    if let Some(PTValue::CharacteristicDefining {
        script: Some(ScriptReference::Inline(source_code)),
        ..
    }) = value
    {
        return script_runtime
            .invoke_cda(source_code, state, source, controller)
            .ok();
    }
    value.and_then(|v| v.as_fixed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{
        CardDefinition, ModeChoice, PTValue, ScriptReference, SupertypeSet as SupertypeSetT,
    };
    use crate::types::counter::{CounterKind, CounterSet};
    use crate::types::game::{GameState, RngState};
    use crate::types::ids::{ObjectId, PlayerId, SequenceNo};
    use crate::types::mana::ManaCost;
    use crate::types::object::{GameObject, VisibilityPolicy};
    use crate::types::player::PlayerState;
    use crate::types::zone::ZoneKind;
    use crate::{CardTypeSet, KeywordSet, ManaPool};
    use std::collections::HashMap;

    fn player(id: PlayerId) -> PlayerState {
        PlayerState {
            player_id: id,
            life: 20,
            mana_pool: ManaPool::default(),
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: HashMap::new(),
        }
    }

    fn object(
        id: ObjectId,
        controller: PlayerId,
        card_types: CardTypeSet,
        zone: ZoneKind,
    ) -> GameObject {
        GameObject {
            object_id: id,
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: ManaCost {
                    symbols: Vec::new(),
                },
                color_indicator: None,
                supertypes: SupertypeSetT::empty(),
                card_types,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            copiable_override: None,
            owner: controller,
            controller,
            zone,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy {
                public: true,
                visible_to: Vec::new(),
            },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    fn creature_with_pt(
        id: ObjectId,
        controller: PlayerId,
        power: i32,
        toughness: i32,
    ) -> GameObject {
        let mut obj = object(id, controller, CardTypeSet::CREATURE, ZoneKind::Battlefield);
        obj.definition.power = Some(PTValue::Fixed(power));
        obj.definition.toughness = Some(PTValue::Fixed(toughness));
        obj
    }

    fn state_with(objects: Vec<GameObject>) -> GameState {
        let p0 = PlayerId(0);
        GameState {
            players: vec![player(p0)],
            turn_order: vec![p0],
            active_player: p0,
            turn_number: 1,
            phase: crate::types::turn::Phase::Main,
            step: crate::types::turn::Step::PreCombatMain,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: objects.into_iter().map(|o| (o.object_id, o)).collect(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState {
                seed: 1,
                draws_consumed: 0,
            },
            next_event_id: 0,
            next_sequence_no: 10,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    fn continuous_effect(
        source: ObjectId,
        layer: Layer,
        primitive: EffectPrimitive,
        affected: AffectedFilter,
        timestamp: u64,
    ) -> ContinuousEffect {
        ContinuousEffect {
            source,
            layer,
            affected,
            effect: EffectPayload::Primitive(primitive),
            timestamp: SequenceNo(timestamp),
            duration: crate::types::effect::EffectDuration::Permanent,
            condition: None,
        }
    }

    fn self_filter(_id: ObjectId) -> AffectedFilter {
        AffectedFilter::Primitive(crate::types::effect::EffectFilter {
            controller: None,
            card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            self_only: true,
        })
    }

    #[test]
    fn effective_controller_without_effects_returns_raw_controller() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let state = state_with(vec![object(
            id,
            p0,
            CardTypeSet::CREATURE,
            ZoneKind::Battlefield,
        )]);
        assert_eq!(effective_controller(&state, &ScriptRuntime::new(), id), p0);
    }

    #[test]
    fn effective_controller_follows_latest_gain_control() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let id = ObjectId(1);
        let mut state = state_with(vec![object(
            id,
            p0,
            CardTypeSet::CREATURE,
            ZoneKind::Battlefield,
        )]);
        state.players.push(player(p1));
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::ControlChangingEffects,
            EffectPrimitive::GainControl(p1),
            self_filter(id),
            1,
        ));
        assert_eq!(effective_controller(&state, &ScriptRuntime::new(), id), p1);
    }

    #[test]
    fn effective_characteristics_carries_base_definition_values() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut obj = object(id, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield);
        obj.definition.power = Some(PTValue::Fixed(3));
        obj.definition.toughness = Some(PTValue::Fixed(4));
        obj.definition.keywords = KeywordSet::FLYING;
        let state = state_with(vec![obj]);
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.power, Some(3));
        assert_eq!(ec.toughness, Some(4));
        assert!(ec.keywords.contains(KeywordSet::FLYING));
    }

    #[test]
    fn add_card_type_unions_with_base_types() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut state = state_with(vec![object(
            id,
            p0,
            CardTypeSet::LAND,
            ZoneKind::Battlefield,
        )]);
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::TypeChanging,
            EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
            self_filter(id),
            1,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert!(ec.card_types.contains(CardTypeSet::LAND));
        assert!(ec.card_types.contains(CardTypeSet::CREATURE));
    }

    #[test]
    fn set_card_types_overwrites_earlier_type_changes() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut state = state_with(vec![object(
            id,
            p0,
            CardTypeSet::LAND,
            ZoneKind::Battlefield,
        )]);
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::TypeChanging,
            EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
            self_filter(id),
            1,
        ));
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::TypeChanging,
            EffectPrimitive::SetCardTypes(CardTypeSet::ARTIFACT),
            self_filter(id),
            2,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.card_types, CardTypeSet::ARTIFACT);
    }

    #[test]
    fn add_color_unions_with_base_color() {
        let p0 = PlayerId(0);
        let mut obj = object(
            ObjectId(1),
            p0,
            CardTypeSet::CREATURE,
            ZoneKind::Battlefield,
        );
        obj.definition.color_indicator = Some(crate::ColorSet::BLUE);
        let mut state = state_with(vec![obj]);
        state.continuous_effects.push(continuous_effect(
            ObjectId(1),
            Layer::ColorChanging,
            EffectPrimitive::AddColor(crate::ColorSet::RED),
            self_filter(ObjectId(1)),
            1,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), ObjectId(1));
        assert!(ec.colors.contains(crate::ColorSet::BLUE));
        assert!(ec.colors.contains(crate::ColorSet::RED));
    }

    #[test]
    fn add_keyword_grants_and_remove_keyword_removes() {
        let p0 = PlayerId(0);
        let mut obj = object(
            ObjectId(1),
            p0,
            CardTypeSet::CREATURE,
            ZoneKind::Battlefield,
        );
        obj.definition.keywords = KeywordSet::FLYING;
        let mut state = state_with(vec![obj]);
        state.continuous_effects.push(continuous_effect(
            ObjectId(1),
            Layer::AbilityAdding,
            EffectPrimitive::AddKeyword(KeywordSet::REACH),
            self_filter(ObjectId(1)),
            1,
        ));
        state.continuous_effects.push(continuous_effect(
            ObjectId(1),
            Layer::AbilityAdding,
            EffectPrimitive::RemoveKeyword(KeywordSet::FLYING),
            self_filter(ObjectId(1)),
            2,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), ObjectId(1));
        assert!(!ec.keywords.contains(KeywordSet::FLYING));
        assert!(ec.keywords.contains(KeywordSet::REACH));
    }

    #[test]
    fn modify_pt_adds_to_base() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut state = state_with(vec![creature_with_pt(id, p0, 2, 3)]);
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::PowerToughnessModifying,
            EffectPrimitive::ModifyPT {
                power: 2,
                toughness: -1,
            },
            self_filter(id),
            1,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.power, Some(4));
        assert_eq!(ec.toughness, Some(2));
    }

    #[test]
    fn set_pt_overwrites_base() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut state = state_with(vec![creature_with_pt(id, p0, 2, 3)]);
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::PowerToughnessSetting,
            EffectPrimitive::SetPT {
                power: 5,
                toughness: 5,
            },
            self_filter(id),
            1,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.power, Some(5));
        assert_eq!(ec.toughness, Some(5));
    }

    #[test]
    fn double_pt_depends_on_earlier_modify_pt() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut state = state_with(vec![creature_with_pt(id, p0, 2, 2)]);
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::PowerToughnessModifying,
            EffectPrimitive::ModifyPT {
                power: 2,
                toughness: 0,
            },
            self_filter(id),
            1,
        ));
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::PowerToughnessModifying,
            EffectPrimitive::DoublePT {
                power: true,
                toughness: false,
            },
            self_filter(id),
            2,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.power, Some(8));
        assert_eq!(ec.toughness, Some(2));
    }

    #[test]
    fn switch_pt_swaps_power_and_toughness() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut state = state_with(vec![creature_with_pt(id, p0, 2, 5)]);
        state.continuous_effects.push(continuous_effect(
            id,
            Layer::PowerToughnessSwitching,
            EffectPrimitive::SwitchPT,
            self_filter(id),
            1,
        ));
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.power, Some(5));
        assert_eq!(ec.toughness, Some(2));
    }

    #[test]
    fn plus_one_counters_add_to_power_and_toughness() {
        let p0 = PlayerId(0);
        let id = ObjectId(1);
        let mut obj = creature_with_pt(id, p0, 2, 2);
        obj.counters
            .add(CounterKind::PlusOnePlusOne, 3, SequenceNo(1));
        let state = state_with(vec![obj]);
        let ec = effective_characteristics(&state, &ScriptRuntime::new(), id);
        assert_eq!(ec.power, Some(5));
        assert_eq!(ec.toughness, Some(5));
    }
}
