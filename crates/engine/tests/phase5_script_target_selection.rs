//! Phase 5 vertical slice: backlog item 9, `Game.SelectTargets` — the first
//! real end-to-end use of the Lua coroutine mechanism `phase5_coroutine_spike.rs`
//! confirmed against real mlua. A card script's operation can now suspend
//! mid-resolution to ask the player for a real target (`ScriptRuntime::
//! invoke_operation` runs the operation as a coroutine; `Game.SelectTargets`
//! is a plain Lua function that calls `coroutine.yield` directly - not a
//! Rust-bound callback, since mlua 0.9 without the `async` feature can't
//! yield from inside a Rust closure). `Engine::submit_script_choice` answers
//! the pause, resuming the same suspended script with the chosen target.
//!
//! Scoped narrowly on purpose: exactly one request shape (`select_target`,
//! count always 1) - modal spells and "may" abilities are still named
//! follow-ups, not attempted here. Backlog item 9b (player-target selection,
//! `Game.SelectTargets({ type = "player" })`/`{ type = "any" }`) is covered
//! by this file's own player/any-target tests below.
//!
//! Uses real card-script self-registration (`Game.CreateActivatedAbility`,
//! same mechanism `phase5_activated_ability_self_registration.rs` already
//! proves) rather than pre-loading a script into a standalone `ScriptRuntime`
//! - the operation must be registered through `Engine`'s own internal
//! `ScriptRuntime` (a separate `ScriptRuntime` is a different Lua VM
//! entirely; a `ScriptHandle` from one is meaningless to the other).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, ScriptChoiceError, StepStatus};
use stackfish_engine::script::ScriptChoiceRequest;
use stackfish_engine::*;

/// The chosen creature's own raw object id becomes the gain_life amount -
/// proves the exact resumed value reached the script and its continuation
/// ran with it, without needing a new effect-shape key just for this test.
const SELF_REGISTERING_SELECT_TARGET_THEN_GAIN_LIFE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        local target = Game.SelectTargets({ card_types = { "Creature" } })
        return { gain_life = target.id }
    end)
    e:Register()
end
return s
"#;

/// Backlog item 9b: a player/any-target filter. `target.kind` is folded into
/// the resulting life total (100 added when `kind == "player"`) so a test
/// can tell, from the life total alone, that the resumed answer really was
/// tagged `"player"` rather than falling through to the `"object"` arm by
/// accident - same "prove the exact resumed value reached the script"
/// approach the object-only test above already established, no new
/// effect-shape key needed for it.
const SELF_REGISTERING_SELECT_PLAYER_THEN_GAIN_LIFE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        local target = Game.SelectTargets({ type = "player" })
        if target.kind == "player" then
            return { gain_life = 100 + target.id }
        end
        return { gain_life = target.id }
    end)
    e:Register()
end
return s
"#;

const SELF_REGISTERING_SELECT_ANY_THEN_GAIN_LIFE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        local target = Game.SelectTargets({ type = "any" })
        if target.kind == "player" then
            return { gain_life = 100 + target.id }
        end
        return { gain_life = target.id }
    end)
    e:Register()
end
return s
"#;

const SELF_REGISTERING_NEVER_YIELDS: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        return { gain_life = 1 }
    end)
    e:Register()
end
return s
"#;

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Select-Target Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "{0}: Gain life equal to a target creature's object id.".to_string(),
        power: None,
        toughness: None,
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

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn non_creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 3,
        name: "Test Non-Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_non_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
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
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
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
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

/// Plays the self-registering land, then activates its (only) ability and
/// pays its `{0}` cost - drains to whatever the operation's own execution
/// produces next (normally either `AwaitingScriptChoice` or, for a script
/// that never yields, straight back to `AwaitingAction`). Returns the land's
/// real post-CR-400.7 battlefield id.
fn play_activate_and_drain(engine: &mut Engine, player: PlayerId, land_in_hand: ObjectId) -> (ObjectId, StepStatus) {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::PlayLand { card: land_in_hand } })
        .expect("player holds priority, land is in hand");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected priority to reopen after playing the land");
    assert_eq!(engine.state().abilities.len(), 1, "the land's activated ability self-registered");

    // The land is played (and re-minted, CR 400.7) after any creature a test
    // pre-seeds directly onto the battlefield - push_bottom appends, so the
    // land is always the *last* entry, never the first.
    let land = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.back().unwrap();
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source: land,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("player controls the land and holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the {{0}} cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine);

    // Paying the cost only commits it - the ability still sits on the stack
    // until priority passes around (CR 117.4). Solo player's first pass
    // already counts as "everyone passed" (consecutive_passes >=
    // turn_order.len() == 1), same convention every other solo fixture uses.
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
    (land, drain(engine))
}

#[test]
fn a_script_calling_select_targets_pauses_with_the_real_filter() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let creature = ObjectId(1);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_TARGET_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    let StepStatus::AwaitingScriptChoice { player, request } = status else {
        panic!("expected the operation to pause on Game.SelectTargets, got {status:?}")
    };
    assert_eq!(player, p0);
    let ScriptChoiceRequest::SelectTarget { filter } = request else {
        panic!("expected a SelectTarget request")
    };
    let TargetFilter::Object { card_types, .. } = filter else {
        panic!("expected an Object filter, got {filter:?}")
    };
    assert_eq!(card_types, Some(CardTypeSet::CREATURE), "the real filter from the script should have reached the pause");
}

