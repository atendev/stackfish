//! CR 122.1d: an untap attempt on a permanent with a stun counter removes one stun counter
//! instead, both in the untap step and from `Game.Ops.Untap`, `UntapAll` and modal repeats.

use stackfish_engine::engine::Engine;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    any_target_requirement, card_object, drain, minimal_card_definition, pay_free_and_drain,
    two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BEAR: ObjectId = ObjectId(1);
const OTHER: ObjectId = ObjectId(2);
const SPELL: ObjectId = ObjectId(50);

const GAIN_ON_UNTAP: &str = r#"
local s = {}
function s.register_abilities(c)
    local t = Game.CreateTriggeredAbility(c)
    t:SetCondition(Game.Conditions.BecomesUntapped)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

const ENTERS_STUNNED: &str = r#"
local s = {}
function s.register_abilities(c)
    local tapped = Game.CreateReplacementEffect(c)
    tapped:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    tapped:SetPayload(Game.ReplacementPayloads.EntersTapped)
    tapped:SetAffected(Game.Affected.Self)
    tapped:Register()

    local stun = Game.CreateReplacementEffect(c)
    stun:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    stun:SetPayload(Game.ReplacementPayloads.EntersWithCounters("Stun", 1))
    stun:SetAffected(Game.Affected.Self)
    stun:Register()
end
return s
"#;

fn no_cost() -> ManaCost {
    ManaCost {
        symbols: Vec::new(),
    }
}

fn creature(id: u64, script: Option<&str>) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: script
            .map(|s| ScriptReference::Inline(s.to_string()))
            .unwrap_or(ScriptReference::ByKey("bear".to_string())),
        ..minimal_card_definition(id, "Bear", "bear", no_cost(), CardTypeSet::CREATURE)
    }
}

fn base_state(phase: Phase, step: Step, active: PlayerId) -> GameState {
    let mut state = GameState {
        phase,
        step,
        active_player: active,
        priority_player: active,
        turn_number: 2,
        last_active_turn_number: std::collections::HashMap::from([(P0, 1), (P1, 2)]),
        ..two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
            ],
        ))
    };
    // Each player draws on later turns, and an empty library loses the game (CR 704.5b).
    for player in [P0, P1] {
        for n in 0..8 {
            let id = ObjectId(200 + 10 * player.0 as u64 + n);
            state.objects.insert(
                id,
                card_object(id, creature(200 + n, None), player, ZoneKind::Library),
            );
            state
                .zone_mut(ZoneKind::Library, Some(player))
                .unwrap()
                .push_bottom(id);
        }
    }
    state
}

fn add_permanent(
    state: &mut GameState,
    runtime: &mut ScriptRuntime,
    id: ObjectId,
    script: Option<&str>,
    tapped: bool,
    stun: i32,
) {
    let mut object = card_object(id, creature(id.0, script), P0, ZoneKind::Battlefield);
    object.tapped = tapped;
    if stun > 0 {
        let timestamp = state.allocate_sequence_no();
        object.counters.add(CounterKind::Stun, stun, timestamp);
    }
    state.objects.insert(id, object);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    // Fixture permanents never "entered", so their script is registered by hand.
    if script.is_some() {
        let definition = state.objects[&id].definition.clone();
        runtime
            .run_card_script(state, &definition, id)
            .expect("script registers");
    }
}

fn add_to_hand(state: &mut GameState, id: ObjectId, definition: CardDefinition) {
    state
        .objects
        .insert(id, card_object(id, definition, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(id);
}

fn spell(effect: AbilityEffect, targeted: bool) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: targeted.then(any_target_requirement),
        ..minimal_card_definition(99, "Test Spell", "unused", no_cost(), CardTypeSet::INSTANT)
    }
}

fn pass(engine: &mut Engine) {
    let player = engine.state().priority_player;
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .expect("a priority window is open");
    drain(engine);
}

/// Passes until P0's next untap step has run and its upkeep holds priority.
fn to_next_p0_upkeep(engine: &mut Engine) {
    let start = engine.state().turn_number;
    for _ in 0..60 {
        pass(engine);
        let state = engine.state();
        if state.turn_number != start && state.active_player == P0 && state.step == Step::Upkeep {
            return;
        }
    }
    panic!("never reached P0's next upkeep");
}

fn cast(engine: &mut Engine, card: ObjectId, modes: Vec<u32>, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes,
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("P0 holds priority in its main phase");
    pay_free_and_drain(engine);
    pass(engine);
    pass(engine);
}

