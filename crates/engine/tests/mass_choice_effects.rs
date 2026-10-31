//! Generic Lua primitives for Balance-shaped effects. `Game.ChooseN(filter, n, opts)` asks a player to choose N objects mid-resolution through the `Game.SelectTargets` and `Game.MayChoose` coroutine mechanism.
//! `Game.Ops.SacrificeChosen` and `DiscardChosen` move a chosen list, `SacrificeRandom` and `DiscardRandom` take no choice (CR 701.9b, 701.21), and `Game.Ops.ForEachPlayer(fn)` runs `fn` per player in APNAP order (CR 101.4) and moves the cards only after every player has chosen.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, ScriptChoiceError, StepStatus};
use stackfish_engine::script::ScriptChoiceRequest;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, player_state, two_player_state as base_two_player_state, zones,
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
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(format!("test_creature_{definition_id}")),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(format!("test_land_{definition_id}")),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn sorcery_definition(definition_id: u64, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Sorcery".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
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

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        players: vec![player_state(p0), player_state(p1)],
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Library,
            ],
        ))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state
        .zone_mut(ZoneKind::Hand, Some(owner))
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

// `s.operation()` must be loaded through this Engine's own ScriptRuntime and patched onto CardDefinition.effect, because a ScriptHandle from another runtime is meaningless.
fn register_script_operation(engine: &mut Engine, object: ObjectId, source: &str) {
    let Some((primitive, handle)) = engine
        .build_operation(source)
        .unwrap_or_else(|e| panic!("script operation() failed to load: {e:?}"))
    else {
        return;
    };
    let effect = primitive.or(Some(AbilityEffect::Script(handle)));
    engine
        .state_mut()
        .objects
        .get_mut(&object)
        .expect("object exists")
        .definition
        .effect = effect;
}

// Casts a 0-cost sorcery, pays the empty cost, passes priority, and returns what ResolveStackTop reaches next.
fn cast_and_resolve(engine: &mut Engine, p0: PlayerId, p1: PlayerId, card: ObjectId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
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
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the {{0}} cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine);
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

fn expect_select_target(status: StepStatus) -> (PlayerId, TargetFilter, Vec<ObjectId>) {
    match status {
        StepStatus::AwaitingScriptChoice {
            player,
            request:
                ScriptChoiceRequest::SelectTarget {
                    filter,
                    exclude_ids,
                },
        } => (player, filter, exclude_ids),
        other => panic!("expected AwaitingScriptChoice/SelectTarget, got {other:?}"),
    }
}

const CHOOSE_TWO_CREATURES_THEN_GAIN_LIFE: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.ChooseN({ card_types = { "Creature" } }, 2)
        return Game.Ops.GainLife(#chosen)
    end
end
return s
"#;

#[test]
fn choose_n_asks_twice_excluding_the_first_pick_and_rejects_a_repeat() {
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
            sorcery_definition(1, CHOOSE_TWO_CREATURES_THEN_GAIN_LIFE),
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

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, CHOOSE_TWO_CREATURES_THEN_GAIN_LIFE);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    let (player, _filter, exclude_ids) = expect_select_target(status);
    assert_eq!(player, p0);
    assert!(exclude_ids.is_empty(), "first ask excludes nothing yet");

    // The second ask rejects the object the first ask offered once exclude_ids covers it. The first ask's exclude_ids is empty.
    engine
        .submit_script_choice(Target::Object(bear_a))
        .expect("bear_a is a legal, unexcluded first pick");
    let status = drain(&mut engine);
    let (player, _filter, exclude_ids) = expect_select_target(status);
    assert_eq!(player, p0);
    assert_eq!(
        exclude_ids,
        vec![bear_a],
        "second ask must exclude the first pick"
    );

    let err = engine
        .submit_script_choice(Target::Object(bear_a))
        .expect_err("bear_a was already chosen this batch");
    assert!(matches!(err, ScriptChoiceError::InvalidCandidate));

    engine
        .submit_script_choice(Target::Object(bear_b))
        .expect("bear_b is still unchosen");
    let status = drain(&mut engine);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        22,
        "GainLife(#chosen) = 2, from a 20 starting life"
    );
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

