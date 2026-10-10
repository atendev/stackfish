//! Phase 5 vertical slice: CR 707.9c/d — "some copy effects specifically
//! state that they don't copy certain characteristics and the affected
//! objects instead retain their original values." The inverse of the
//! existing `CopyExceptions` fields (`set_power_toughness`/`add_keywords`
//! *provide new* values; `retain_name`/`retain_color` *keep old* ones).
//!
//! `GameObject.definition` (the object's own true, never-overwritten
//! characteristics) is already sitting right there at every copy call site,
//! distinct from `copiable_override` (the frozen copy snapshot) — so
//! `CopyExceptions::apply` just takes it as a new `Option<&CardDefinition>`
//! parameter and reads it back when `retain_name`/`retain_color` is set.
//! `None` only for `TokenSpec::CopyOf`: a freshly created token has no prior
//! "own identity" to retain from at all, so both flags are a documented,
//! tested no-op there.
//!
//! Narrowed to name/color (not all seven CR 707.2 copiable characteristics)
//! — no real card combination needs retained P/T/types/subtypes yet, same
//! "no real card" posture the original `set_power_toughness`/`add_keywords`
//! fields were scoped with. CR 707.9e/g ("exception tied to the most recent
//! copy effect") stays unmodeled — it needs a real `GameObject` schema
//! addition (a copy generation/history), not just a new `CopyExceptions`
//! field.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::Engine;
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain};

fn doppelganger_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Doppelganger".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        // Its own original color - blue, distinct from what it copies.
        color_indicator: Some(ColorSet::BLUE),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        // CR 704.5f: nonzero toughness so it survives to actually copy.
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn bear_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: Some(ColorSet::RED),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::FLYING,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn become_copy_ability(source: ObjectId, exceptions: CopyExceptions) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::BecomeCopy(exceptions)),
        intervening_if: None,
    }
}

fn one_player_state(objects: Vec<GameObject>, abilities: Vec<RegisteredAbility>) -> GameState {
    let p0 = PlayerId(0);
    let mut objects_map = HashMap::new();
    let mut bf_contents = VecDeque::new();
    for obj in objects {
        bf_contents.push_back(obj.object_id);
        objects_map.insert(obj.object_id, obj);
    }
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
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: objects_map,
        zones: vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: bf_contents }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        hand_reveal_modifiers: Vec::new(),
        draw_limit_modifiers: Vec::new(),
        land_play_modifiers: Vec::new(),
        abilities,
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
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

fn activate_pay_and_resolve(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        .expect("player holds priority");
    drain(engine);
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("free cost");
    drain(engine);
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
}

#[test]
fn retain_name_keeps_the_copys_own_name() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let exceptions = CopyExceptions { retain_name: true, ..CopyExceptions::default() };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel, exceptions)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(chars.name, "Doppelganger", "retain_name keeps the copy's own original name");
    assert_eq!(chars.power, Some(2), "everything else still comes from the copied object");
    assert!(chars.keywords.contains(KeywordSet::FLYING));
}

#[test]
fn retain_color_keeps_the_copys_own_color() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let exceptions = CopyExceptions { retain_color: true, ..CopyExceptions::default() };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel, exceptions)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(chars.colors, ColorSet::BLUE, "retain_color keeps the copy's own original color, not the copied Bear's red");
    assert_eq!(chars.name, "Bear", "name still comes from the copied object");
}

#[test]
fn without_retain_color_the_copy_adopts_the_copied_objects_color() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel, CopyExceptions::default())],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(chars.colors, ColorSet::RED, "no retain_color exception - the copy is red like Bear");
}

#[test]
fn retain_name_and_retain_color_combine_with_pt_and_keyword_exceptions() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let exceptions = CopyExceptions {
        set_power_toughness: Some((9, 9)),
        add_keywords: KeywordSet::TRAMPLE,
        retain_name: true,
        retain_color: true,
        grant_recurring_copy_trigger: false,
        enters_with_additional_counters: None,
    };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel, exceptions)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(chars.name, "Doppelganger", "retained");
    assert_eq!(chars.colors, ColorSet::BLUE, "retained");
    assert_eq!(chars.power, Some(9), "overridden by set_power_toughness");
    assert_eq!(chars.toughness, Some(9), "overridden by set_power_toughness");
    assert!(chars.keywords.contains(KeywordSet::TRAMPLE), "added by add_keywords");
    assert!(chars.keywords.contains(KeywordSet::FLYING), "still comes from the copied object");
}

#[test]
fn retain_name_and_retain_color_are_a_no_op_for_a_freshly_created_copy_token() {
    let mut state = one_player_state(vec![card_object(ObjectId(1), bear_definition(), PlayerId(0), ZoneKind::Battlefield)], Vec::new());
    let p0 = PlayerId(0);
    let bear = ObjectId(1);
    let exceptions = CopyExceptions { retain_name: true, retain_color: true, ..CopyExceptions::default() };
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: bear,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::CreateToken { spec: TokenSpec::CopyOf(bear, exceptions), count: 1 }),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, bear, Vec::new());

    let token_id = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .copied()
        .find(|&id| id != bear)
        .expect("a token was created");
    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), token_id);
    assert_eq!(chars.name, "Bear", "no prior identity to retain from - the token is fully the copied object's name");
    assert_eq!(chars.colors, ColorSet::RED, "same for color");
}

// ---------------------------------------------------------------------
// Game.Ops.BecomeCopy Lua decoding.
// ---------------------------------------------------------------------

#[test]
fn game_ops_become_copy_decodes_retain_name_and_retain_color() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation("return Game.Ops.BecomeCopy({ retain_name = true, retain_color = true })")
        .expect("loads cleanly");
    let Some(AbilityEffect::BecomeCopy(exceptions)) = effect else {
        panic!("expected Some(AbilityEffect::BecomeCopy(_)), got {effect:?}")
    };
    assert!(exceptions.retain_name);
    assert!(exceptions.retain_color);
    assert_eq!(exceptions.set_power_toughness, None);
}
