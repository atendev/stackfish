//! Phase 5 vertical slice: backlog item 11c — commander marking + casting
//! from the command zone (CR 903.3/903.6/903.8), plus CR 903.9a (owner may
//! return a commander from graveyard/exile to the command zone). Foundation
//! for 11d (commander tax) and 11e (commander damage production) — neither
//! is implemented here.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{CommanderReturnChoiceError, Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(
    id: ObjectId,
    definition: CardDefinition,
    owner: PlayerId,
    zone: ZoneKind,
    is_commander: bool,
) -> GameObject {
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
        is_commander,
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

/// Two players, ready for priority at `(Main, PreCombatMain)`, every zone
/// kind (including command) registered for both.
fn two_player_state() -> GameState {
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
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Command, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Command, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
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

fn put_in_zone(state: &mut GameState, object: GameObject, owner: Option<PlayerId>) {
    let kind = object.zone;
    let id = object.object_id;
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
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

/// Every test card here has an empty `mana_cost` ({0}) - CR 118.5a still
/// requires an explicit empty `PaymentPlan`, no auto-pay, so casting always
/// pauses at `AwaitingPayment` once before continuing.
fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
            },
        })
        .expect("player holds priority");
    let status = drain(engine);
    if matches!(status, StepStatus::AwaitingPayment { .. }) {
        engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
        return drain(engine);
    }
    status
}

// ---------------------------------------------------------------------
// Marking (CR 903.3) + casting from the command zone (CR 903.8)
// ---------------------------------------------------------------------

#[test]
fn casting_a_commander_from_the_command_zone_is_legal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(commander, creature_definition("General"), p0, ZoneKind::Command, true),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let status = cast_spell(&mut engine, p0, commander);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");

    // Marking survives the Command -> Stack remap (CR 400.7 new id).
    let on_stack = engine.state().stack.last().expect("cast onto the stack");
    let new_id = on_stack.stack_id;
    assert!(engine.state().object(new_id).unwrap().is_commander);
    assert!(engine.state().zone(ZoneKind::Command, Some(p0)).unwrap().contents.is_empty());
}

#[test]
fn commander_marking_survives_stack_to_battlefield_too() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(commander, creature_definition("General"), p0, ZoneKind::Command, true),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, commander);
    // Both players pass with a permanent spell on the stack - it resolves
    // to the battlefield (CR 608.2m).
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");

    let on_battlefield = engine
        .state()
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Battlefield)
        .expect("resolved onto the battlefield");
    assert!(on_battlefield.is_commander);
}

#[test]
fn casting_a_non_commander_card_from_the_command_zone_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    // Not a legal setup under real rules, but the engine must not silently
    // permit it - Command-zone permission is gated on is_commander, not on
    // the zone alone.
    let not_a_commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(not_a_commander, creature_definition("Impostor"), p0, ZoneKind::Command, false),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let status = cast_spell(&mut engine, p0, not_a_commander);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "got {status:?}");
}

#[test]
fn casting_someone_elses_commander_from_the_command_zone_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander = ObjectId(0);
    // p1's commander, sitting in p1's command zone.
    put_in_zone(
        &mut state,
        card_object(commander, creature_definition("General"), p1, ZoneKind::Command, true),
        Some(p1),
    );

    let mut engine = Engine::new(state);
    let status = cast_spell(&mut engine, p0, commander);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "got {status:?}");
}

// ---------------------------------------------------------------------
// Real game setup (CR 903.6): Engine::new_game
// ---------------------------------------------------------------------

