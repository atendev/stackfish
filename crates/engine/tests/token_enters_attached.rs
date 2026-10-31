//! CR 303.4f/g, 301.5e, 301.6: pre-attached entry for token creation (`AbilityEffect::CreateToken`). Only `TokenSpec::CopyOf` can produce an Aura, Equipment or Fortification token (Estrid, the Masked; Bruna, the Fading Light).
//! The attach-candidate dispatch always runs, gated by `is_aura`, `is_equipment` and `is_fortification` on the token's resolved `CardDefinition`. Per CR 303.4g an Aura token with no legal target is not created, unlike Search, which leaves the library card in place.
//! `continue_create_token` takes a `remaining: u32`, so a multi-candidate pause on one token resumes the rest.

use stackfish_engine::engine::{AttachChoiceError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

fn instant_definition(id: u64, name: &str, effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn aura_source_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Test Aura: enchant creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
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
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn aura_source_enchant_player_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Player { controller: None },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        ..aura_source_definition(id, name)
    }
}

fn equipment_source_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Equipment".to_string())],
        oracle_text: "Test Equipment: equip creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
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
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

// The CopyOf source sits in the graveyard, where it is exempt from the CR 704.5m unattached-Aura SBA, because a battlefield Aura with zero legal targets would already have died.
fn put_in_graveyard(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state
        .zone_mut(ZoneKind::Graveyard, Some(owner))
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
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

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

fn battlefield_objects_excluding(state: &GameState, exclude: Vec<ObjectId>) -> Vec<&GameObject> {
    state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && !exclude.contains(&o.object_id))
        .collect()
}

#[test]
fn token_copy_of_aura_with_one_legal_candidate_attaches_immediately_no_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let bear = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            aura_source_definition(1, "Source Aura"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(2, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                3,
                "Clone Legion",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no attach-choice pause with only one legal candidate, got {status:?}"
    );

    let new_tokens = battlefield_objects_excluding(engine.state(), [source, bear].to_vec());
    assert_eq!(
        new_tokens.len(),
        1,
        "exactly one Aura token should have been created"
    );
    let token = new_tokens[0];
    assert_eq!(token.attached_to, Some(bear));
    assert!(token.is_token);
    assert_eq!(
        engine.state().object(bear).unwrap().attachments,
        vec![token.object_id]
    );
}

#[test]
fn token_copy_of_aura_with_two_legal_candidates_pauses_and_resumes() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let bear_a = ObjectId(1);
    let bear_b = ObjectId(2);
    let card = ObjectId(3);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            aura_source_definition(1, "Source Aura"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_a,
            creature_definition(2, "Bear A"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_b,
            creature_definition(3, "Bear B"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                4,
                "Clone Legion",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingTokenAttachChoice {
        player,
        attaching,
        candidates,
    } = status
    else {
        panic!("expected a CR 303.4f attach-choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    let mut expected = vec![Target::Object(bear_a), Target::Object(bear_b)];
    expected.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    assert_eq!(sorted, expected);
    assert_eq!(
        engine.state().object(attaching).unwrap().zone,
        ZoneKind::Battlefield
    );
    assert_eq!(engine.state().object(attaching).unwrap().attached_to, None);

    engine
        .submit_token_attach_choice(Target::Object(bear_b))
        .expect("bear_b is an offered candidate");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "resolution should finish, got {status:?}"
    );

    assert_eq!(
        engine.state().object(attaching).unwrap().attached_to,
        Some(bear_b)
    );
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Attached { object, attached_to } if object == attaching && attached_to == bear_b)),
        "an Attached event should be logged"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })),
        "the spell's own resolution should still have finished"
    );
}