const CHOOSE_UP_TO_TWO_THEN_GAIN_LIFE: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.ChooseN({ card_types = { "Creature" } }, 2, { up_to = true })
        return Game.Ops.GainLife(#chosen)
    end
end
return s
"#;

#[test]
fn choose_n_up_to_can_stop_before_reaching_n() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear_a = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, CHOOSE_UP_TO_TWO_THEN_GAIN_LIFE),
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
            creature_definition(2, "Bear A"),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, CHOOSE_UP_TO_TWO_THEN_GAIN_LIFE);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    // up_to asks "one more?" (Game.MayChoose-shaped) before every pick,
    // including the first.
    assert!(matches!(
        status,
        StepStatus::AwaitingScriptChoice {
            request: ScriptChoiceRequest::MayChoose,
            ..
        }
    ));
    engine
        .submit_script_may_choice(true)
        .expect("may answer yes to the first 'one more?' ask");
    let status = drain(&mut engine);
    let (_player, _filter, _exclude_ids) = expect_select_target(status);
    engine
        .submit_script_choice(Target::Object(bear_a))
        .expect("bear_a is legal");
    let status = drain(&mut engine);
    assert!(matches!(
        status,
        StepStatus::AwaitingScriptChoice {
            request: ScriptChoiceRequest::MayChoose,
            ..
        }
    ));
    engine
        .submit_script_may_choice(false)
        .expect("decline the second pick");
    let status = drain(&mut engine);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        21,
        "GainLife(#chosen) = 1, stopped early"
    );
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

const CHOOSE_AT_LEAST_ONE_THEN_GAIN_LIFE: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.ChooseN({ card_types = { "Creature" } }, 1, { at_least = true })
        return Game.Ops.GainLife(#chosen)
    end
end
return s
"#;

// opts.at_least (CR 601.2c, 115.6): the first n picks are mandatory with no "one more?" ask, then the loop keeps asking until declined.
#[test]
fn choose_n_at_least_is_mandatory_then_unbounded() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear_a = ObjectId(1);
    let bear_b = ObjectId(2);
    let bear_c = ObjectId(3);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, CHOOSE_AT_LEAST_ONE_THEN_GAIN_LIFE),
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
    put_on_battlefield(
        &mut state,
        card_object(
            bear_c,
            creature_definition(4, "Bear C"),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, CHOOSE_AT_LEAST_ONE_THEN_GAIN_LIFE);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    // The mandatory first pick - no "one more?" ask before it, unlike up_to.
    let (_player, _filter, _exclude_ids) = expect_select_target(status);
    engine
        .submit_script_choice(Target::Object(bear_a))
        .expect("bear_a is legal");
    let status = drain(&mut engine);
    // Past the minimum now - every further pick is asked first.
    assert!(matches!(
        status,
        StepStatus::AwaitingScriptChoice {
            request: ScriptChoiceRequest::MayChoose,
            ..
        }
    ));
    engine
        .submit_script_may_choice(true)
        .expect("choose one more");
    let status = drain(&mut engine);
    let (_player, _filter, exclude_ids) = expect_select_target(status);
    assert_eq!(exclude_ids, vec![bear_a]);
    engine
        .submit_script_choice(Target::Object(bear_b))
        .expect("bear_b is legal");
    let status = drain(&mut engine);
    assert!(matches!(
        status,
        StepStatus::AwaitingScriptChoice {
            request: ScriptChoiceRequest::MayChoose,
            ..
        }
    ));
    engine
        .submit_script_may_choice(false)
        .expect("stop here, bear_c never touched");
    let status = drain(&mut engine);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        22,
        "GainLife(#chosen) = 2"
    );
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().object(bear_c).unwrap().zone,
        ZoneKind::Battlefield,
        "never asked, never touched"
    );
}

fn creature_definition_with_power(definition_id: u64, name: &str, power: i32) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(power)),
        ..creature_definition(definition_id, name)
    }
}