fn stun(engine: &Engine, id: ObjectId) -> i32 {
    engine.state().objects[&id]
        .counters
        .count(&CounterKind::Stun)
}

fn tapped(engine: &Engine, id: ObjectId) -> bool {
    engine.state().objects[&id].tapped
}

fn untapped_events(engine: &Engine, id: ObjectId) -> usize {
    engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(e.kind, GameEventKind::Untapped { object } if object == id))
        .count()
}

fn stun_removals(engine: &Engine, id: ObjectId) -> usize {
    engine
        .state()
        .ledger
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                GameEventKind::CounterAdded { object, kind: CounterKind::Stun, amount: -1 }
                    if object == id
            )
        })
        .count()
}

/// P1's end step, with P0's untap step next.
fn before_p0_untap(permanents: &[(ObjectId, bool, i32)]) -> Engine {
    let mut state = base_state(Phase::Ending, Step::End, P1);
    let mut runtime = ScriptRuntime::new();
    for (id, is_tapped, stun) in permanents {
        add_permanent(&mut state, &mut runtime, *id, None, *is_tapped, *stun);
    }
    Engine::new(state)
}

fn main_phase(permanents: &[(ObjectId, Option<&str>, bool, i32)], spell: CardDefinition) -> Engine {
    let mut state = base_state(Phase::Main, Step::PreCombatMain, P0);
    let mut runtime = ScriptRuntime::new();
    for (id, script, is_tapped, stun) in permanents {
        add_permanent(&mut state, &mut runtime, *id, *script, *is_tapped, *stun);
    }
    add_to_hand(&mut state, SPELL, spell);
    Engine::new(state)
}

// CR 122.1d, 502.3
#[test]
fn untap_step_removes_one_stun_counter_instead_of_untapping() {
    let mut engine = before_p0_untap(&[(BEAR, true, 1)]);
    to_next_p0_upkeep(&mut engine);

    assert_eq!(stun(&engine, BEAR), 0);
    assert!(tapped(&engine, BEAR));
    assert_eq!(untapped_events(&engine, BEAR), 0);
    assert_eq!(stun_removals(&engine, BEAR), 1);
}

// CR 122.1d, 614.5
#[test]
fn two_stun_counters_take_two_untap_steps_before_the_permanent_untaps() {
    let mut engine = before_p0_untap(&[(BEAR, true, 2)]);
    to_next_p0_upkeep(&mut engine);
    assert_eq!((stun(&engine, BEAR), tapped(&engine, BEAR)), (1, true));
    to_next_p0_upkeep(&mut engine);
    assert_eq!((stun(&engine, BEAR), tapped(&engine, BEAR)), (0, true));
    to_next_p0_upkeep(&mut engine);
    assert_eq!((stun(&engine, BEAR), tapped(&engine, BEAR)), (0, false));
    assert_eq!(untapped_events(&engine, BEAR), 1);
}

// CR 701.26b
#[test]
fn an_untapped_permanent_keeps_its_stun_counter_through_the_untap_step() {
    let mut engine = before_p0_untap(&[(BEAR, false, 1)]);
    to_next_p0_upkeep(&mut engine);

    assert_eq!(stun(&engine, BEAR), 1);
    assert_eq!(stun_removals(&engine, BEAR), 0);
}

// CR 702.26b
#[test]
fn a_phased_out_stunned_permanent_keeps_its_tapped_flag_and_counter() {
    let mut state = base_state(Phase::Ending, Step::End, P1);
    let mut runtime = ScriptRuntime::new();
    add_permanent(&mut state, &mut runtime, BEAR, None, true, 1);
    state.objects.get_mut(&BEAR).unwrap().phased_out = true;
    let mut engine = Engine::new(state);
    to_next_p0_upkeep(&mut engine);

    assert!(tapped(&engine, BEAR));
    assert_eq!(stun(&engine, BEAR), 1);
    assert_eq!(untapped_events(&engine, BEAR), 0);
}

// CR 122.1d, 701.26b
#[test]
fn untap_effect_on_a_stunned_creature_removes_a_counter_and_leaves_it_tapped() {
    let mut engine = main_phase(&[(BEAR, None, true, 1)], spell(AbilityEffect::Untap, true));
    cast(&mut engine, SPELL, Vec::new(), vec![Target::Object(BEAR)]);

    assert!(tapped(&engine, BEAR));
    assert_eq!(stun(&engine, BEAR), 0);
    assert_eq!(untapped_events(&engine, BEAR), 0);
}

