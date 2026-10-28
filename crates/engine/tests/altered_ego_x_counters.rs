//! CR 707.9e: Altered Ego copies a creature "except it enters with X additional +1/+1 counters".
//! The amount is `CounterAmount::X`, read from the entering object's own `x_value` and kept across chained copy choices.
//! The fixed-count shape is in clone_enters_as_copy.rs.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, player_state, two_player_state as base_two_player_state};

fn creature_definition(name: &str, mana_cost: ManaCost, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn cast_with_x(engine: &mut Engine, player: PlayerId, card: ObjectId, x_value: u32) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(x_value),
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

fn pay_x_and_drain(engine: &mut Engine, x: u32) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's cost, got {status:?}");
    let units = vec![ManaUnit::Colorless; x as usize];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("X colorless mana covers a bare {X} cost exactly");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

const CLONE_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy())
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const ALTERED_EGO_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy({ counter_kind = "PlusOnePlusOne", counter_amount = "X" }))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn x_mana_cost() -> ManaCost {
    ManaCost { symbols: vec![ManaSymbol::X] }
}

#[test]
fn counter_amount_x_decodes_to_the_x_variant() {
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let source = ObjectId(0);
    let definition = creature_definition("Altered Ego", x_mana_cost(), ALTERED_EGO_SCRIPT);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive { payload: ReplacementPayload::EntersAsCopy(exceptions), .. } => {
            assert_eq!(exceptions.enters_with_additional_counters, Some((CounterKind::PlusOnePlusOne, CounterAmount::X)));
        }
        other => panic!("expected an EntersAsCopy spec, got {other:?}"),
    }
}

#[test]
fn counter_amount_neither_a_number_nor_x_is_a_script_error() {
    const BAD_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy({ counter_kind = "PlusOnePlusOne", counter_amount = "Y" }))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;
    let mut state = base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let source = ObjectId(0);
    let definition = creature_definition("Bad Card", x_mana_cost(), BAD_SCRIPT);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let result = ScriptRuntime::new().run_card_script(&mut state, &definition, source);
    assert!(result.is_err(), "\"Y\" is neither a number nor \"X\" - should be rejected at registration time");
}

#[test]
fn choosing_to_copy_applies_x_additional_counters() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state_with_pool(ManaPool { colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, creature_definition("Altered Ego", x_mana_cost(), ALTERED_EGO_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ManaCost { symbols: Vec::new() }, ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast_with_x(&mut engine, p0, card, 3);
    pay_x_and_drain(&mut engine, 3);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else { panic!("expected a copy choice pause, got {status:?}") };

    engine.submit_copy_choice(Some(bear)).expect("the Bear was an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(
        engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        3,
        "CR 707.9e/107.3: the announced X=3 resolves the copy's own additional-counters exception"
    );
}

#[test]
fn choosing_to_copy_with_x_zero_adds_no_counters() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state_with_pool(ManaPool::default());
    state.objects.insert(card, card_object(card, creature_definition("Altered Ego", x_mana_cost(), ALTERED_EGO_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ManaCost { symbols: Vec::new() }, ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast_with_x(&mut engine, p0, card, 0);
    pay_x_and_drain(&mut engine, 0);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else { panic!("expected a copy choice pause, got {status:?}") };

    engine.submit_copy_choice(Some(bear)).expect("the Bear was an offered candidate");
    drain(&mut engine);

    assert_eq!(
        engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        0,
        "CR 107.3b: X=0 resolves to zero counters, not a fixed fallback amount"
    );
}

#[test]
fn declining_a_chained_copy_still_applies_the_original_x() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let clone_link = ObjectId(1);
    let bear = ObjectId(2);
    let mut state = two_player_state_with_pool(ManaPool { colorless: 5, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, creature_definition("Altered Ego", x_mana_cost(), ALTERED_EGO_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(clone_link, creature_definition("Clone", ManaCost { symbols: Vec::new() }, CLONE_SCRIPT), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ManaCost { symbols: Vec::new() }, ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast_with_x(&mut engine, p0, card, 5);
    pay_x_and_drain(&mut engine, 5);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object: entrant, .. } = status else { panic!("expected the first copy choice pause, got {status:?}") };

    engine.submit_copy_choice(Some(clone_link)).expect("chain into the Clone link");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingCopyChoice { .. }), "expected the chained pause, got {status:?}");

    // Declining the chained choice fires Altered Ego's own exception with the X paid at cast time.
    engine.submit_copy_choice(None).expect("declining the chained choice is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");

    assert_eq!(
        engine.state().object(entrant).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        5,
        "the original X=5 survives the chain and the decline"
    );
}
