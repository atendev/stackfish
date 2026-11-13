//! CR 601.2d, 608.2b: an effect divided among its targets. The split is announced with the
//! targets, each legal target takes its own share at resolution, and an illegal target's
//! share is lost.

use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, pass_both_and_drain, pay_free_and_drain,
    two_player_main_phase_state, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const A: ObjectId = ObjectId(0);
const B: ObjectId = ObjectId(1);
const C: ObjectId = ObjectId(2);
const D: ObjectId = ObjectId(3);
const CARD: ObjectId = ObjectId(10);

fn tough_creature(name: &str) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(9)),
        ..minimal_card_definition(
            1,
            name,
            "tough",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn creature_word(count: u32, up_to: bool) -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            excluded_card_types: None,
            excluded_subtypes: None,
            card_types: Some(CardTypeSet::CREATURE),
            subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count,
        up_to,
        at_least: false,
        exclude_source: false,
    }
}

fn any_word(count: u32) -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count,
        up_to: true,
        at_least: false,
        exclude_source: false,
    }
}

/// Arc Lightning's shape: 3 damage divided among up to three targets.
fn arc_lightning() -> CardDefinition {
    CardDefinition {
        target_requirement: Some(TargetSpec::Primitive(any_word(3))),
        effect: Some(AbilityEffect::DividedDamage(3)),
        ..minimal_card_definition(
            20,
            "Arc",
            "arc",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

/// Four tough creatures A to D under P1's control, and `spell` in P0's hand.
fn state_with(spell: CardDefinition) -> GameState {
    let mut state = two_player_main_phase_state();
    for (id, name) in [(A, "A"), (B, "B"), (C, "C"), (D, "D")] {
        state.objects.insert(
            id,
            card_object(id, tough_creature(name), P1, ZoneKind::Battlefield),
        );
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(id);
    }
    state
        .objects
        .insert(CARD, card_object(CARD, spell, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(CARD);
    state
}

fn cast(
    engine: &mut Engine,
    modes: Vec<u32>,
    targets: &[ObjectId],
    division: Vec<u32>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::CastSpell {
            card: CARD,
            modes,
            targets: targets.iter().map(|&id| Target::Object(id)).collect(),
            division,
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    })
}

fn damage(engine: &Engine, id: ObjectId) -> u32 {
    engine.state().object(id).unwrap().damage_marked
}

fn plus_counters(engine: &Engine, id: ObjectId) -> i32 {
    engine
        .state()
        .object(id)
        .unwrap()
        .counters
        .count(&CounterKind::PlusOnePlusOne)
}

fn resolve(engine: &mut Engine) {
    pay_free_and_drain(engine);
    pass_both_and_drain(engine, P0, P1);
}

// CR 601.2d
#[test]
fn each_target_takes_its_announced_share() {
    let mut engine = Engine::new(state_with(arc_lightning()));
    cast(&mut engine, Vec::new(), &[A, B], vec![2, 1]).expect("a valid split");
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.last().unwrap().division, vec![2, 1]);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!((damage(&engine, A), damage(&engine, B)), (2, 1));
}

// CR 601.2d
// A word wider than the total can't give every target at least 1, so the slot caps it.
#[test]
fn a_cast_entry_shows_the_divided_total_and_caps_the_target_count() {
    let wide = CardDefinition {
        target_requirement: Some(TargetSpec::Primitive(any_word(5))),
        ..arc_lightning()
    };
    let engine = Engine::new(state_with(wide));
    let slots = engine
        .legal_actions()
        .into_iter()
        .find_map(|entry| match entry {
            decision::Entry::Cast(entry) if entry.card == CARD => Some(entry.targets),
            _ => None,
        })
        .expect("the spell is castable");
    let decision::EntryTargets::Fixed(slots) = slots else {
        panic!("a non-modal spell has fixed slots");
    };
    assert_eq!(
        slots
            .iter()
            .map(|s| (s.divide_total, s.min, s.max))
            .collect::<Vec<_>>(),
        vec![(Some(3), 0, Some(3))]
    );
}

// CR 601.2d
#[test]
fn a_lone_target_takes_the_whole_total_with_no_division() {
    let mut engine = Engine::new(state_with(arc_lightning()));
    cast(&mut engine, Vec::new(), &[A], Vec::new()).expect("one target sends no division");
    resolve(&mut engine);
    assert_eq!(damage(&engine, A), 3);
}

// CR 601.2d
#[test]
fn a_bad_division_is_rejected_before_anything_changes() {
    let bad: [(&[ObjectId], Vec<u32>); 6] = [
        (&[A, B], Vec::new()),
        (&[A, B], vec![3]),
        (&[A, B], vec![2, 2]),
        (&[A, B], vec![3, 0]),
        (&[A], vec![3]),
        (&[], vec![3]),
    ];
    for (targets, division) in bad {
        let mut engine = Engine::new(state_with(arc_lightning()));
        assert!(
            cast(&mut engine, Vec::new(), targets, division.clone()).is_err(),
            "{targets:?} with {division:?} must be rejected"
        );
        assert!(engine.state().stack.is_empty());
    }
}

// CR 601.2d
#[test]
fn a_division_on_an_undivided_effect_is_rejected() {
    let blast = CardDefinition {
        target_requirement: Some(TargetSpec::Primitive(creature_word(2, false))),
        effect: Some(AbilityEffect::Damage(2)),
        ..minimal_card_definition(
            21,
            "Blast",
            "blast",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    };
    let mut engine = Engine::new(state_with(blast));
    assert!(cast(&mut engine, Vec::new(), &[A, B], vec![1, 1]).is_err());
}

// CR 608.2b
#[test]
fn an_illegal_target_loses_its_share_with_no_redistribution() {
    let mut engine = Engine::new(state_with(arc_lightning()));
    cast(&mut engine, Vec::new(), &[A, B], vec![2, 1]).unwrap();
    pay_free_and_drain(&mut engine);
    engine
        .state_mut()
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .remove(A);
    engine.state_mut().objects.remove(&A);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!(damage(&engine, B), 1, "B keeps its own share, not A's");
}

// CR 608.2b
#[test]
fn the_spell_fizzles_only_when_every_target_is_illegal() {
    let mut engine = Engine::new(state_with(arc_lightning()));
    cast(&mut engine, Vec::new(), &[A, B], vec![1, 2]).unwrap();
    pay_free_and_drain(&mut engine);
    for id in [A, B] {
        engine
            .state_mut()
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .remove(id);
        engine.state_mut().objects.remove(&id);
    }
    pass_both_and_drain(&mut engine, P0, P1);
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }))
    );
}