// CR 122.1d, 701.26b
#[test]
fn untap_all_untaps_plain_creatures_and_removes_counters_from_stunned_ones() {
    let filter = EffectFilter {
        controller: None,
        card_types: Some(CardTypeSet::CREATURE),
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        self_only: false,
    };
    let mut engine = main_phase(
        &[(BEAR, None, true, 1), (OTHER, None, true, 0)],
        spell(AbilityEffect::UntapAll { filter }, false),
    );
    cast(&mut engine, SPELL, Vec::new(), Vec::new());

    assert_eq!((stun(&engine, BEAR), tapped(&engine, BEAR)), (0, true));
    assert!(!tapped(&engine, OTHER));
    assert_eq!(untapped_events(&engine, OTHER), 1);
}

// CR 603.2e, 614.6
#[test]
fn a_replaced_untap_fires_no_becomes_untapped_trigger() {
    let mut engine = main_phase(
        &[(BEAR, Some(GAIN_ON_UNTAP), true, 1)],
        spell(AbilityEffect::Untap, true),
    );
    cast(&mut engine, SPELL, Vec::new(), vec![Target::Object(BEAR)]);

    assert!(engine.state().stack.is_empty(), "nothing triggered");
    assert_eq!(engine.state().player(P0).unwrap().life, 20);

    // Without the counter the same untap does trigger.
    let mut engine = main_phase(
        &[(BEAR, Some(GAIN_ON_UNTAP), true, 0)],
        spell(AbilityEffect::Untap, true),
    );
    cast(&mut engine, SPELL, Vec::new(), vec![Target::Object(BEAR)]);
    pass(&mut engine);
    pass(&mut engine);
    assert_eq!(engine.state().player(P0).unwrap().life, 21);
}

// CR 122.1d, 614.5
#[test]
fn one_stun_counter_and_two_untaps_in_one_resolution_end_untapped() {
    let twice = CardDefinition {
        modes: vec![SpellMode {
            target_requirement: Some(any_target_requirement()),
            effect: Some(AbilityEffect::Untap),
            additional_cost: None,
            pawprint_weight: 0,
        }],
        mode_choice: ModeChoice::ExactlyRepeatable(2),
        ..minimal_card_definition(99, "Twice", "unused", no_cost(), CardTypeSet::INSTANT)
    };
    let mut engine = main_phase(&[(BEAR, None, true, 1)], twice);
    cast(
        &mut engine,
        SPELL,
        vec![0, 0],
        vec![Target::Object(BEAR), Target::Object(BEAR)],
    );

    assert_eq!(stun_removals(&engine, BEAR), 1);
    assert_eq!(stun(&engine, BEAR), 0);
    assert!(!tapped(&engine, BEAR));
    assert_eq!(untapped_events(&engine, BEAR), 1);
}

// CR 122.1d, 614.1c
#[test]
fn a_creature_that_enters_tapped_and_stunned_loses_the_counter_at_the_next_untap_step() {
    let mut state = base_state(Phase::Main, Step::PreCombatMain, P0);
    state.turn_number = 3;
    state.last_active_turn_number = std::collections::HashMap::from([(P0, 3), (P1, 2)]);
    add_to_hand(&mut state, SPELL, creature(SPELL.0, Some(ENTERS_STUNNED)));
    let mut engine = Engine::new(state);
    cast(&mut engine, SPELL, Vec::new(), Vec::new());
    // CR 400.7: the permanent is a new object.
    let entered = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents[0];
    assert_eq!(
        (stun(&engine, entered), tapped(&engine, entered)),
        (1, true)
    );

    to_next_p0_upkeep(&mut engine);
    assert_eq!(
        (stun(&engine, entered), tapped(&engine, entered)),
        (0, true)
    );
    to_next_p0_upkeep(&mut engine);
    assert!(!tapped(&engine, entered));
}

// CR 122.1d
#[test]
fn lua_names_stun_as_a_known_counter_kind() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _) = runtime
        .load_operation("return Game.Ops.Counters(Game.Counter.Stun, 2)")
        .expect("Game.Counter.Stun loads");
    assert!(matches!(
        effect,
        Some(AbilityEffect::Counters {
            kind: CounterKind::Stun,
            amount: 2
        })
    ));
    let (effect, _) = runtime
        .load_operation("return Game.Ops.Counters(\"Stun\", 1)")
        .expect("the \"Stun\" name loads");
    assert!(matches!(
        effect,
        Some(AbilityEffect::Counters {
            kind: CounterKind::Stun,
            amount: 1
        })
    ));
}
