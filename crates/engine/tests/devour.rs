//! CR 702.82a Devour N: the entering permanent gets N +1/+1 counters per creature sacrificed as it enters.
//! `ResolveStackTop` checks `replacement::pending_devour_choice` before `apply_enters_battlefield_modifiers`, pauses on `AwaitingDevourChoice` and resumes via `submit_devour_choice` through `continue_scripted_zone_move`.
//! Candidates are the controller's creatures (CR 701.17a), including the entrant, and `Devour [quality]` narrows them by subtype (CR 702.82c).

use stackfish_engine::engine::{DevourChoiceError, Engine, StepStatus};
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

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

fn subtyped_creature_definition(
    name: &str,
    mana_cost: ManaCost,
    script: &str,
    subtypes: Vec<Subtype>,
) -> CardDefinition {
    let mut definition = creature_definition(name, mana_cost, script);
    definition.subtypes = subtypes;
    definition
}

fn artifact_definition(name: &str, mana_cost: ManaCost, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
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

fn devour_effect(source: ObjectId, per_creature: i32) -> ReplacementEffect {
    devour_effect_with_quality(source, per_creature, None)
}

fn devour_effect_with_quality(
    source: ObjectId,
    per_creature: i32,
    quality: Option<Subtype>,
) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::EntersWithCountersDevour {
                per_creature,
                quality,
            },
        },
        consumption: ConsumptionPolicy::Repeatable,
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