fn mode(word: TargetRequirement, effect: AbilityEffect) -> SpellMode {
    SpellMode {
        target_requirement: Some(TargetSpec::Primitive(word)),
        effect: Some(effect),
        additional_cost: None,
        pawprint_weight: 0,
    }
}

/// Two divided modes: 3 damage among up to three creatures, and two +1/+1 counters on
/// exactly two creatures.
fn two_divided_modes() -> CardDefinition {
    CardDefinition {
        modes: vec![
            mode(creature_word(3, true), AbilityEffect::DividedDamage(3)),
            mode(
                creature_word(2, false),
                AbilityEffect::DividedCounters {
                    kind: CounterKind::PlusOnePlusOne,
                    total: 2,
                },
            ),
        ],
        mode_choice: ModeChoice::Exactly(2),
        ..minimal_card_definition(
            22,
            "Charm",
            "charm",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::SORCERY,
        )
    }
}

// CR 601.2d, 700.2d
#[test]
fn two_divided_modes_are_checked_and_resolved_separately() {
    for bad in [
        vec![3, 0, 1, 1],
        vec![2, 1, 2, 0],
        vec![2, 1, 1, 2],
        vec![2, 1],
    ] {
        let mut engine = Engine::new(state_with(two_divided_modes()));
        assert!(
            cast(&mut engine, vec![0, 1], &[A, B, C, D], bad.clone()).is_err(),
            "{bad:?} must be rejected"
        );
    }
    let mut engine = Engine::new(state_with(two_divided_modes()));
    cast(&mut engine, vec![0, 1], &[A, B, C, D], vec![2, 1, 1, 1]).expect("both splits valid");
    resolve(&mut engine);
    assert_eq!((damage(&engine, A), damage(&engine, B)), (2, 1));
    assert_eq!(
        (plus_counters(&engine, C), plus_counters(&engine, D)),
        (1, 1)
    );
}