#[test]
fn token_attach_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let bear_a = ObjectId(1);
    let bear_b = ObjectId(2);
    let card = ObjectId(3);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            aura_source_definition(1, "Source Aura"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_a,
            creature_definition(2, "Bear A"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_b,
            creature_definition(3, "Bear B"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                4,
                "Clone Legion",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine
        .submit_token_attach_choice(Target::Object(ObjectId(999)))
        .unwrap_err();
    assert!(matches!(err, AttachChoiceError::InvalidCandidate));
}

#[test]
fn token_copy_of_aura_with_zero_legal_candidates_is_not_created() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            aura_source_definition(1, "Source Aura"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    // No creatures anywhere - zero legal enchant targets.
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                2,
                "Clone Legion",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no pause; resolution just completes with nothing created, got {status:?}"
    );

    let new_tokens = battlefield_objects_excluding(engine.state(), [source].to_vec());
    assert!(
        new_tokens.is_empty(),
        "CR 303.4g: an Aura token with no legal target isn't created at all, got {new_tokens:?}"
    );
}

#[test]
fn token_copy_of_equipment_with_zero_legal_candidates_still_created_unattached() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            equipment_source_definition(1, "Source Equipment"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    // No creatures anywhere - zero legal equip targets.
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                2,
                "Equipment Clone",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let new_tokens = battlefield_objects_excluding(engine.state(), [source].to_vec());
    assert_eq!(
        new_tokens.len(),
        1,
        "CR 301.5e: the Equipment token is still created even with zero legal candidates"
    );
    assert_eq!(
        new_tokens[0].attached_to, None,
        "just unattached, not attached to anything"
    );
}

#[test]
fn count_2_token_copies_of_aura_each_independently_pause_and_attach() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let bear_a = ObjectId(1);
    let bear_b = ObjectId(2);
    let card = ObjectId(3);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            aura_source_definition(1, "Source Aura"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_a,
            creature_definition(2, "Bear A"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_b,
            creature_definition(3, "Bear B"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                4,
                "Double Clone Legion",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 2,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingTokenAttachChoice {
        attaching: first_token,
        ..
    } = status
    else {
        panic!("expected the first token's own attach-choice pause, got {status:?}")
    };
    engine
        .submit_token_attach_choice(Target::Object(bear_a))
        .expect("bear_a is an offered candidate");

    let status = drain(&mut engine);
    let StepStatus::AwaitingTokenAttachChoice {
        attaching: second_token,
        ..
    } = status
    else {
        panic!("expected the second token's own independent attach-choice pause, got {status:?}")
    };
    assert_ne!(
        first_token, second_token,
        "each token in the count loop gets its own real id"
    );
    engine
        .submit_token_attach_choice(Target::Object(bear_b))
        .expect("bear_b is an offered candidate");

    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "both tokens resolved, resolution should finish, got {status:?}"
    );

    assert_eq!(
        engine.state().object(first_token).unwrap().attached_to,
        Some(bear_a)
    );
    assert_eq!(
        engine.state().object(second_token).unwrap().attached_to,
        Some(bear_b)
    );
    let new_tokens =
        battlefield_objects_excluding(engine.state(), [source, bear_a, bear_b].to_vec());
    assert_eq!(
        new_tokens.len(),
        2,
        "both Aura tokens should have been created"
    );
}

#[test]
fn token_copy_of_player_aura_attaches_to_a_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_in_graveyard(
        &mut state,
        card_object(
            source,
            aura_source_enchant_player_definition(1, "Source Player Aura"),
            p0,
            ZoneKind::Graveyard,
        ),
    );
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                2,
                "Clone Legion",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(source, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    // A plain TargetFilter::Player has no controller restriction, so p0 and p1 are both legal, with no exactly-one case.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingTokenAttachChoice {
        attaching,
        candidates,
        ..
    } = status
    else {
        panic!("expected a player-target attach-choice pause, got {status:?}")
    };
    assert!(candidates.contains(&Target::Player(p0)) && candidates.contains(&Target::Player(p1)));

    engine
        .submit_token_attach_choice(Target::Player(p1))
        .expect("p1 is an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(
        engine.state().object(attaching).unwrap().attached_to_player,
        Some(p1)
    );
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::AttachedToPlayer { object, player } if object == attaching && player == p1)),
        "an AttachedToPlayer event should be logged"
    );
}
