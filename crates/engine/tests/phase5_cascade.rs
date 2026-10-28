//! CR 702.85 Cascade: a "when you cast this spell" trigger (CR 601.2i) runs a library walk that ends in a "cast it or not" pause.
//! `AwaitingCascadeChoice` is answered by `decline_cascade_cast` or `submit_cascade_cast`, and a cast synthesizes a `{0}` alternative cost through `pending_free_cast`.
//! The trigger is registered by direct `RegisteredAbility` construction.
// TODO: a script cannot self-register a `Cast` trigger, since re-running `register_abilities` at cast time double-registers ETB triggers

use std::collections::HashMap;

use stackfish_engine::engine::{ActionError, CascadeChoiceError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

fn cascade_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Cascade Beast".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(3)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Cascade".to_string(),
        power: Some(PTValue::Fixed(5)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_cascade_beast".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn filler_creature(definition_id: u64, name: &str, generic_mv: u32) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(generic_mv))] },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_filler".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn basic_land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..filler_creature(definition_id, name, 0)
    }
    .with_land()
}

trait AsLand {
    fn with_land(self) -> Self;
}
impl AsLand for CardDefinition {
    fn with_land(mut self) -> Self {
        self.card_types = CardTypeSet::LAND;
        self.power = None;
        self.toughness = None;
        self.mana_cost = ManaCost { symbols: Vec::new() };
        self
    }
}

// Registered on the predicted stack object id: next_object_id starts at 1000 and nothing allocates before the cast.
fn cascade_trigger(source: ObjectId, _controller: PlayerId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Cast)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Cascade),
        intervening_if: None,
    }
}

const PREDICTED_STACK_ID: ObjectId = ObjectId(1000);

fn two_player_state(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Exile, ZoneKind::Graveyard]))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn library_top_to_bottom(state: &mut GameState, p0: PlayerId, defs: Vec<(u64, CardDefinition)>) {
    for (id, def) in defs {
        let oid = ObjectId(id);
        state.objects.insert(oid, card_object(oid, def, p0, ZoneKind::Library));
        state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(oid);
    }
}

// Casts the spell and pays through the acknowledgment pause, returning with the Cast trigger on the stack above it.
fn cast_cascade_spell_and_await_trigger(engine: &mut Engine, p0: PlayerId, card: ObjectId) {
    cast_spell(engine, p0, card);
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless, ManaUnit::Colorless, ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .expect("{3}{R} paid with 1 red + 3 generic");
    let status = drain(engine);
    assert_eq!(engine.state().stack.len(), 2, "the cascade spell plus the Cast trigger it raised");
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
}

#[test]
fn cascade_trigger_fires_off_casting_and_walks_past_lands_to_a_qualifying_candidate() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, cascade_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    library_top_to_bottom(
        &mut state,
        p0,
        vec![
            (10, basic_land_definition(10, "Forest")),
            (11, basic_land_definition(11, "Island")),
            (12, filler_creature(12, "TooExpensive", 5)), // mv 5, not < 4
            (13, filler_creature(13, "Candidate", 2)),     // mv 2, < 4 - qualifies
            (14, filler_creature(14, "NeverReached", 1)),
        ],
    );
    state.abilities.push(cascade_trigger(PREDICTED_STACK_ID, p0));

    let mut engine = Engine::new(state);
    cast_cascade_spell_and_await_trigger(&mut engine, p0, card);

    let status = pass_both_and_drain(&mut engine, p0, p1);
    let (player, candidate) = match status {
        StepStatus::AwaitingCascadeChoice { player, candidate } => (player, candidate),
        other => panic!("expected AwaitingCascadeChoice, got {other:?}"),
    };
    assert_eq!(player, p0);
    let candidate = candidate.expect("a qualifying candidate (mv 2 < 4) should have been found");
    assert_eq!(engine.state().object(candidate).unwrap().definition.name, "Candidate");

    // Exiled: the two lands, the too-expensive creature and the candidate. The walk stopped there.
    let exile_names: Vec<String> = engine
        .state()
        .zone(ZoneKind::Exile, None)
        .unwrap()
        .contents
        .iter()
        .map(|&id| engine.state().object(id).unwrap().definition.name.clone())
        .collect();
    assert_eq!(exile_names.len(), 4, "got {exile_names:?}");
    assert!(
        engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.iter().any(|&id| {
            engine.state().object(id).unwrap().definition.name == "NeverReached"
        }),
        "the walk must stop at the first qualifying candidate, not exile the whole library"
    );
}