/// 2 damage to one creature, then 3 damage divided among up to three creatures.
fn undivided_then_divided_mode() -> CardDefinition {
    CardDefinition {
        modes: vec![
            mode(creature_word(1, false), AbilityEffect::Damage(2)),
            mode(creature_word(3, true), AbilityEffect::DividedDamage(3)),
        ],
        ..two_divided_modes()
    }
}

// CR 601.2d, 700.2d
#[test]
fn a_share_outside_a_divided_word_of_two_or_more_targets_is_rejected() {
    let bad: [(&[ObjectId], Vec<u32>); 4] = [
        (&[A, B, C], vec![1, 2, 1]),
        (&[A, B, C], vec![2, 1]),
        (&[A, B], vec![0, 3]),
        (&[A, B], vec![0, 0]),
    ];
    for (targets, division) in bad {
        let mut engine = Engine::new(state_with(undivided_then_divided_mode()));
        assert!(
            cast(&mut engine, vec![0, 1], targets, division.clone()).is_err(),
            "{targets:?} with {division:?} must be rejected"
        );
    }
    let mut engine = Engine::new(state_with(undivided_then_divided_mode()));
    cast(&mut engine, vec![0, 1], &[A, B, C], vec![0, 2, 1]).expect("a split on the divided word");
    resolve(&mut engine);
    assert_eq!(
        (damage(&engine, A), damage(&engine, B), damage(&engine, C)),
        (2, 2, 1)
    );
    let mut engine = Engine::new(state_with(undivided_then_divided_mode()));
    cast(&mut engine, vec![0, 1], &[A, B], Vec::new()).expect("a lone divided target");
    resolve(&mut engine);
    assert_eq!((damage(&engine, A), damage(&engine, B)), (2, 3));
}

// CR 601.2d, 700.2d
#[test]
fn a_lone_target_in_a_divided_mode_takes_a_zero_entry_and_the_whole_total() {
    let mut engine = Engine::new(state_with(two_divided_modes()));
    cast(&mut engine, vec![0, 1], &[A, C, D], vec![0, 1, 1]).expect("A takes all 3");
    resolve(&mut engine);
    assert_eq!(damage(&engine, A), 3);
    assert_eq!(
        (plus_counters(&engine, C), plus_counters(&engine, D)),
        (1, 1)
    );
}

fn divided_ability(kind: AbilityKind, effect: AbilityEffect) -> RegisteredAbility {
    RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: CARD,
        kind,
        condition: (kind == AbilityKind::Triggered).then_some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: Some(TargetSpec::Primitive(creature_word(3, true))),
        operation: ScriptHandle(0),
        effect: Some(effect),
        intervening_if: None,
    }
}

fn activate(
    engine: &mut Engine,
    targets: &[ObjectId],
    division: Vec<u32>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::ActivateAbility {
            source: CARD,
            ability_index: 0,
            modes: Vec::new(),
            targets: targets.iter().map(|&id| Target::Object(id)).collect(),
            division,
            chosen_colors: Vec::new(),
            x_value: None,
        },
    })
}

