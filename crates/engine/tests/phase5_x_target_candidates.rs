//! Phase 5 slice (ticket 031): an entry's target candidates when a
//! `mana_value` filter mentions X. The entry is listed before the caster
//! announces X, so such a word lists every candidate legal for some X, flagged
//! `x_dependent`, and the engine checks the announced pair (CR 107.3a,
//! 601.2b-c, 602.2b).

use std::collections::HashMap;

use stackfish_engine::decision::{Answer, Entry, EntryTargets, Reply};
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn spell_definition(name: &str, mana_value: u32) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            1,
            name,
            name,
            ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(mana_value))] },
            CardTypeSet::CREATURE,
        )
    }
}

fn x_requirement(comparison: ManaValueComparison) -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            card_types: None,
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: Some(ZoneKind::Stack),
            mana_value: Some(ManaValueRequirement { comparison, bound: ManaValueBound::X }),
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn fixed_requirement() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            card_types: None,
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: Some(ZoneKind::Stack),
            mana_value: Some(ManaValueRequirement { comparison: ManaValueComparison::Equal, bound: ManaValueBound::Fixed(2) }),
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn x_counterspell(spec: TargetSpec) -> CardDefinition {
    CardDefinition {
        mana_cost: ManaCost { symbols: vec![ManaSymbol::X] },
        target_requirement: Some(spec),
        effect: Some(AbilityEffect::Counter),
        ..minimal_card_definition(30, "X Counter", "X Counter", ManaCost { symbols: vec![ManaSymbol::X] }, CardTypeSet::INSTANT)
    }
}