const CHOOSE_ONE_BIG_CREATURE_VIA_PREDICATE: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.ChooseN(function(object, src, ctrl, state)
            for _, card in ipairs(state.battlefield) do
                if card.id == object then
                    return card.power ~= nil and card.power >= 4
                end
            end
            return false
        end, 1)
        return Game.Ops.GainLife(chosen[1].id)
    end
end
return s
"#;

// ChooseN's filter also accepts a scripted predicate, which can express a power test the flat table cannot.
#[test]
fn choose_n_accepts_a_scripted_predicate_filter() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let small_bear = ObjectId(1);
    let big_bear = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, CHOOSE_ONE_BIG_CREATURE_VIA_PREDICATE),
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
            small_bear,
            creature_definition_with_power(2, "Small Bear", 2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            big_bear,
            creature_definition_with_power(3, "Big Bear", 4),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, CHOOSE_ONE_BIG_CREATURE_VIA_PREDICATE);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    let (player, _filter, _exclude_ids) = expect_select_target(status);
    assert_eq!(player, p0);

    // small_bear (power 2) fails the predicate - only big_bear is legal.
    let err = engine
        .submit_script_choice(Target::Object(small_bear))
        .expect_err("power 2 < 4");
    assert!(matches!(err, ScriptChoiceError::InvalidCandidate));
    engine
        .submit_script_choice(Target::Object(big_bear))
        .expect("power 4 >= 4");
    drain(&mut engine);
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        20 + big_bear.0 as i32,
        "GainLife(the chosen object's own id)"
    );
}

const CHOOSE_ONE_CREATURE_THEN_SACRIFICE_IT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.ChooseN({ card_types = { "Creature" } }, 1)
        return Game.Ops.SacrificeChosen(chosen)
    end
end
return s
"#;

#[test]
fn sacrifice_chosen_moves_the_chosen_object_to_the_graveyard() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, CHOOSE_ONE_CREATURE_THEN_SACRIFICE_IT),
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
            creature_definition(2, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, CHOOSE_ONE_CREATURE_THEN_SACRIFICE_IT);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    let (player, _filter, _exclude_ids) = expect_select_target(status);
    assert_eq!(player, p0);
    engine
        .submit_script_choice(Target::Object(bear))
        .expect("bear is a legal creature");
    drain(&mut engine);

    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .is_empty()
    );
    let bear_object = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Bear")
        .expect("bear still tracked");
    assert_eq!(bear_object.zone, ZoneKind::Graveyard);
}

const CHOOSE_ONE_HAND_CARD_THEN_DISCARD_IT: &str = r#"
local s = {}
function s.operation()
    return function(source, controller)
        local chosen = Game.ChooseN({ zone = "Hand", controller = "you" }, 1)
        return Game.Ops.DiscardChosen(chosen)
    end
end
return s
"#;

#[test]
fn discard_chosen_moves_the_chosen_card_from_hand_to_graveyard() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let extra_card = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, CHOOSE_ONE_HAND_CARD_THEN_DISCARD_IT),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(
            extra_card,
            creature_definition(2, "Spare Card"),
            p0,
            ZoneKind::Hand,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, CHOOSE_ONE_HAND_CARD_THEN_DISCARD_IT);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    let (player, filter, _exclude_ids) = expect_select_target(status);
    assert_eq!(player, p0);
    let TargetFilter::Object {
        mana_value: None,
        zone,
        ..
    } = filter
    else {
        panic!("expected an Object filter")
    };
    assert_eq!(
        zone,
        Some(ZoneKind::Hand),
        "Game.ChooseN's zone override must reach the real request"
    );
    engine
        .submit_script_choice(Target::Object(extra_card))
        .expect("extra_card is in hand");
    drain(&mut engine);

    let moved = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Spare Card")
        .expect("still tracked");
    assert_eq!(moved.zone, ZoneKind::Graveyard);
}

const SACRIFICE_TWO_CREATURES_AT_RANDOM: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.SacrificeRandom({ card_types = { "Creature" } }, 2)
end
return s
"#;