/// The tough creatures, plus a P0 artifact at `CARD` carrying `ability`.
fn state_with_ability(mut state: GameState, ability: RegisteredAbility) -> GameState {
    for (id, name) in [(A, "A"), (B, "B")] {
        state.objects.insert(
            id,
            card_object(id, tough_creature(name), P1, ZoneKind::Battlefield),
        );
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(id);
    }
    let artifact = minimal_card_definition(
        23,
        "Rod",
        "rod",
        ManaCost {
            symbols: Vec::new(),
        },
        CardTypeSet::ARTIFACT,
    );
    state
        .objects
        .insert(CARD, card_object(CARD, artifact, P0, ZoneKind::Battlefield));
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(CARD);
    state.abilities.push(ability);
    state
}

// CR 601.2d, 602.2b
#[test]
fn an_activated_ability_divides_counters_as_announced() {
    let ability = divided_ability(
        AbilityKind::Activated,
        AbilityEffect::DividedCounters {
            kind: CounterKind::PlusOnePlusOne,
            total: 3,
        },
    );
    let mut engine = Engine::new(state_with_ability(two_player_main_phase_state(), ability));
    assert!(activate(&mut engine, &[A, B], vec![2, 2]).is_err());
    activate(&mut engine, &[A, B], vec![1, 2]).expect("a valid split");
    resolve(&mut engine);
    assert_eq!(
        (plus_counters(&engine, A), plus_counters(&engine, B)),
        (1, 2)
    );
}

// CR 601.2d, 603.3d
#[test]
fn a_trigger_target_answer_carries_its_division() {
    let state = GameState {
        phase: Phase::PreGame,
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
    let ability = divided_ability(AbilityKind::Triggered, AbilityEffect::DividedDamage(3));
    let mut engine = Engine::new(state_with_ability(state, ability));
    let status = pass_both_and_drain(&mut engine, P0, P1);
    assert!(
        matches!(status, StepStatus::AwaitingTriggerTargets { .. }),
        "{status:?}"
    );
    let targets = vec![Target::Object(A), Target::Object(B)];
    assert!(
        engine
            .submit_trigger_targets(targets.clone(), vec![3])
            .is_err()
    );
    engine
        .submit_trigger_targets(targets, vec![1, 2])
        .expect("a valid split");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!((damage(&engine, A), damage(&engine, B)), (1, 2));
}

fn load_error(script: &str) -> String {
    let mut state = two_player_main_phase_state();
    let definition = CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            24,
            "Rod",
            "rod",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::ARTIFACT,
        )
    };
    state.objects.insert(
        CARD,
        card_object(CARD, definition.clone(), P0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(CARD);
    let error = ScriptRuntime::new()
        .run_card_script(&mut state, &definition, CARD)
        .expect_err("the script must not load");
    format!("{error:?}")
}

// CR 601.2d
#[test]
fn a_bad_divided_shape_is_a_load_error() {
    let total_zero = "return { register_abilities = function(c) \
        local a = Game.CreateActivatedAbility(c) \
        a:SetTarget({ type = 'object' }) a:SetOperation(Game.Ops.DividedDamage(0)) a:Register() end }";
    assert!(load_error(total_zero).contains("at least 1"));
    let player_counters = "return { register_abilities = function(c) \
        local a = Game.CreateActivatedAbility(c) \
        a:SetTarget({ type = 'player' }) a:SetOperation(Game.Ops.DividedCounters('PlusOnePlusOne', 2)) \
        a:Register() end }";
    assert!(load_error(player_counters).contains("only target objects"));
    let two_words = "return { register_abilities = function(c) \
        local a = Game.CreateActivatedAbility(c) \
        a:SetTarget({ { type = 'object' }, { type = 'object' } }) \
        a:SetOperation(Game.Ops.DividedDamage(2)) a:Register() end }";
    assert!(load_error(two_words).contains("exactly one target word"));
}

const LUA_LAND: &str = "Lua Rod";

fn lua_land_engine(body: &str) -> Engine {
    let mut state = two_player_main_phase_state();
    for (id, name) in [(A, "A"), (B, "B")] {
        state.objects.insert(
            id,
            card_object(id, tough_creature(name), P1, ZoneKind::Battlefield),
        );
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(id);
    }
    let land = CardDefinition {
        script_ref: ScriptReference::Inline(format!(
            "return {{ register_abilities = function(c) {body} end }}"
        )),
        ..minimal_card_definition(
            25,
            LUA_LAND,
            "lua_rod",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::LAND,
        )
    };
    state
        .objects
        .insert(CARD, card_object(CARD, land, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(CARD);
    Engine::new(state)
}

fn play_lua_land(engine: &mut Engine) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PlayLand { card: CARD },
        })
        .expect("p0 holds priority and the land is in hand");
    drain(engine)
}

