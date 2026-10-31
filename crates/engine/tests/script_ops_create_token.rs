//! `AbilityEffect::CreateToken` (`Game.Tokens.*` and `Game.Ops.CreateToken`) places tokens directly onto the battlefield with `is_token: true`. Named tokens (Clue, Food) register their activated ability through `register_token_abilities`, and Creature tokens, artifact creatures included, have none.
// TODO: CR 111.7 cessation, since a token goes to the graveyard like any other permanent

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn instant_definition(id: u64, name: &str, effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(effect),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
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
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
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
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
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
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("player holds priority");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected payment pause, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("free cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn clue_token_lands_on_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(bf.contents.len(), 1, "one token on battlefield");
    let token_id = bf.contents[0];
    let token = engine.state().object(token_id).unwrap();
    assert!(token.is_token, "token has is_token=true");
    assert_eq!(token.definition.name, "Clue");
    assert!(token.definition.card_types.contains(CardTypeSet::ARTIFACT));
    assert_eq!(token.controller, p0);
}

#[test]
fn clue_token_has_registered_ability() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents[0];
    let abilities: Vec<_> = engine.state().abilities.iter().filter(|a| a.source == token_id).collect();
    assert_eq!(abilities.len(), 1, "Clue has exactly one activated ability");
    assert!(matches!(abilities[0].effect, Some(AbilityEffect::DrawCards(1))));
}

#[test]
fn food_token_has_registered_ability() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Food Maker", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Food), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents[0];
    let abilities: Vec<_> = engine.state().abilities.iter().filter(|a| a.source == token_id).collect();
    assert_eq!(abilities.len(), 1, "Food has exactly one activated ability");
    assert!(matches!(abilities[0].effect, Some(AbilityEffect::GainLife(3))));
}

#[test]
fn treasure_token_has_registered_ability() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Treasure Maker", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Treasure), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents[0];
    assert_eq!(engine.state().object(token_id).unwrap().definition.name, "Treasure");
    let abilities: Vec<_> = engine.state().abilities.iter().filter(|a| a.source == token_id).collect();
    assert_eq!(abilities.len(), 1, "Treasure has exactly one activated ability");
    assert!(matches!(
        abilities[0].effect,
        Some(AbilityEffect::AddManaChoice { count: 1, choices }) if choices == ColorSet::all()
    ));
}

// CR 602.2b
// The Treasure ability adds the chosen color of mana when activated and paid for.
#[test]
fn treasure_ability_adds_chosen_color_mana_when_activated() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Treasure Maker", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Treasure), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let token_id = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents[0];

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: token_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: vec![Color::Blue],
                x_value: None,
            },
        })
        .expect("Treasure ability is available");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "Treasure costs {{T}} + sacrifice");

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Tap(token_id), CostPayment::Sacrifice(token_id)],
        })
        .expect("payment accepted");
    // CR 605.3b: a mana ability resolves as part of payment and never uses the stack, checked before another pass would empty the pool (CR 500.4).
    drain(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 1, "added one blue mana");
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "Treasure was sacrificed");
}

#[test]
fn create_three_clue_tokens() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Investigate x3", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 3 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(bf.contents.len(), 3, "three Clue tokens");
    assert_eq!(
        engine.state().abilities.iter().filter(|a| matches!(a.effect, Some(AbilityEffect::DrawCards(1)))).count(),
        3,
        "three DrawCards(1) abilities registered"
    );
}

#[test]
fn creature_token_has_correct_stats() {
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Soldier".to_string()],
        colors: ColorSet::empty(),
        power: 1,
        toughness: 1,
        keywords: KeywordSet::empty(),
        name: None,
        additional_types: CardTypeSet::empty(),
        additional_supertypes: SupertypeSet::empty(),
    });
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, instant_definition(1, "Make Soldier", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token = engine.state().object(bf.contents[0]).unwrap();
    assert!(token.is_token);
    assert!(token.definition.card_types.contains(CardTypeSet::CREATURE));
    assert!(matches!(token.definition.power, Some(PTValue::Fixed(1))));
    assert!(matches!(token.definition.toughness, Some(PTValue::Fixed(1))));
    assert_eq!(token.definition.subtypes, vec![Subtype("Soldier".to_string())]);
    assert_eq!(token.definition.name, "Soldier");
}

