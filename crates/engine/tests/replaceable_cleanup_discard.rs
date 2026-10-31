//! CR 514.1: the cleanup hand-size discard is a replacement-checked zone-change proposal (Leyline of the Void redirects it to exile), closing the TODO in `Engine::submit_discard`'s doc comment. `continue_discard_moves` reuses the `continue_cost_zone_moves` loop with `ReplacementResumeTarget::CleanupDiscard`.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn vanilla_card(id: ObjectId, owner: PlayerId) -> GameObject {
    let definition = CardDefinition {
        definition_id: id.0,
        name: "Test Card".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
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
    };
    GameObject {
        controlled_since_turn: 1,
        ..card_object(id, definition, owner, ZoneKind::Hand)
    }
}

fn ward_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 999,
        name: "Ward".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
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

fn state_at_end_step() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(999);
    let mut state = GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Library,
                ZoneKind::Hand,
                ZoneKind::Graveyard,
                ZoneKind::Battlefield,
                ZoneKind::Exile,
            ],
        ))
    };
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(ward);
    state.objects.insert(
        ward,
        GameObject {
            controlled_since_turn: 1,
            ..card_object(ward, ward_definition(""), p0, ZoneKind::Battlefield)
        },
    );
    state
}

fn add_cards_to_hand(state: &mut GameState, player: PlayerId, count: usize) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    for i in 0..count {
        let id = ObjectId(100 + i as u64);
        state.objects.insert(id, vanilla_card(id, player));
        state
            .zone_mut(ZoneKind::Hand, Some(player))
            .unwrap()
            .push_bottom(id);
        ids.push(id);
    }
    ids
}

fn step_until_pause(engine: &mut Engine) -> StepStatus {
    loop {
        let r = engine.step();
        if !matches!(r.status, StepStatus::Progressed) {
            return r.status;
        }
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .expect("PassPriority should be accepted");
}

fn pass_to_cleanup_discard(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    let s = step_until_pause(engine);
    assert!(
        matches!(s, StepStatus::AwaitingAction { player } if player == p0),
        "expected p0 end-step priority, got {s:?}"
    );
    pass(engine, p0);
    let s = step_until_pause(engine);
    assert!(
        matches!(s, StepStatus::AwaitingAction { player } if player == p1),
        "expected p1 end-step priority, got {s:?}"
    );
    pass(engine, p1);
    step_until_pause(engine)
}

const DISCARDED_CARD_GOES_TO_EXILE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Hand", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected(Game.Filter({ card_types = { "Creature" } }))
    e:Register()
end
return s
"#;

#[test]
fn a_cleanup_discard_is_replacement_checked_for_real() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let hand_ids = add_cards_to_hand(&mut state, p0, 8);

    let ward = ObjectId(999);
    let ward_def = ward_definition(DISCARDED_CARD_GOES_TO_EXILE);
    state.object_mut(ward).unwrap().definition = ward_def.clone();
    ScriptRuntime::new()
        .run_card_script(&mut state, &ward_def, ward)
        .expect("ward script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let mut engine = Engine::new(state);
    let status = pass_to_cleanup_discard(&mut engine, p0, p1);
    let StepStatus::AwaitingDiscard {
        player,
        hand_size: 8,
        max_hand_size: 7,
    } = status
    else {
        panic!("expected an AwaitingDiscard pause for 1 excess card, got {status:?}");
    };
    assert_eq!(player, p0);

    // Discards any one of the 8, which are all the same vanilla Creature.
    engine
        .submit_discard(vec![hand_ids[0]])
        .expect("valid discard");
    let status = step_until_pause(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "expected the next turn's upkeep priority, got {status:?}"
    );
    assert!(
        engine.state().object(hand_ids[0]).is_none(),
        "the original hand card id is retired"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .is_empty(),
        "the ward's WouldMoveZone(Hand, Graveyard) + ReplaceZoneDestination(Exile) must have kept \
         the discarded card out of the graveyard entirely"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .len(),
        1,
        "the discarded card must have landed in exile instead"
    );
}

#[test]
fn a_cleanup_discard_without_a_matching_replacement_lands_in_the_graveyard_normally() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let hand_ids = add_cards_to_hand(&mut state, p0, 8);
    // No replacement effect is registered: the ward object exists but its script is empty.

    let mut engine = Engine::new(state);
    let status = pass_to_cleanup_discard(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingDiscard { .. }));

    engine
        .submit_discard(vec![hand_ids[0]])
        .expect("valid discard");
    step_until_pause(&mut engine);

    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .len(),
        1,
        "no replacement registered - the card lands in the graveyard as normal"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty()
    );
}

#[test]
fn two_matching_wards_pause_the_cleanup_discard_for_a_cr_616_1_choice() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let hand_ids = add_cards_to_hand(&mut state, p0, 8);

    let ward_a = ObjectId(999);
    let ward_a_def = ward_definition(DISCARDED_CARD_GOES_TO_EXILE);
    state.object_mut(ward_a).unwrap().definition = ward_a_def.clone();
    ScriptRuntime::new()
        .run_card_script(&mut state, &ward_a_def, ward_a)
        .expect("ward A self-registers cleanly");

    let ward_b = ObjectId(1001);
    state.objects.insert(
        ward_b,
        GameObject {
            object_id: ward_b,
            ..state.objects[&ward_a].clone()
        },
    );
    state.replacement_effects.push(ReplacementEffect {
        source: ward_b,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Hand),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter {
                    card_types: Some(CardTypeSet::CREATURE),
                    controller: None,
                    self_only: false,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    assert_eq!(state.replacement_effects.len(), 2);

    let mut engine = Engine::new(state);
    let status = pass_to_cleanup_discard(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingDiscard { .. }));

    engine
        .submit_discard(vec![hand_ids[0]])
        .expect("valid discard");
    let status = step_until_pause(&mut engine);
    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!(
            "two matching replacement effects on a cleanup discard must pause for a CR 616.1 choice, got {status:?}"
        );
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);
    assert!(
        engine.state().object(hand_ids[0]).is_some(),
        "nothing committed yet"
    );

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(ward_b))
        .expect("ward_b is a real candidate");
    let status = step_until_pause(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "discard should finish resolving after the choice, got {status:?}"
    );

    assert!(engine.state().object(hand_ids[0]).is_none());
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .len(),
        1,
        "both wards redirect to Exile - either choice ends up there"
    );
}