#[test]
fn pending_devour_choice_only_offers_creatures_the_entrant_s_own_controller_controls() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let entrant = ObjectId(0);
    let own_bear = ObjectId(1);
    let opponents_bear = ObjectId(2);
    let own_land = ObjectId(3);
    state.objects.insert(
        entrant,
        card_object(
            entrant,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.replacement_effects.push(devour_effect(entrant, 2));
    put_on_battlefield(
        &mut state,
        card_object(
            own_bear,
            creature_definition(
                "Own Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            opponents_bear,
            creature_definition(
                "Foe Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            own_land,
            artifact_definition(
                "Not A Creature",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let runtime = ScriptRuntime::new();
    let (per_creature, mut candidates) =
        replacement::pending_devour_choice(&state, &runtime, entrant, ZoneKind::Stack, p0)
            .expect("a Devour effect is registered with real candidates");
    candidates.sort_by_key(|id| id.0);

    assert_eq!(per_creature, 2);
    // The entrant and own_bear qualify, and the opponent's bear and the artifact do not.
    assert_eq!(candidates, vec![entrant, own_bear]);
}

#[test]
fn pending_devour_choice_with_a_quality_only_offers_the_matching_subtype() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    let nightmare = ObjectId(1);
    let bear = ObjectId(2);
    state.objects.insert(
        entrant,
        card_object(
            entrant,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.replacement_effects.push(devour_effect_with_quality(
        entrant,
        2,
        Some(Subtype("Nightmare".to_string())),
    ));
    put_on_battlefield(
        &mut state,
        card_object(
            nightmare,
            subtyped_creature_definition(
                "Night Terror",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
                vec![Subtype("Nightmare".to_string())],
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(
                "Plain Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let runtime = ScriptRuntime::new();
    let (per_creature, candidates) =
        replacement::pending_devour_choice(&state, &runtime, entrant, ZoneKind::Stack, p0)
            .expect("a Devour effect is registered with a real candidate");

    assert_eq!(per_creature, 2);
    // The entrant is not a Nightmare either, since quality narrows every candidate.
    assert_eq!(candidates, vec![nightmare]);
}

#[test]
fn pending_devour_choice_is_none_when_the_controller_has_no_creatures_at_all() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    // The entering permanent is not a creature, and there are no creatures anywhere.
    state.objects.insert(
        entrant,
        card_object(
            entrant,
            artifact_definition(
                "Odd Devourer",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state.replacement_effects.push(devour_effect(entrant, 2));

    let runtime = ScriptRuntime::new();
    assert!(
        replacement::pending_devour_choice(&state, &runtime, entrant, ZoneKind::Stack, p0)
            .is_none()
    );
}

#[test]
fn pending_devour_choice_is_none_without_a_registered_devour_effect() {
    let state = two_player_state();
    let p0 = PlayerId(0);
    let runtime = ScriptRuntime::new();
    assert!(
        replacement::pending_devour_choice(&state, &runtime, ObjectId(0), ZoneKind::Stack, p0)
            .is_none()
    );
}

const DEVOUR_TWO: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersDevour(2))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

#[test]
fn set_payload_devour_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition(
        "Gluttonous Beast",
        ManaCost {
            symbols: Vec::new(),
        },
        DEVOUR_TWO,
    );
    state.objects.insert(
        source,
        card_object(
            source,
            definition.clone(),
            PlayerId(0),
            ZoneKind::Battlefield,
        ),
    );

    ScriptRuntime::new()
        .run_card_script(&mut state, &definition, source)
        .expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter:
                EventFilter::WouldMoveZone {
                    to: Some(ZoneKind::Battlefield),
                    ..
                },
            payload:
                ReplacementPayload::EntersWithCountersDevour {
                    per_creature: 2,
                    quality: None,
                },
        } => {}
        other => panic!(
            "expected a WouldMoveZone(_, Battlefield)/EntersWithCountersDevour(2) spec, got {other:?}"
        ),
    }
}

const DEVOUR_NIGHTMARE_TWO: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersDevour(2, "Nightmare"))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

#[test]
fn set_payload_devour_with_a_quality_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition(
        "Gluttonous Nightmare",
        ManaCost {
            symbols: Vec::new(),
        },
        DEVOUR_NIGHTMARE_TWO,
    );
    state.objects.insert(
        source,
        card_object(
            source,
            definition.clone(),
            PlayerId(0),
            ZoneKind::Battlefield,
        ),
    );

    ScriptRuntime::new()
        .run_card_script(&mut state, &definition, source)
        .expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter:
                EventFilter::WouldMoveZone {
                    to: Some(ZoneKind::Battlefield),
                    ..
                },
            payload:
                ReplacementPayload::EntersWithCountersDevour {
                    per_creature: 2,
                    quality: Some(quality),
                },
        } => {
            assert_eq!(quality, &Subtype("Nightmare".to_string()));
        }
        other => panic!(
            "expected a WouldMoveZone(_, Battlefield)/EntersWithCountersDevour(2, \"Nightmare\") spec, got {other:?}"
        ),
    }
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
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
        .expect("player holds priority, spell is in hand, main phase, empty stack");
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

#[test]
fn choosing_to_sacrifice_both_offered_creatures_grants_four_counters_and_removes_them() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear_a = ObjectId(1);
    let bear_b = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                DEVOUR_TWO,
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_on_battlefield(
        &mut state,
        card_object(
            bear_a,
            creature_definition(
                "Bear A",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear_b,
            creature_definition(
                "Bear B",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingDevourChoice {
        player,
        object,
        per_creature,
        candidates,
    } = status
    else {
        panic!("expected a CR 702.82a Devour choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(per_creature, 2);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|id| id.0);
    let mut expected = vec![object, bear_a, bear_b];
    expected.sort_by_key(|id| id.0);
    assert_eq!(
        sorted, expected,
        "the entrant itself plus both bears are all legal candidates"
    );

    engine
        .submit_devour_choice(vec![bear_a, bear_b])
        .expect("both bears were offered candidates");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "resolution should finish cleanly, got {status:?}"
    );

    assert_eq!(
        engine
            .state()
            .object(object)
            .unwrap()
            .counters
            .count(&CounterKind::PlusOnePlusOne),
        4,
        "2 per creature x 2 sacrificed"
    );
    assert!(
        engine.state().object(bear_a).is_none(),
        "bear_a was sacrificed, its old id is gone"
    );
    assert!(
        engine.state().object(bear_b).is_none(),
        "bear_b was sacrificed, its old id is gone"
    );
    let battlefield: Vec<ObjectId> = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .copied()
        .collect();
    assert_eq!(
        battlefield,
        vec![object],
        "only the entrant remains on the battlefield"
    );
}

#[test]
fn devour_with_a_quality_only_offers_the_matching_subtype_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let nightmare = ObjectId(1);
    let bear = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Gluttonous Nightmare",
                ManaCost {
                    symbols: Vec::new(),
                },
                DEVOUR_NIGHTMARE_TWO,
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_on_battlefield(
        &mut state,
        card_object(
            nightmare,
            subtyped_creature_definition(
                "Night Terror",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
                vec![Subtype("Nightmare".to_string())],
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(
                "Plain Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingDevourChoice {
        object,
        per_creature,
        candidates,
        ..
    } = status
    else {
        panic!("expected a CR 702.82c Devour [quality] choice pause, got {status:?}")
    };
    assert_eq!(per_creature, 2);
    // Only Night Terror qualifies, since neither the entrant nor the plain Bear is a Nightmare.
    assert_eq!(candidates, vec![nightmare]);

    engine
        .submit_devour_choice(vec![nightmare])
        .expect("Night Terror was the offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(
        engine
            .state()
            .object(object)
            .unwrap()
            .counters
            .count(&CounterKind::PlusOnePlusOne),
        2
    );
    assert!(
        engine.state().object(nightmare).is_none(),
        "Night Terror was sacrificed"
    );
    assert!(
        engine.state().object(bear).is_some(),
        "the plain Bear was never a legal candidate, so it's untouched"
    );
}

#[test]
fn choosing_to_sacrifice_nothing_grants_no_counters_but_still_resolves() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                DEVOUR_TWO,
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(
                "Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingDevourChoice { object, .. } = status else {
        panic!("expected a Devour choice pause, got {status:?}")
    };

    engine
        .submit_devour_choice(Vec::new())
        .expect("choosing to sacrifice nothing is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(
        engine
            .state()
            .object(object)
            .unwrap()
            .counters
            .count(&CounterKind::PlusOnePlusOne),
        0
    );
    assert!(
        engine.state().object(bear).is_some(),
        "the bear was never sacrificed"
    );
}

#[test]
fn a_devour_creature_with_no_other_creatures_can_still_devour_itself() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                DEVOUR_TWO,
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
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingDevourChoice {
        object, candidates, ..
    } = status
    else {
        panic!(
            "expected a Devour choice pause (the entrant itself is a legal candidate), got {status:?}"
        )
    };
    assert_eq!(
        candidates,
        vec![object],
        "only the entrant itself qualifies"
    );

    engine
        .submit_devour_choice(vec![object])
        .expect("devouring itself is legal, if pointless");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    // It sacrificed itself before it could receive counters, so it is gone and not on the battlefield with 2.
    assert!(engine.state().object(object).is_none());
    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .is_empty()
    );
}

#[test]
fn devour_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                DEVOUR_TWO,
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
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine
        .submit_devour_choice(vec![ObjectId(999)])
        .unwrap_err();
    assert!(matches!(err, DevourChoiceError::InvalidCandidate));
}

#[test]
fn devour_choice_rejects_a_duplicate_candidate() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Gluttonous Beast",
                ManaCost {
                    symbols: Vec::new(),
                },
                DEVOUR_TWO,
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(
                "Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_devour_choice(vec![bear, bear]).unwrap_err();
    assert!(matches!(err, DevourChoiceError::DuplicateCandidate));
}

#[test]
fn submit_devour_choice_while_not_awaiting_is_rejected() {
    let p0 = PlayerId(0);
    let mut engine = Engine::new(two_player_state());
    let err = engine.submit_devour_choice(Vec::new()).unwrap_err();
    assert!(matches!(err, DevourChoiceError::NotAwaitingChoice));
    let _ = p0;
}

#[test]
fn a_creature_without_devour_never_pauses() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            creature_definition(
                "Plain Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(
                "Other Bear",
                ManaCost {
                    symbols: Vec::new(),
                },
                "",
            ),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = pay_free_and_drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no Devour effect registered - no pause, got {status:?}"
    );
}