#[test]
fn submitting_a_legal_target_resumes_and_finishes_the_operation() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let creature = ObjectId(1);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_TARGET_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    assert!(matches!(status, StepStatus::AwaitingScriptChoice { .. }));

    engine.submit_script_choice(Target::Object(creature)).expect("the creature is a legal target for the request's filter");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        20 + creature.0 as i32,
        "the resumed script's own continuation ran with the exact chosen id"
    );
    assert!(engine.state().stack.is_empty(), "the ability resolved off the stack for good");
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })),
        "the deferred StackObjectResolved must still land once the pause resolves"
    );
}

#[test]
fn submitting_an_illegal_target_is_rejected_and_the_pause_stays_open() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let creature = ObjectId(1);
    let non_creature = ObjectId(2);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_TARGET_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(non_creature, non_creature_definition(), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    play_activate_and_drain(&mut engine, p0, land);

    let err = engine.submit_script_choice(Target::Object(non_creature)).unwrap_err();
    assert!(matches!(err, ScriptChoiceError::InvalidCandidate));
    // Still paused on the same request - nothing was consumed.
    assert!(matches!(engine.step().status, StepStatus::AwaitingScriptChoice { .. }));
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "a rejected answer must not resume the script");
}

#[test]
fn a_script_calling_select_targets_with_type_player_pauses_with_a_player_filter() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_PLAYER_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    let StepStatus::AwaitingScriptChoice { request, .. } = status else {
        panic!("expected the operation to pause on Game.SelectTargets, got {status:?}")
    };
    let ScriptChoiceRequest::SelectTarget { filter } = request else {
        panic!("expected a SelectTarget request")
    };
    assert_eq!(filter, TargetFilter::Player, "the real filter from the script should have reached the pause");
}

#[test]
fn submitting_a_player_target_resumes_with_the_tagged_kind_the_script_can_read() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_PLAYER_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    assert!(matches!(status, StepStatus::AwaitingScriptChoice { .. }));

    engine.submit_script_choice(Target::Player(p0)).expect("p0 is a legal player target");
    drain(&mut engine);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        20 + 100 + p0.0 as i32,
        "the resumed answer must be tagged kind=\"player\", not fall through to the object arm"
    );
}

#[test]
fn submitting_an_object_target_for_a_player_request_is_rejected() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let creature = ObjectId(1);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_PLAYER_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    play_activate_and_drain(&mut engine, p0, land);

    let err = engine.submit_script_choice(Target::Object(creature)).unwrap_err();
    assert!(matches!(err, ScriptChoiceError::InvalidCandidate), "an object can never satisfy a player-only request");
}

#[test]
fn a_script_calling_select_targets_with_type_any_accepts_a_creature() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let creature = ObjectId(1);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_ANY_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    let StepStatus::AwaitingScriptChoice { request, .. } = status else {
        panic!("expected a pause, got {status:?}")
    };
    let ScriptChoiceRequest::SelectTarget { filter } = request else {
        panic!("expected a SelectTarget request")
    };
    assert_eq!(filter, TargetFilter::AnyTarget);

    engine.submit_script_choice(Target::Object(creature)).expect("a creature is a legal any-target");
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 20 + creature.0 as i32);
}

#[test]
fn a_script_calling_select_targets_with_type_any_accepts_a_player() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_SELECT_ANY_THEN_GAIN_LIFE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    assert!(matches!(status, StepStatus::AwaitingScriptChoice { .. }));

    engine.submit_script_choice(Target::Player(p0)).expect("p0 is a legal any-target");
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 20 + 100 + p0.0 as i32);
}

#[test]
fn submit_script_choice_rejects_when_not_awaiting() {
    let mut engine = Engine::new(base_state());
    let err = engine.submit_script_choice(Target::Object(ObjectId(0))).unwrap_err();
    assert!(matches!(err, ScriptChoiceError::NotAwaitingChoice));
}

#[test]
fn a_script_that_never_yields_still_resolves_synchronously() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTERING_NEVER_YIELDS), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    let (_land, status) = play_activate_and_drain(&mut engine, p0, land);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "a script that never calls Game.SelectTargets must resolve synchronously, not pause: {status:?}"
    );
    assert_eq!(engine.state().player(p0).unwrap().life, 21);
}

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::invoke_operation directly, without driving the
// whole Engine pipeline - confirms the coroutine rewrite is transparent for
// a function that never yields (same "Ok(Finished)" outcome and observable
// gain_life effect the old plain `.call()` implementation always had).
// ---------------------------------------------------------------------

#[test]
fn invoke_operation_finishes_synchronously_when_the_script_never_yields() {
    use stackfish_engine::script::ScriptRuntime;
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(source, controller) return { gain_life = 1 } end").unwrap();
    let mut state = base_state();
    let outcome = runtime.invoke_operation(handle, &mut state, ObjectId(0), PlayerId(0));
    assert!(matches!(outcome, Ok(stackfish_engine::script::OperationOutcome::Finished)), "outcome: {outcome:?}");
    assert_eq!(state.player(PlayerId(0)).unwrap().life, 21);
}