fn lua_land_id(engine: &Engine) -> ObjectId {
    *engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .find(|&&id| engine.state().objects[&id].definition.name == LUA_LAND)
        .expect("the land is on the battlefield")
}

fn lua_target(engine: &Engine) -> Option<TargetSpec> {
    let source = lua_land_id(engine);
    engine
        .state()
        .abilities
        .iter()
        .find(|ability| ability.source == source)
        .expect("the land registered an ability")
        .target
        .clone()
}

fn lua_activate(
    engine: &mut Engine,
    modes: Vec<u32>,
    division: Vec<u32>,
) -> Result<(), ActionError> {
    let source = lua_land_id(engine);
    engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::ActivateAbility {
            source,
            ability_index: 0,
            modes,
            targets: vec![Target::Object(A), Target::Object(B)],
            division,
            chosen_colors: Vec::new(),
            x_value: None,
        },
    })
}

// CR 601.2d, 602.2b
#[test]
fn a_lua_activated_ability_divides_counters_among_its_counted_word() {
    let mut engine = lua_land_engine(
        "local a = Game.CreateActivatedAbility(c) \
         a:SetTarget({ type = 'object', card_types = { 'Creature' }, count = 3, up_to = true }) \
         a:SetOperation(Game.Ops.DividedCounters('PlusOnePlusOne', 3)) a:Register()",
    );
    play_lua_land(&mut engine);
    assert!(lua_activate(&mut engine, Vec::new(), vec![2, 2]).is_err());
    lua_activate(&mut engine, Vec::new(), vec![1, 2]).expect("a valid split");
    resolve(&mut engine);
    assert_eq!(
        (plus_counters(&engine, A), plus_counters(&engine, B)),
        (1, 2)
    );
}

// CR 601.2d, 603.3d
#[test]
fn a_lua_trigger_divides_damage_among_its_counted_word() {
    let mut engine = lua_land_engine(
        "local e = Game.CreateTriggeredAbility(c) e:SetCondition(Game.Conditions.Enters) \
         e:SetTarget({ type = 'any', count = 3, up_to = true }) \
         e:SetOperation(Game.Ops.DividedDamage(3)) e:Register()",
    );
    let status = play_lua_land(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingTriggerTargets { .. }),
        "{status:?}"
    );
    let targets = vec![Target::Object(A), Target::Object(B)];
    assert!(
        engine
            .submit_trigger_targets(targets.clone(), vec![3])
            .is_err()
    );
    engine
        .submit_trigger_targets(targets, vec![2, 1])
        .expect("a valid split");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert_eq!((damage(&engine, A), damage(&engine, B)), (2, 1));
}

// CR 601.2d, 602.2b
#[test]
fn a_lua_mode_word_takes_its_count_and_a_division() {
    let mut engine = lua_land_engine(
        "local a = Game.CreateActivatedAbility(c) \
         a:SetModes(Game.ModeChoice.Exactly(1), { \
           { target = { type = 'object', card_types = { 'Creature' }, count = 2 }, \
             operation = Game.Ops.DividedCounters('PlusOnePlusOne', 3) }, \
           { operation = Game.Ops.DrawCards(1) } }) \
         a:Register()",
    );
    play_lua_land(&mut engine);
    lua_activate(&mut engine, vec![0], vec![2, 1]).expect("a valid split");
    resolve(&mut engine);
    assert_eq!(
        (plus_counters(&engine, A), plus_counters(&engine, B)),
        (2, 1)
    );
}

