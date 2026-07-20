//! Phase 5 vertical slice: `Game.ReplacementPayloads.RedirectDamageToSource`
//! (CR 614.9) — the first Lua construction path for
//! `ReplacementPayload::RedirectDamage(Target)`, narrowed to exactly
//! "redirect to this effect's own source" (Pariah/Pariah's Shield-shape
//! cards: "damage that would be dealt to you is dealt to [this permanent]
//! instead"). `ReplacementEffectBuilder`'s own doc comment (script.rs) names
//! why an arbitrary chosen redirect target stays unbound.
//!
//! `matches()`-level unit tests for CR 614.9's own destination-validity
//! clause ("if [the destination] is no longer on the battlefield... the
//! effect does nothing") live in `types/effect.rs`'s own test module
//! instead, since exercising an eliminated-player destination needs the
//! general `Target::Player` shape this narrow Lua binding never produces.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

const REDIRECT_DAMAGE_TO_SELF: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
    e:SetAffected({ controller = "you" })
    e:Register()
end
return s
"#;

const MISMATCHED_WOULD_DIE_REDIRECT: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldDie)
    e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
    e:Register()
end
return s
"#;

fn creature_definition(name: &str, power: i32, toughness: i32, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Redirect-damage test fixture.".to_string(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn vanilla_creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition { script_ref: ScriptReference::ByKey(name.to_string()), ..creature_definition(name, power, toughness, "") }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn player_state(id: PlayerId) -> PlayerState {
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
        commander_casts: std::collections::HashMap::new(),
    }
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::CombatDamage,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        objects: HashMap::new(),
        zones: vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
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

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
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

// ---------------------------------------------------------------------
// Unit-level: run_card_script's self-registration mechanics.
// ---------------------------------------------------------------------

#[test]
fn set_payload_redirect_damage_to_source_registers_with_own_source_as_target() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Pariah", 0, 4, REDIRECT_DAMAGE_TO_SELF);
    state.objects.insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt { affected },
            payload: ReplacementPayload::RedirectDamage(Target::Object(target)),
        } => {
            assert_eq!(*target, source, "RedirectDamageToSource must resolve to the registering permanent's own id");
            assert_eq!(affected.controller, Some(FilterController::You));
        }
        other => panic!("expected a DamageDealt/RedirectDamage(Object(source)) primitive spec, got {other:?}"),
    }
}

#[test]
fn mismatched_would_die_filter_and_redirect_damage_payload_is_a_real_register_time_error() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Broken", 2, 2, MISMATCHED_WOULD_DIE_REDIRECT);
    state.objects.insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, source);

    assert!(result.is_err(), "WouldDie + RedirectDamage is a real mismatch, must be rejected");
    assert!(state.replacement_effects.is_empty(), "no partial effect should have been committed");
}

// ---------------------------------------------------------------------
// End-to-end: a Lua-registered redirect actually changes real Engine
// combat-damage resolution, not just the queued struct's shape.
// ---------------------------------------------------------------------

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

#[test]
fn lua_registered_redirect_damage_sends_combat_damage_to_shield_instead_of_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let shield_source = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
    });
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature_definition("Bear", 3, 3), p0));
    let shield_definition = creature_definition("Pariah", 0, 4, REDIRECT_DAMAGE_TO_SELF);
    put_on_battlefield(&mut state, permanent(shield_source, shield_definition.clone(), p1));

    ScriptRuntime::new()
        .run_card_script(&mut state, &shield_definition, shield_source)
        .expect("script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert_eq!(
        engine.state().player(p1).unwrap().life,
        20,
        "the Lua-registered redirect must have kept the damage off the player entirely"
    );
    assert_eq!(
        engine.state().object(shield_source).map(|o| o.damage_marked),
        Some(3),
        "the redirected damage must have actually landed on the shield instead"
    );
    assert!(
        !engine.state().replacement_effects.is_empty(),
        "Repeatable (the default consumption) must not be consumed by one redirect"
    );
}