#[test]
fn sacrifice_random_moves_exactly_count_when_enough_candidates_exist() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, SACRIFICE_TWO_CREATURES_AT_RANDOM),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    for (id, name) in [(1, "Bear A"), (2, "Bear B"), (3, "Bear C")] {
        put_on_battlefield(
            &mut state,
            card_object(
                ObjectId(id),
                creature_definition(id + 10, name),
                p0,
                ZoneKind::Battlefield,
            ),
        );
    }

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, SACRIFICE_TWO_CREATURES_AT_RANDOM);
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no player choice - resolves synchronously, got {status:?}"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .len(),
        1,
        "exactly 2 of 3 sacrificed at random"
    );
    // 2 sacrificed creatures plus the sorcery itself, which resolves into its owner's graveyard (CR 608.2m).
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .len(),
        3
    );
}

const DISCARD_THREE_AT_RANDOM: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DiscardRandom({}, 3)
end
return s
"#;

#[test]
fn discard_random_discards_fewer_than_count_when_hand_is_smaller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, DISCARD_THREE_AT_RANDOM),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(
            ObjectId(1),
            creature_definition(11, "Spare A"),
            p0,
            ZoneKind::Hand,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(
            ObjectId(2),
            creature_definition(12, "Spare B"),
            p0,
            ZoneKind::Hand,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, DISCARD_THREE_AT_RANDOM);
    cast_and_resolve(&mut engine, p0, p1, card);
    // Only 2 cards were in hand, so CR 701.17b "as many as possible" discards both though count asked for 3.
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .is_empty()
    );
    // 2 discarded cards plus the sorcery itself (CR 608.2m).
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .len(),
        3
    );
}

const DISCARD_A_RANDOM_LAND: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DiscardRandom({ card_types = { "Land" } }, 1)
end
return s
"#;

// DiscardRandom's filter must really restrict candidates and not be ignored.
#[test]
fn discard_random_respects_its_filter() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, DISCARD_A_RANDOM_LAND),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    put_in_hand(
        &mut state,
        card_object(
            ObjectId(1),
            land_definition(11, "Spare Land"),
            p0,
            ZoneKind::Hand,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(
            ObjectId(2),
            creature_definition(12, "Spare Creature"),
            p0,
            ZoneKind::Hand,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, DISCARD_A_RANDOM_LAND);
    cast_and_resolve(&mut engine, p0, p1, card);

    // Only the land was a legal candidate, so the creature stays in hand whatever next_rng draws.
    let hand_names: Vec<String> = engine
        .state()
        .zone(ZoneKind::Hand, Some(p0))
        .unwrap()
        .contents
        .iter()
        .map(|&id| engine.state().object(id).unwrap().definition.name.clone())
        .collect();
    assert_eq!(hand_names, vec!["Spare Creature".to_string()]);
    let discarded = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Spare Land")
        .expect("still tracked");
    assert_eq!(discarded.zone, ZoneKind::Graveyard);
}

const EACH_PLAYER_SACRIFICES_A_CREATURE_THEY_CONTROL: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ForEachPlayer(function(source, controller)
        local chosen = Game.ChooseN({ card_types = { "Creature" }, controller = "you" }, 1)
        return Game.Ops.SacrificeChosen(chosen)
    end)
end
return s
"#;

// CR 101.4
// "Each player sacrifices a creature": the active player is asked first, "you" resolves to the asked player, and both creatures move together only after p1 answers.
#[test]
fn for_each_player_asks_in_apnap_order_and_sacrifices_simultaneously() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let p0_bear = ObjectId(1);
    let p1_bear = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, EACH_PLAYER_SACRIFICES_A_CREATURE_THEY_CONTROL),
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
            p0_bear,
            creature_definition(2, "P0 Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            p1_bear,
            creature_definition(3, "P1 Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(
        &mut engine,
        card,
        EACH_PLAYER_SACRIFICES_A_CREATURE_THEY_CONTROL,
    );
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    let (player, filter, _exclude_ids) = expect_select_target(status);
    assert_eq!(player, p0, "active player is asked first (CR 101.4 APNAP)");
    let TargetFilter::Object {
        mana_value: None,
        controller,
        ..
    } = filter
    else {
        panic!("expected an Object filter")
    };
    assert_eq!(controller, Some(FilterController::You));

    engine
        .submit_script_choice(Target::Object(p0_bear))
        .expect("p0_bear is p0's own creature");
    let status = drain(&mut engine);

    // Simultaneity: p0's chosen creature has not moved while p1 is asked.
    assert_eq!(
        engine.state().object(p0_bear).unwrap().zone,
        ZoneKind::Battlefield,
        "not sacrificed until every player has chosen"
    );
    let (player, _filter, _exclude_ids) = expect_select_target(status);
    assert_eq!(player, p1, "second in turn order");

    // "You" resolves to p1, so p0's creature is illegal for p1's ask though the filter is identical.
    let err = engine
        .submit_script_choice(Target::Object(p0_bear))
        .expect_err("p0_bear isn't p1's own creature");
    assert!(matches!(err, ScriptChoiceError::InvalidCandidate));

    engine
        .submit_script_choice(Target::Object(p1_bear))
        .expect("p1_bear is p1's own creature");
    drain(&mut engine);

    // Both moved together after the last answer. CR 400.7 re-mints ids, so the post-move objects are found by name.
    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .is_empty()
    );
    let p0_bear_after = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "P0 Bear")
        .expect("still tracked");
    let p1_bear_after = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "P1 Bear")
        .expect("still tracked");
    assert_eq!(p0_bear_after.zone, ZoneKind::Graveyard);
    assert_eq!(p1_bear_after.zone, ZoneKind::Graveyard);
}