fn word_of(spec: Option<TargetSpec>) -> TargetRequirement {
    match spec {
        Some(TargetSpec::Primitive(word)) => word,
        other => panic!("expected one word, got {other:?}"),
    }
}

fn counts(word: &TargetRequirement) -> (u32, bool, bool, bool) {
    (word.count, word.up_to, word.at_least, word.exclude_source)
}

fn registered_target(body: &str) -> Option<TargetSpec> {
    let mut engine = lua_land_engine(body);
    play_lua_land(&mut engine);
    lua_target(&engine)
}

fn activated_with_target(target: &str) -> String {
    format!(
        "local a = Game.CreateActivatedAbility(c) a:SetTarget({target}) \
         a:SetOperation(Game.Ops.Tap) a:Register()"
    )
}

// CR 601.2c, 115.4, 115.6
#[test]
fn set_target_reads_the_count_keys_and_defaults_count_to_one() {
    let bare = word_of(registered_target(&activated_with_target(
        "{ type = 'object' }",
    )));
    assert_eq!(counts(&bare), (1, false, false, false));
    let keyed = word_of(registered_target(&activated_with_target(
        "{ type = 'object', count = 2, up_to = true, exclude_source = true }",
    )));
    assert_eq!(counts(&keyed), (2, true, false, true));
    let any_number = word_of(registered_target(&activated_with_target(
        "{ type = 'object', count = 0, at_least = true }",
    )));
    assert_eq!(counts(&any_number), (0, false, true, false));
    let scripted = word_of(registered_target(&activated_with_target(
        "{ count = 2, filter = function() return true end }",
    )));
    assert!(matches!(scripted.filter, TargetFilter::Script(_)));
    assert_eq!(scripted.count, 2);
    let function = word_of(registered_target(&activated_with_target(
        "function() return true end",
    )));
    assert_eq!(counts(&function), (1, false, false, false));
    let Some(TargetSpec::Multiple(words)) = registered_target(&activated_with_target(
        "{ { type = 'object', count = 2 }, { type = 'player' } }",
    )) else {
        panic!("expected two words");
    };
    assert_eq!(
        words.iter().map(counts).collect::<Vec<_>>(),
        vec![(2, false, false, false), (1, false, false, false)]
    );
}

// CR 601.2c
#[test]
fn a_function_mode_word_stays_one_exact_target() {
    let mut engine = lua_land_engine(
        "local a = Game.CreateActivatedAbility(c) \
         a:SetModes(Game.ModeChoice.Exactly(1), { \
           { target = function() return true end, operation = Game.Ops.Tap }, \
           { operation = Game.Ops.DrawCards(1) } }) \
         a:Register()",
    );
    play_lua_land(&mut engine);
    let source = lua_land_id(&engine);
    let ability = engine
        .state()
        .abilities
        .iter()
        .find(|ability| ability.source == source)
        .unwrap();
    assert_eq!(
        counts(&word_of(ability.modes[0].target_requirement.clone())),
        (1, false, false, false)
    );
}

// CR 601.2c, 115.6
#[test]
fn a_bad_set_target_count_is_a_load_error() {
    let ability = |target: &str| {
        format!(
            "return {{ register_abilities = function(c) {} end }}",
            activated_with_target(target)
        )
    };
    assert!(load_error(&ability("{ type = 'triggering', count = 2 }")).contains("triggering"));
    assert!(load_error(&ability("{ type = 'object', count = -1 }")).contains("whole number"));
    assert!(
        load_error(&ability(
            "{ type = 'object', count = 2, up_to = true, at_least = true }"
        ))
        .contains("both up_to and at_least")
    );
    assert!(load_error(&ability("{ type = 'object', count = 0 }")).contains("at_least = true"));
    let too_many = "return { register_abilities = function(c) \
        local a = Game.CreateActivatedAbility(c) \
        a:SetTarget({ type = 'any', count = 4 }) a:SetOperation(Game.Ops.DividedDamage(3)) \
        a:Register() end }";
    assert!(load_error(too_many).contains("at least 1 of 3"));
}