fn put(state: &mut GameState, owner: Option<PlayerId>, object: GameObject) {
    let (zone, id) = (object.zone, object.object_id);
    state.zone_mut(zone, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn push_spell(state: &mut GameState, id: u64, mana_value: u32) {
    let id = ObjectId(id);
    state.objects.insert(id, card_object(id, spell_definition("Bear", mana_value), P1, ZoneKind::Stack));
    state.stack.push(StackObject {
        stack_id: id,
        kind: StackEntryKind::Spell,
        source: id,
        source_lki: None,
        owner: P1,
        controller: P1,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });
}

/// P0 holds priority with an X counterspell in hand (id 9) over spells of
/// mana value 0, 2 and 5 (ids 1, 2, 3).
fn state_with(counter: CardDefinition) -> GameState {
    let mut state = GameState {
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    push_spell(&mut state, 1, 0);
    push_spell(&mut state, 2, 2);
    push_spell(&mut state, 3, 5);
    put(&mut state, Some(P0), card_object(ObjectId(9), counter, P0, ZoneKind::Hand));
    state.players[0].mana_pool.colorless = 10;
    state
}

fn cast_slot(engine: &Engine) -> stackfish_engine::decision::TargetSlot {
    let entries = engine.legal_actions();
    let Some(Entry::Cast(cast)) = entries.iter().find(|e| matches!(e, Entry::Cast(c) if c.card == ObjectId(9))) else {
        panic!("the X counterspell is not listed: {entries:?}");
    };
    assert!(cast.x);
    let EntryTargets::Fixed(slots) = &cast.targets else { panic!("not modal") };
    assert_eq!(slots.len(), 1);
    slots[0].clone()
}

fn cast_answer(engine: &mut Engine, target: u64, x_value: u32) -> Result<(), stackfish_engine::decision::Rejection> {
    let decision = engine.open_decision().expect("a Decision is open");
    let kind = PlayerActionKind::CastSpell {
        card: ObjectId(9),
        modes: Vec::new(),
        targets: vec![Target::Object(ObjectId(target))],
        x_value: Some(x_value),
        alternative_cost: None,
        pay_buyback: false,
    };
    engine.answer(P0, Answer { decision: decision.id, reply: Reply::Action(PlayerAction { player: P0, kind }) })
}

fn all_spells() -> Vec<Target> {
    vec![Target::Object(ObjectId(1)), Target::Object(ObjectId(2)), Target::Object(ObjectId(3))]
}

#[test]
fn an_equal_x_word_lists_every_spell_whatever_its_mana_value() {
    let engine = Engine::new(state_with(x_counterspell(x_requirement(ManaValueComparison::Equal))));
    let slot = cast_slot(&engine);
    assert_eq!(slot.candidates, all_spells());
    assert!(slot.x_dependent);
}

#[test]
fn at_most_and_at_least_x_words_list_every_spell_too() {
    for comparison in [ManaValueComparison::AtMost, ManaValueComparison::AtLeast] {
        let engine = Engine::new(state_with(x_counterspell(x_requirement(comparison))));
        let slot = cast_slot(&engine);
        assert_eq!(slot.candidates, all_spells(), "{comparison:?}");
        assert!(slot.x_dependent, "{comparison:?}");
    }
}

#[test]
fn a_fixed_mana_value_word_is_exact_and_not_flagged() {
    let engine = Engine::new(state_with(x_counterspell(fixed_requirement())));
    let slot = cast_slot(&engine);
    assert_eq!(slot.candidates, vec![Target::Object(ObjectId(2))]);
    assert!(!slot.x_dependent);
}

#[test]
fn an_answer_whose_x_matches_the_target_is_accepted() {
    let mut engine = Engine::new(state_with(x_counterspell(x_requirement(ManaValueComparison::Equal))));
    cast_answer(&mut engine, 3, 5).expect("X = 5 targets the mana value 5 spell");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's cost, got {status:?}");
}

#[test]
fn an_answer_whose_x_misses_the_target_is_rejected_and_changes_nothing() {
    let mut engine = Engine::new(state_with(x_counterspell(x_requirement(ManaValueComparison::Equal))));
    let before = engine.state().stack.len();
    let hand_before = engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents.clone();
    let rejection = cast_answer(&mut engine, 3, 2).expect_err("X = 2 cannot target a mana value 5 spell");
    assert!(!rejection.message.is_empty());
    assert_eq!(engine.state().stack.len(), before);
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(P0)).unwrap().contents, hand_before);
    assert!(engine.open_decision().is_some(), "the Decision stays open");
    cast_answer(&mut engine, 3, 5).expect("a corrected Answer is accepted");
}

#[test]
fn at_most_x_accepts_only_a_big_enough_x() {
    let mut engine = Engine::new(state_with(x_counterspell(x_requirement(ManaValueComparison::AtMost))));
    cast_answer(&mut engine, 3, 4).expect_err("mana value 5 is over X = 4");
    cast_answer(&mut engine, 3, 5).expect("mana value 5 is at most X = 5");
}

#[test]
fn an_activated_ability_with_an_x_cost_lists_the_superset_and_rechecks() {
    let mut state = state_with(minimal_card_definition(
        31,
        "Idle",
        "Idle",
        ManaCost { symbols: Vec::new() },
        CardTypeSet::SORCERY,
    ));
    put(
        &mut state,
        None,
        card_object(ObjectId(7), spell_definition("Wand", 0), P0, ZoneKind::Battlefield),
    );
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(7),
        source: ObjectId(7),
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec::mana_only(ManaCost { symbols: vec![ManaSymbol::X] })),
        target: Some(x_requirement(ManaValueComparison::Equal)),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Counter),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);
    let entries = engine.legal_actions();
    let Some(Entry::Activate(activate)) = entries.iter().find(|e| matches!(e, Entry::Activate(a) if a.source == ObjectId(7))) else {
        panic!("the ability is not listed: {entries:?}");
    };
    assert!(activate.x);
    assert_eq!(activate.targets[0].candidates, all_spells());
    assert!(activate.targets[0].x_dependent);

    let activate_with = |engine: &mut Engine, x_value: u32| {
        let decision = engine.open_decision().expect("a Decision is open");
        let kind = PlayerActionKind::ActivateAbility {
            source: ObjectId(7),
            ability_index: 0,
            targets: vec![Target::Object(ObjectId(3))],
            chosen_colors: Vec::new(),
            x_value: Some(x_value),
        };
        engine.answer(P0, Answer { decision: decision.id, reply: Reply::Action(PlayerAction { player: P0, kind }) })
    };
    activate_with(&mut engine, 2).expect_err("X = 2 cannot target a mana value 5 spell");
    activate_with(&mut engine, 5).expect("X = 5 can");
}