#[test]
fn new_game_places_commanders_in_the_command_zone_not_the_library() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let config = GameConfig {
        players: vec![
            PlayerConfig {
                player_id: p0,
                library: vec![creature_definition("Forest"), creature_definition("Bear")],
                commanders: vec![creature_definition("General")],
                life_override: Some(40),
            },
            PlayerConfig {
                player_id: p1,
                library: vec![creature_definition("Island")],
                commanders: Vec::new(),
                life_override: Some(40),
            },
        ],
        turn_order: vec![p0, p1],
        settings: GameSettings { shuffle_libraries: false, ..GameSettings::default() },
        rng_seed: 1,
    };

    let engine = Engine::new_game(config);

    let p0_command = engine.state().zone(ZoneKind::Command, Some(p0)).unwrap();
    assert_eq!(p0_command.contents.len(), 1);
    let commander_id = p0_command.contents[0];
    assert!(engine.state().object(commander_id).unwrap().is_commander);
    assert_eq!(engine.state().object(commander_id).unwrap().definition.name, "General");

    // The commander did NOT get shuffled into the library alongside it.
    let p0_library = engine.state().zone(ZoneKind::Library, Some(p0)).unwrap();
    assert_eq!(p0_library.contents.len(), 2, "only the two real library cards, not the commander");
    for &id in &p0_library.contents {
        assert!(!engine.state().object(id).unwrap().is_commander);
    }

    // A player with no commanders gets an empty (but present) command zone.
    assert!(engine.state().zone(ZoneKind::Command, Some(p1)).unwrap().contents.is_empty());
}

// ---------------------------------------------------------------------
// CR 903.9a: owner may return a commander from graveyard/exile
// ---------------------------------------------------------------------

#[test]
fn a_dead_commander_pauses_for_the_return_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(commander, creature_definition("General"), p0, ZoneKind::Graveyard, true),
        Some(p0),
    );

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    assert!(
        matches!(
            outcome,
            SbaOutcome::NeedsCommanderReturnChoice { player, commander: c, .. }
                if player == p0 && c == commander
        ),
        "a commander freshly in the graveyard must pause for the 903.9a choice"
    );
}

#[test]
fn declining_the_return_leaves_the_commander_in_the_graveyard_and_never_asks_again() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(commander, creature_definition("General"), p0, ZoneKind::Graveyard, true),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    // Any command drives the pipeline into an SBA pass; a bare pass from
    // the active player is the least eventful way to do that.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingCommanderReturnChoice { player, commander: c } if player == p0 && c == commander),
        "got {status:?}"
    );

    engine.submit_commander_return_choice(false).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "got {status:?}");

    assert!(engine.state().object(commander).is_some(), "declined - stays exactly where it was");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents, vec![commander]);
    assert!(engine.state().commander_return_offered.contains(&commander));
}

#[test]
fn accepting_the_return_moves_the_commander_to_the_command_zone_in_recastable_shape() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(commander, creature_definition("General"), p0, ZoneKind::Graveyard, true),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    engine.submit_commander_return_choice(true).unwrap();
    drain(&mut engine);

    assert!(engine.state().object(commander).is_none(), "old id remapped away, CR 400.7");
    assert!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.is_empty());
    let command_zone = engine.state().zone(ZoneKind::Command, Some(p0)).unwrap();
    assert_eq!(command_zone.contents.len(), 1);
    let new_id = command_zone.contents[0];
    let returned = engine.state().object(new_id).unwrap();
    assert!(returned.is_commander);
    assert_eq!(returned.owner, p0);
    // Recastability itself (is_commander + Command zone + owner match) is
    // exactly what `casting_a_commander_from_the_command_zone_is_legal`
    // already proves against a fresh fixture - not re-driven here.
}

#[test]
fn a_non_commander_creature_dying_never_pauses_for_the_return_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let mundane = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(mundane, creature_definition("Bear"), p0, ZoneKind::Graveyard, false),
        Some(p0),
    );

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    assert!(matches!(outcome, SbaOutcome::Progressed(..)), "got a pause for a non-commander");
}

#[test]
fn submit_commander_return_choice_rejects_when_not_awaiting() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    put_in_zone(
        &mut state,
        card_object(ObjectId(0), creature_definition("Bear"), p0, ZoneKind::Battlefield, false),
        None,
    );
    let mut engine = Engine::new(state);

    let result = engine.submit_commander_return_choice(true);
    assert!(matches!(result, Err(CommanderReturnChoiceError::NotAwaitingChoice)));
}