#[test]
fn declining_bottoms_every_exiled_card_in_a_random_order_and_casts_nothing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, cascade_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    library_top_to_bottom(
        &mut state,
        p0,
        vec![(10, basic_land_definition(10, "Forest")), (13, filler_creature(13, "Candidate", 2))],
    );
    state.abilities.push(cascade_trigger(PREDICTED_STACK_ID, p0));

    let mut engine = Engine::new(state);
    cast_cascade_spell_and_await_trigger(&mut engine, p0, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingCascadeChoice { candidate: Some(_), .. }), "got {status:?}");

    engine.decline_cascade_cast().expect("valid decline");
    drain(&mut engine);

    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty(), "must have left exile");
    assert_eq!(
        engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.len(),
        2,
        "both exiled cards returned to the library"
    );
    // The Cast trigger resolved, so the cascade spell underneath still needs its own pass-both cycle.
    assert_eq!(engine.state().stack.len(), 1, "just the cascade spell itself remains");
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(engine.state().stack.is_empty(), "nothing was cast via cascade - only the cascade spell itself resolved");
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
}

#[test]
fn casting_the_candidate_pays_no_mana_cost_and_bottoms_only_the_leftovers() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    // No mana is left after the cascade spell's {3}{R}, so the candidate was cast for {0}.
    let mut state = two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, cascade_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    library_top_to_bottom(
        &mut state,
        p0,
        vec![(10, basic_land_definition(10, "Forest")), (13, filler_creature(13, "Candidate", 2))],
    );
    state.abilities.push(cascade_trigger(PREDICTED_STACK_ID, p0));

    let mut engine = Engine::new(state);
    cast_cascade_spell_and_await_trigger(&mut engine, p0, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let candidate = match status {
        StepStatus::AwaitingCascadeChoice { candidate: Some(c), .. } => c,
        other => panic!("expected a candidate, got {other:?}"),
    };

    engine.submit_cascade_cast(Vec::new(), Vec::new(), None).expect("casting the candidate via cascade");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "CR 118.5a still needs an explicit {{0}} acknowledgment, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");

    assert!(engine.state().object(candidate).is_none(), "the exiled candidate left exile to become the spell on the stack");
    assert_eq!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.len(), 1, "only the land was bottomed");
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 0);

    // LIFO: the cast candidate resolves first onto the battlefield (CR 608.3), then the cascade spell.
    assert_eq!(engine.state().stack.len(), 2, "the candidate plus the cascade spell itself");
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    let candidate_on_battlefield = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .any(|&id| engine.state().object(id).unwrap().definition.name == "Candidate");
    assert!(candidate_on_battlefield, "the cascade-cast creature spell resolves onto the battlefield normally");

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert!(engine.state().stack.is_empty(), "the cascade spell itself has now resolved too");
}

#[test]
fn no_qualifying_candidate_only_allows_decline() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, cascade_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    // Both remaining library cards are lands - never a legal candidate.
    library_top_to_bottom(
        &mut state,
        p0,
        vec![(10, basic_land_definition(10, "Forest")), (11, basic_land_definition(11, "Island"))],
    );
    state.abilities.push(cascade_trigger(PREDICTED_STACK_ID, p0));

    let mut engine = Engine::new(state);
    cast_cascade_spell_and_await_trigger(&mut engine, p0, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingCascadeChoice { candidate: None, .. }), "got {status:?}");

    let result = engine.submit_cascade_cast(Vec::new(), Vec::new(), None);
    assert!(matches!(result, Err(CascadeChoiceError::NoCandidate)), "got {result:?}");

    engine.decline_cascade_cast().expect("decline is always legal");
    let status = drain(&mut engine);
    assert_eq!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.len(), 2, "both lands returned");
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
}

#[test]
fn ordinary_priority_actions_are_rejected_while_awaiting_the_cascade_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, cascade_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    library_top_to_bottom(&mut state, p0, vec![(13, filler_creature(13, "Candidate", 2))]);
    state.abilities.push(cascade_trigger(PREDICTED_STACK_ID, p0));

    let mut engine = Engine::new(state);
    cast_cascade_spell_and_await_trigger(&mut engine, p0, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingCascadeChoice { .. }), "got {status:?}");

    let result = engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority });
    assert!(matches!(result, Err(ActionError::IllegalAction(_))), "expected rejection while mid-pipeline, got {result:?}");
}
