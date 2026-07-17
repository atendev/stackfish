//! Phase 5 vertical slice: CR 611.2c's conditional ("as long as ...")
//! continuous effects. Scoped tightly to `InterveningIfPrimitive::
//! ControllerControlsAtLeast` — the one condition shape this slice ships
//! (see docs/implementation.md's own slice notes) — reusing CR 603.4's
//! `InterveningIfSpec`/`InterveningIfPrimitive` rather than inventing a
//! second predicate type, since both are "evaluate a live board-state
//! predicate, get a bool."
//!
//! `layers::applicable_effects_in_layer` (the one call-site every layer
//! already funnels through) drops an effect whose `condition` doesn't
//! currently hold - checked fresh on every call, no caching/cleanup, same
//! "recompute everything" architecture `EffectiveCharacteristics` already
//! has. The tests below exercise that directly through
//! `effective_characteristics`, plus the `Game.InterveningIf.ControlsAtLeast`/
//! `StaticAbilityBuilder:SetCondition` Lua binding end to end.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn creature_definition(power: i32, toughness: i32, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Permanent".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, owner: PlayerId, power: i32, toughness: i32, card_types: CardTypeSet) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(power, toughness, card_types),
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// "Creatures you control get +1/+1 as long as you control `count` or more
/// artifacts" - the anthem shape this slice's Tests section names.
fn conditional_anthem(source: ObjectId, count: u32) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: Some(FilterController::You),
            card_types: None,
            self_only: false,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: Some(InterveningIfSpec::Primitive(InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: FilterSpec {
                card_types: Some(CardTypeSet::ARTIFACT),
                supertypes: None,
                subtypes: None,
                controller: Some(FilterController::You),
                random: false,
                extra: None,
            },
            count,
        })),
    }
}

#[test]
fn conditional_effect_applies_when_the_control_count_is_met() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    put_on_battlefield(&mut state, permanent(ObjectId(2), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(3), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(4), p0, 0, 0, CardTypeSet::ARTIFACT));
    state.continuous_effects.push(conditional_anthem(anthem, 3));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3), "3 artifacts meets the threshold - the anthem applies");
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn conditional_effect_does_not_apply_when_the_control_count_is_not_met() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    put_on_battlefield(&mut state, permanent(ObjectId(2), p0, 0, 0, CardTypeSet::ARTIFACT));
    state.continuous_effects.push(conditional_anthem(anthem, 3));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(2), "only 1 artifact - below the threshold, no boost");
    assert_eq!(effective.toughness, Some(2));
}

#[test]
fn conditional_effect_stops_applying_the_instant_a_matching_permanent_leaves() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    put_on_battlefield(&mut state, permanent(ObjectId(2), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(3), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(4), p0, 0, 0, CardTypeSet::ARTIFACT));
    state.continuous_effects.push(conditional_anthem(anthem, 3));

    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), bear).power,
        Some(3),
        "starts with 3 artifacts - the anthem applies"
    );

    // No cache to invalidate - a plain zone removal, then re-querying the
    // same object, is the whole "re-evaluated continuously" proof CR 611.2c
    // asks for.
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().remove(ObjectId(4));
    state.objects.remove(&ObjectId(4));

    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), bear).power,
        Some(2),
        "down to 2 artifacts - the anthem stops applying, re-derived fresh"
    );
}

#[test]
fn an_unconditional_effect_is_completely_unaffected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    state.continuous_effects.push(ContinuousEffect {
        source: anthem,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: false }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3), "condition: None means unconditional, same as before this slice");
    assert_eq!(effective.toughness, Some(3));
}

// ---------------------------------------------------------------------
// Game.InterveningIf.ControlsAtLeast / StaticAbilityBuilder:SetCondition -
// the Lua round-trip, both the Primitive and Script decode paths.
// ---------------------------------------------------------------------

/// Returns the `ScriptRuntime` too, not just the resulting `GameState` -
/// a `Script`-backed condition's `ScriptHandle` only resolves against the
/// same runtime's registry that loaded it (see `ScriptRuntime::function`),
/// so callers exercising that path must query `effective_characteristics`
/// with *this* runtime, not a fresh `ScriptRuntime::new()`.
fn run_static_ability_script(source: ObjectId, script: &str) -> (GameState, ScriptRuntime) {
    let mut state = base_state();
    let p0 = PlayerId(0);
    put_on_battlefield(&mut state, permanent(source, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, 2, 2, CardTypeSet::CREATURE));

    let mut runtime = ScriptRuntime::new();
    let definition = CardDefinition { script_ref: ScriptReference::Inline(script.to_string()), ..creature_definition(0, 0, CardTypeSet::ENCHANTMENT) };
    runtime.run_card_script(&mut state, &definition, source).expect("card script registers a static ability");
    (state, runtime)
}

#[test]
fn lua_controls_at_least_primitive_round_trips_through_set_condition() {
    let source = ObjectId(0);
    let script = r#"
        local s = {}
        function s.initial_effect(c)
            local e = Game.CreateStaticAbility(c)
            e:SetLayer(Game.Layers.PowerToughnessModifying)
            e:SetEffect(Game.Effects.ModifyPT(1, 1))
            e:SetAffected({ controller = "you" })
            e:SetCondition(Game.InterveningIf.ControlsAtLeast({ card_types = { "Artifact" } }, 3))
            e:Register()
        end
        return s
    "#;
    let (mut state, runtime) = run_static_ability_script(source, script);
    let bear = ObjectId(1);

    // Below the threshold - no boost yet.
    assert_eq!(effective_characteristics(&state, &runtime, bear).power, Some(2));

    // Add 3 artifacts and re-query - the registered condition is a real
    // InterveningIfSpec::Primitive, re-evaluated fresh, no re-registration
    // needed.
    put_on_battlefield(&mut state, permanent(ObjectId(2), PlayerId(0), 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(3), PlayerId(0), 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(4), PlayerId(0), 0, 0, CardTypeSet::ARTIFACT));
    assert_eq!(effective_characteristics(&state, &runtime, bear).power, Some(3));
}

#[test]
fn lua_set_condition_accepts_a_custom_function_as_a_script_backed_condition() {
    let source = ObjectId(0);
    // A custom condition function - invoked via ScriptRuntime::invoke_intervening_if,
    // same InterveningIfSpec::Script path CR 603.4's SetInterveningIf already exercises.
    let script = r#"
        local s = {}
        function s.initial_effect(c)
            local e = Game.CreateStaticAbility(c)
            e:SetLayer(Game.Layers.PowerToughnessModifying)
            e:SetEffect(Game.Effects.ModifyPT(1, 1))
            e:SetAffected({ controller = "you" })
            e:SetCondition(function(src, controller, snapshot)
                return true
            end)
            e:Register()
        end
        return s
    "#;
    let (state, runtime) = run_static_ability_script(source, script);
    let bear = ObjectId(1);

    assert_eq!(
        effective_characteristics(&state, &runtime, bear).power,
        Some(3),
        "the Script-backed condition always returns true, so the effect applies"
    );
}
