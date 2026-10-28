//! `state_to_lua` is a public-safe board-state snapshot passed to every state-aware `invoke_*` call. Battlefield, graveyard, exile and command are public (CR 400.2, 402.3) with one card-summary table per object, and hand and library expose only `hand_count` and `library_count`.
//! Card summaries are base values netted against counters and not layer-resolved, to avoid unbounded recursion when a battlefield object has its own CDA or scripted continuous effect.

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

fn creature_definition(power: i32, toughness: i32, script_ref: ScriptReference) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref,
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    let definition = creature_definition(power, toughness, ScriptReference::ByKey("vanilla".to_string()));
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn scripted_creature(id: ObjectId, owner: PlayerId, source: &str) -> GameObject {
    let definition = creature_definition(0, 0, ScriptReference::Inline(String::new())).with_cda_power(source);
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

trait WithCdaPower {
    fn with_cda_power(self, source: &str) -> Self;
}

impl WithCdaPower for CardDefinition {
    fn with_cda_power(mut self, source: &str) -> Self {
        self.power = Some(PTValue::CharacteristicDefining {
            display: "*".to_string(),
            script: Some(ScriptReference::Inline(source.to_string())),
        });
        self.toughness = Some(PTValue::Fixed(2));
        self
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Battlefield, ZoneKind::Exile, ZoneKind::Graveyard, ZoneKind::Hand, ZoneKind::Library, ZoneKind::Command],
        ))
    }
}

fn put_in_zone(state: &mut GameState, kind: ZoneKind, owner: Option<PlayerId>, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

#[test]
fn hand_and_library_counts_are_exposed_but_never_contents() {
    let runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    put_in_zone(&mut state, ZoneKind::Hand, Some(p0), creature(ObjectId(1), p0, 2, 2));
    put_in_zone(&mut state, ZoneKind::Hand, Some(p0), creature(ObjectId(2), p0, 3, 3));
    put_in_zone(&mut state, ZoneKind::Library, Some(p0), creature(ObjectId(3), p0, 1, 1));

    let source = "return function(source, controller, state) \
        if state.hand ~= nil or state.library ~= nil then return -1 end \
        return state.hand_count[controller] * 100 + state.library_count[controller] \
        end";
    let cda = scripted_creature(ObjectId(100), p0, source);
    let cda_id = cda.object_id;
    put_in_zone(&mut state, ZoneKind::Battlefield, None, cda);

    let power = effective_characteristics(&state, &runtime, cda_id).power;
    assert_eq!(power, Some(201), "2 cards in hand, 1 in library, and no hand/library content keys leaked");
}

#[test]
fn hand_count_is_symmetric_for_owner_and_opponent() {
    let runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    put_in_zone(&mut state, ZoneKind::Hand, Some(p1), creature(ObjectId(1), p1, 2, 2));
    put_in_zone(&mut state, ZoneKind::Hand, Some(p1), creature(ObjectId(2), p1, 2, 2));
    put_in_zone(&mut state, ZoneKind::Hand, Some(p1), creature(ObjectId(3), p1, 2, 2));

    // invoke_cda is the state-aware entry point under test. A controller can read an opponent's hand count (CR 402.3), so the count-only exposure is symmetric.
    let source = "return function(source, controller, state) return state.hand_count[1] end";
    let base = runtime.invoke_cda(source, &state, ObjectId(1), p0).unwrap();
    assert_eq!(base, 3);
}

#[test]
fn cda_reads_graveyard_card_count_for_base_power() {
    let runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    put_in_zone(&mut state, ZoneKind::Graveyard, Some(p0), creature(ObjectId(1), p0, 2, 2));
    put_in_zone(&mut state, ZoneKind::Graveyard, Some(p0), creature(ObjectId(2), p0, 2, 2));
    put_in_zone(&mut state, ZoneKind::Graveyard, Some(p0), creature(ObjectId(3), p0, 2, 2));

    let source = "return function(source, controller, state) return #state.graveyard[controller] end";
    let cda = scripted_creature(ObjectId(100), p0, source);
    let cda_id = cda.object_id;
    put_in_zone(&mut state, ZoneKind::Battlefield, None, cda);

    let chars = effective_characteristics(&state, &runtime, cda_id);
    assert_eq!(chars.power, Some(3), "3 cards in the controller's own graveyard");
}

#[test]
fn card_summaries_include_counters_but_not_layer_resolved_effective_pt() {
    let mut runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    let mut bear = creature(ObjectId(1), p0, 2, 2);
    bear.counters.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(0));
    let bear_id = bear.object_id;
    put_in_zone(&mut state, ZoneKind::Battlefield, None, bear);

    let source = "return function(object, source, controller, state) \
        for _, c in ipairs(state.battlefield) do \
            if c.id == object then return c.power == 3 and c.toughness == 3 end \
        end \
        return false \
        end";
    let handle = runtime.load_function(source).unwrap();

    assert!(
        runtime.invoke_continuous_affects(handle, &state, bear_id, bear_id, p0, None).unwrap(),
        "base 2/2 + a +1/+1 counter should read as 3/3 in the snapshot (CR 121.3), no layer engine involved"
    );
}

#[test]
fn continuous_affects_predicate_can_gate_on_a_players_life_total() {
    let mut runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = creature(ObjectId(1), p0, 2, 2);
    let bear_id = bear.object_id;
    put_in_zone(&mut state, ZoneKind::Battlefield, None, bear);

    // "Affects only creatures controlled by a player at 5 life or less."
    let source = "return function(object, source, controller, state) \
        for _, c in ipairs(state.battlefield) do \
            if c.id == object then return state.life[c.controller] <= 5 end \
        end \
        return false \
        end";
    let handle = runtime.load_function(source).unwrap();

    assert!(
        !runtime.invoke_continuous_affects(handle, &state, bear_id, bear_id, p0, None).unwrap(),
        "controller starts at 20 life, should not match yet"
    );

    state.player_mut(p0).unwrap().life = 5;
    assert!(
        runtime.invoke_continuous_affects(handle, &state, bear_id, bear_id, p0, None).unwrap(),
        "controller now at 5 life, should match"
    );
}

#[test]
fn continuous_effect_payload_can_read_state_to_decide_its_own_shape() {
    let mut runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);

    let source = "return function(source, controller, state) \
        if state.life[controller] <= 5 then \
            return { modify_pt = { power = 1, toughness = 1 } } \
        end \
        return {} \
        end";
    let handle = runtime.load_function(source).unwrap();

    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), p0, None).unwrap();
    assert_eq!(decoded, None, "controller at full life - no boost");

    state.player_mut(p0).unwrap().life = 5;
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), p0, None).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }));
}

#[test]
fn exile_and_command_zones_are_public_too() {
    let runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    put_in_zone(&mut state, ZoneKind::Exile, None, creature(ObjectId(1), p0, 2, 2));
    put_in_zone(&mut state, ZoneKind::Command, Some(p0), creature(ObjectId(2), p0, 4, 4));

    let source = "return function(source, controller, state) return #state.exile + #state.command[controller] end";
    let cda = scripted_creature(ObjectId(100), p0, source);
    let cda_id = cda.object_id;
    put_in_zone(&mut state, ZoneKind::Battlefield, None, cda);

    let chars = effective_characteristics(&state, &runtime, cda_id);
    assert_eq!(chars.power, Some(2));
}