#[test]
fn creature_token_multi_subtype_name() {
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Elf".to_string(), "Warrior".to_string()],
        colors: ColorSet::empty(),
        power: 1,
        toughness: 1,
        keywords: KeywordSet::empty(),
        name: None,
        additional_types: CardTypeSet::empty(),
        additional_supertypes: SupertypeSet::empty(),
    });
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, instant_definition(1, "Make Elf Warrior", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token = engine.state().object(bf.contents[0]).unwrap();
    assert_eq!(token.definition.name, "Elf Warrior");
}

#[test]
fn creature_token_name_override() {
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Zombie".to_string()],
        colors: ColorSet::empty(),
        power: 2,
        toughness: 2,
        keywords: KeywordSet::empty(),
        name: Some("Rooftop Zombie".to_string()),
        additional_types: CardTypeSet::empty(),
        additional_supertypes: SupertypeSet::empty(),
    });
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, instant_definition(1, "Reanimate", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token = engine.state().object(bf.contents[0]).unwrap();
    assert_eq!(token.definition.name, "Rooftop Zombie");
}

#[test]
fn artifact_creature_token_has_both_types() {
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Thopter".to_string()],
        colors: ColorSet::empty(),
        power: 1,
        toughness: 1,
        keywords: KeywordSet::FLYING,
        name: None,
        additional_types: CardTypeSet::ARTIFACT,
        additional_supertypes: SupertypeSet::empty(),
    });
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, instant_definition(1, "Thopter Factory", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token = engine.state().object(bf.contents[0]).unwrap();
    assert!(token.definition.card_types.contains(CardTypeSet::ARTIFACT));
    assert!(token.definition.card_types.contains(CardTypeSet::CREATURE));
    assert!(token.definition.keywords.contains(KeywordSet::FLYING));
}

// A token respects the controller of the spell's source.
#[test]
fn token_controller_matches_spell_caster() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // Give p1 priority by making it p1's turn
    state.active_player = p1;
    state.priority_player = p1;
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }),
            p1,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p1, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p1, p0);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token = engine.state().object(bf.contents[0]).unwrap();
    assert_eq!(token.controller, p1);
    assert_eq!(token.owner, p1);
}

#[test]
fn clue_ability_draws_a_card_when_activated() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    // Seed the library with one card
    let lib_card = ObjectId(5);
    state.objects.insert(lib_card, card_object(lib_card, land_definition(99, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_card);

    // Spell that creates a Clue
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    // Give p0 2 colorless mana for the Clue activation cost ({2})
    state.players[0].mana_pool = ManaPool { colorless: 2, ..ManaPool::default() };

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    // The Clue is on the battlefield; find it
    let token_id = {
        let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
        assert_eq!(bf.contents.len(), 1);
        bf.contents[0]
    };

    // Activate the Clue's ability (index 0)
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: token_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("Clue ability is available");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "Clue costs {{2}} + sacrifice");

    // Pay {2} mana + sacrifice the Clue itself
    let units = vec![ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units, phyrexian_life: 0 },
                CostPayment::Sacrifice(token_id),
            ],
        })
        .expect("payment accepted");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    // Clue is gone, hand has the drawn card
    assert!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(),
        "Clue was sacrificed"
    );
    assert_eq!(
        engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(),
        1,
        "drew one card from the Clue"
    );
}

