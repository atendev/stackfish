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
