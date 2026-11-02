//! CR 601.2c, 115.6: "up to N" target counts, `TargetRequirement::up_to`, letting a caster choose from 0 to `count` targets.
//! The `ResolveStackTop` fizzle check gates on `!top.targets.is_empty()` and not `req.count > 0`, since a caster may legally choose zero targets (see `zero_chosen_targets_never_produces_a_fizzled_event`).

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, cast_spell, pass_both_and_drain, pay_free_and_drain, try_cast_spell,
    two_player_main_phase_state,
};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn volley_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Volley".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Volley deals 1 damage to each of up to two target creatures."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_volley".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
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
            count: 2,
            up_to: true,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn fizzled(engine: &Engine) -> bool {
    engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }))
}

fn damage_marked_on(engine: &Engine, id: ObjectId) -> u32 {
    engine
        .state()
        .object(id)
        .map(|o| o.damage_marked)
        .unwrap_or(0)
}

fn setup_with_two_creatures() -> (GameState, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = two_player_main_phase_state();
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(
        target_a,
        card_object(
            target_a,
            creature_definition(1, "Bear A"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        target_b,
        card_object(
            target_b,
            creature_definition(2, "Bear B"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target_a);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, volley_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    (state, target_a, target_b, card)
}

#[test]
fn zero_chosen_targets_never_produces_a_fizzled_event() {
    let (state, _target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        !fizzled(&engine),
        "choosing zero targets under 'up to N' is legal - it must resolve, not fizzle, even though req.count is 2"
    );
}

#[test]
fn casting_with_one_of_two_allowed_targets_is_legal() {
    let (state, target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine));
    assert_eq!(damage_marked_on(&engine, target_a), 1);
}

#[test]
fn casting_with_the_maximum_allowed_targets_is_legal() {
    let (state, target_a, target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(
        &mut engine,
        p0,
        card,
        vec![Target::Object(target_a), Target::Object(target_b)],
    );
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine));
    assert_eq!(damage_marked_on(&engine, target_a), 1);
    assert_eq!(damage_marked_on(&engine, target_b), 1);
}

#[test]
fn casting_with_more_than_the_maximum_is_rejected() {
    let (mut state, target_a, target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_c = ObjectId(3);
    state.objects.insert(
        target_c,
        card_object(
            target_c,
            creature_definition(3, "Bear C"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target_c);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(
        &mut engine,
        p0,
        card,
        vec![
            Target::Object(target_a),
            Target::Object(target_b),
            Target::Object(target_c),
        ],
    )
    .unwrap_err();

    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "got {err:?}"
    );
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "cast never happened"
    );
}

#[test]
fn casting_with_a_duplicate_target_is_rejected() {
    let (state, target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(
        &mut engine,
        p0,
        card,
        vec![Target::Object(target_a), Target::Object(target_a)],
    )
    .unwrap_err();

    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "got {err:?}"
    );
}

#[test]
fn the_lone_chosen_target_becoming_illegal_fully_fizzles() {
    let (state, target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a)]);
    pay_free_and_drain(&mut engine);

    engine
        .state_mut()
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .remove(target_a);
    engine.state_mut().objects.remove(&target_a);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        fizzled(&engine),
        "the only chosen target is now illegal - CR 608.2b: the spell doesn't resolve at all"
    );
}