#[test]
fn food_ability_gains_life_when_activated() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Make Food", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Food), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    // Give p0 2 colorless mana for the Food activation cost ({2})
    state.players[0].mana_pool = ManaPool { colorless: 2, ..ManaPool::default() };

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let token_id = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents[0];

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: token_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("Food ability is available");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    let units = vec![ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units, phyrexian_life: 0 },
                CostPayment::Tap(token_id),
                CostPayment::Sacrifice(token_id),
            ],
        })
        .expect("payment accepted");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "gained 3 life from Food");
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "Food was sacrificed");
}

// CR 111.10b, 111.10f
// A "Sacrifice this artifact" cost cannot be paid by sacrificing a different matching artifact, since `SacrificeSelf` pins it to the source.
#[test]
fn clue_ability_cannot_be_paid_by_sacrificing_a_different_artifact() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(1, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.players[0].mana_pool = ManaPool { colorless: 2, ..ManaPool::default() };

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let clue_id = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents[0];

    // A second artifact the payer also controls - a decoy for the sacrifice.
    let decoy_id = ObjectId(9001);
    engine.state_mut().objects.insert(
        decoy_id,
        card_object(
            decoy_id,
            CardDefinition { definition_id: 2, card_types: CardTypeSet::ARTIFACT, ..instant_definition(2, "Decoy", AbilityEffect::GainLife(0)) },
            p0,
            ZoneKind::Battlefield,
        ),
    );
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(decoy_id);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: clue_id,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("Clue ability is available");
    drain(&mut engine);

    let units = vec![ManaUnit::Colorless, ManaUnit::Colorless];
    let result = engine.submit_payment(PaymentPlan {
        payments: vec![
            CostPayment::Mana { units, phyrexian_life: 0 },
            CostPayment::Sacrifice(decoy_id),
        ],
    });
    assert!(result.is_err(), "sacrificing the decoy instead of the Clue itself must be rejected");
}

#[test]
fn creature_token_has_no_registered_abilities() {
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Beast".to_string()],
        colors: ColorSet::empty(),
        power: 3,
        toughness: 3,
        keywords: KeywordSet::empty(),
        name: None,
        additional_types: CardTypeSet::empty(),
        additional_supertypes: SupertypeSet::empty(),
    });
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, instant_definition(1, "Beast Within", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().abilities.is_empty(), "creature tokens register no abilities");
}

// TODO: tokens still go to the graveyard until CR 111.7 cessation lands
#[test]
fn dying_token_goes_to_graveyard_todo_cr111_7() {
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Saproling".to_string()],
        colors: ColorSet::empty(),
        power: 1,
        toughness: 1,
        keywords: KeywordSet::empty(),
        name: None,
        additional_types: CardTypeSet::empty(),
        additional_supertypes: SupertypeSet::empty(),
    });
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    // Place a prebuilt creature that will fight the token
    let attacker = ObjectId(0);
    state.objects.insert(attacker, {
        let mut obj = card_object(attacker, creature_definition(10, "Crusher"), p1, ZoneKind::Battlefield);
        obj.definition.power = Some(PTValue::Fixed(2));
        obj.definition.toughness = Some(PTValue::Fixed(2));
        obj
    });
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(attacker);

    // Spell to make the Saproling
    let card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(card, instant_definition(2, "Saproling Migration", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    // Token is on the battlefield; deal 2 damage to kill it
    let token_id = engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents
        .iter().find(|&&id| id != attacker).copied().unwrap();
    engine.state_mut().object_mut(token_id).unwrap().damage_marked = 2;

    // Pass priority both ways to trigger SBA processing
    pass_both_and_drain(&mut engine, p0, p1);

    // TODO: the token goes to the graveyard and not away until CR 111.7 cessation lands. Death creates a new lineage object, so check by zone length and not id.
    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(bf.contents.len(), 1, "only the attacker remains on battlefield");
    assert_eq!(bf.contents[0], attacker, "the surviving creature is the attacker");
    // The graveyard holds the resolved spell + the dead token
    let gy = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert!(!gy.contents.is_empty(), "token went to graveyard (CR 111.7 cessation not yet implemented)");
}
