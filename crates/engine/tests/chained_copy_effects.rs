//! CR 707.9e: if another copy effect is applied after a copy effect with an additional-effect exception, that exception does not happen.
//! Altered Ego copies Clone, Clone's copy ability picks a second creature, and the extra counters never appear.
//! `submit_copy_choice` re-enters `AwaitingCopyChoice` and defers counters and a linked granted ability (CR 707.9a) through `apply_copy_additional_effect`.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, script: &str) -> CardDefinition {
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

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
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
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

const SPARK_DOUBLE_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy({ counter = "PlusOnePlusOne", amount = 1 }))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

fn setup_chain_scenario() -> (Engine, PlayerId, PlayerId, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let entrant = ObjectId(0);
    let clone_link = ObjectId(1);
    let bear = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(entrant, card_object(entrant, creature_definition("Spark Double", SPARK_DOUBLE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(entrant);
    put_on_battlefield(&mut state, card_object(clone_link, creature_definition("Clone", CLONE_SCRIPT), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, entrant);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    (engine, p0, p1, entrant, clone_link, bear)
}

#[test]
fn choosing_a_chain_link_re_pauses_instead_of_finishing() {
    let (mut engine, ..) = setup_chain_scenario();
    let (clone_link, entrant) = {
        let StepStatus::AwaitingCopyChoice { object, ref candidates, .. } = drain(&mut engine) else {
            panic!("expected the first copy choice pause")
        };
        // candidates: entrant itself, the Clone link, and the Bear.
        assert_eq!(candidates.len(), 3);
        (candidates.iter().copied().find(|&id| id != object && id.0 == 1).unwrap(), object)
    };

    engine.submit_copy_choice(Some(clone_link)).expect("the Clone link was an offered candidate");
    let status = drain(&mut engine);
    let StepStatus::AwaitingCopyChoice { player, object, candidates } = status else {
        panic!("expected a CHAINED copy choice pause (the Clone link's own EntersAsCopy), got {status:?}")
    };
    assert_eq!(object, entrant, "still the same entering object being resolved");
    assert_eq!(player, PlayerId(0));
    assert_eq!(candidates.len(), 3, "the entrant, the Clone link, and the Bear are all still legal candidates");

    assert_eq!(
        engine.state().object(entrant).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        0,
        "CR 707.9e: the first round's additional-effect exception hasn't applied yet - still pending the chain"
    );
}

#[test]
fn a_completed_chain_suppresses_the_first_rounds_additional_effect_exception() {
    let (mut engine, p0, ..) = setup_chain_scenario();
    let StepStatus::AwaitingCopyChoice { object: entrant, ref candidates, .. } = drain(&mut engine) else {
        panic!("expected the first copy choice pause")
    };
    let clone_link = candidates.iter().copied().find(|&id| id.0 == 1).unwrap();

    engine.submit_copy_choice(Some(clone_link)).expect("chain into the Clone link");
    let StepStatus::AwaitingCopyChoice { candidates, .. } = drain(&mut engine) else {
        panic!("expected the chained pause")
    };
    let bear = candidates.iter().copied().find(|&id| id.0 == 2).unwrap();

    engine.submit_copy_choice(Some(bear)).expect("finish the chain by copying the Bear");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");

    let runtime = ScriptRuntime::new();
    let now = stackfish_engine::layers::effective_characteristics(engine.state(), &runtime, entrant);
    assert_eq!(now.name, "Bear", "the entrant ends up as a copy of the FINAL link in the chain, not the first");
    assert_eq!(
        engine.state().object(entrant).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        0,
        "CR 707.9e: a second copy effect really was applied, so Spark Double's own exception never happens"
    );
    assert_eq!(engine.state().object(entrant).unwrap().copy_generation, 2, "two real, distinct copy applications occurred");
    assert_eq!(
        engine
            .state()
            .ledger
            .iter()
            .filter(|e| matches!(e.kind, GameEventKind::BecameCopy { object: o, .. } if o == entrant))
            .count(),
        2,
        "both copy applications are logged as real BecameCopy events"
    );
}

#[test]
fn declining_the_chained_choice_still_applies_the_first_rounds_exception() {
    let (mut engine, p0, ..) = setup_chain_scenario();
    let StepStatus::AwaitingCopyChoice { object: entrant, ref candidates, .. } = drain(&mut engine) else {
        panic!("expected the first copy choice pause")
    };
    let clone_link = candidates.iter().copied().find(|&id| id.0 == 1).unwrap();

    engine.submit_copy_choice(Some(clone_link)).expect("chain into the Clone link");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingCopyChoice { .. }), "expected the chained pause, got {status:?}");

    // Declining the chained choice applies no second copy effect, so CR 707.9e's condition is unmet and the exception fires.
    engine.submit_copy_choice(None).expect("declining the chained choice is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");

    let runtime = ScriptRuntime::new();
    let now = stackfish_engine::layers::effective_characteristics(engine.state(), &runtime, entrant);
    assert_eq!(now.name, "Clone", "the entrant ends up as a copy of the Clone link - the chain stopped there");
    assert_eq!(
        engine.state().object(entrant).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        1,
        "no second copy effect was actually applied, so the first round's exception fires normally"
    );
    assert_eq!(engine.state().object(entrant).unwrap().copy_generation, 1, "only one real copy application occurred");
}

#[test]
fn choosing_a_non_chaining_target_directly_applies_the_exception_with_no_extra_pause() {
    let (mut engine, p0, ..) = setup_chain_scenario();
    let StepStatus::AwaitingCopyChoice { object: entrant, ref candidates, .. } = drain(&mut engine) else {
        panic!("expected the first copy choice pause")
    };
    let bear = candidates.iter().copied().find(|&id| id.0 == 2).unwrap();

    engine.submit_copy_choice(Some(bear)).expect("the Bear was an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no chain - resolution finishes in one round, got {status:?}");

    assert_eq!(
        engine.state().object(entrant).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        1,
        "the Bear has no EntersAsCopy of its own, so no chain - the exception applies immediately"
    );
    assert_eq!(engine.state().object(entrant).unwrap().copy_generation, 1);
}