const EACH_PLAYER_SACRIFICES_A_CREATURE_AT_RANDOM_BARE_TAG: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ForEachPlayer(Game.Ops.SacrificeRandom({ card_types = { "Creature" }, controller = "you" }, 1))
end
return s
"#;

// ForEachPlayer can wrap a bare SacrificeRandom or DiscardRandom tag with no coroutine, so it resolves synchronously to AwaitingAction.
#[test]
fn for_each_player_wraps_a_bare_sacrifice_random_tag_with_no_player_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let p0_bear = ObjectId(1);
    let p1_bear = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(
            card,
            sorcery_definition(1, EACH_PLAYER_SACRIFICES_A_CREATURE_AT_RANDOM_BARE_TAG),
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
            p0_bear,
            creature_definition(2, "P0 Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            p1_bear,
            creature_definition(3, "P1 Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );

    let mut engine = Engine::new(state);
    register_script_operation(
        &mut engine,
        card,
        EACH_PLAYER_SACRIFICES_A_CREATURE_AT_RANDOM_BARE_TAG,
    );
    let status = cast_and_resolve(&mut engine, p0, p1, card);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "no player choice - resolves synchronously, got {status:?}"
    );

    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .is_empty(),
        "both players' only creature sacrificed"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .len(),
        2,
        "P0 Bear + the sorcery itself"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p1))
            .unwrap()
            .contents
            .len(),
        1,
        "P1 Bear"
    );
}

// Any other Game.Ops tag handed to ForEachPlayer is a loud load-time error and not a silent no-op.
#[test]
fn for_each_player_rejects_an_unsupported_bare_tag() {
    const BAD_SCRIPT: &str = r#"
    local s = {}
    function s.operation()
        return Game.Ops.ForEachPlayer(Game.Ops.Destroy)
    end
    return s
    "#;
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(card, sorcery_definition(1, BAD_SCRIPT), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);
    let engine = Engine::new(state);

    let err = engine
        .build_operation(BAD_SCRIPT)
        .expect_err("Destroy isn't SacrificeRandom/DiscardRandom");
    let message = format!("{err:?}");
    assert!(
        message.contains("SacrificeRandom") && message.contains("DiscardRandom"),
        "got {message}"
    );
}
