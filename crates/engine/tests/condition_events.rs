//! CR 603.2: the event table a Lua trigger condition reads for each bridged `GameEventKind`, and the
//! `Game.Conditions.*` tags built on them: BecomesTapped, BecomesUntapped, GainsLife, LosesLife,
//! Attacks, Blocks and Mills.

use std::collections::HashMap;

use stackfish_engine::engine::Engine;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;
use stackfish_testutil::{
    assign_combat_damage, card_object, cast_spell, declare_attackers, minimal_card_definition,
    pass_both_and_drain, pay_free_and_drain, put_on_battlefield, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATCHER: ObjectId = ObjectId(5);
const OTHER: ObjectId = ObjectId(6);

fn creature(name: &str, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(3)),
        toughness: Some(PTValue::Fixed(3)),
        keywords,
        ..minimal_card_definition(
            1,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn state_with(keywords: KeywordSet) -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 0)]),
        ..two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    let mut watcher = card_object(
        WATCHER,
        creature("Watcher", keywords),
        P0,
        ZoneKind::Battlefield,
    );
    watcher.controlled_since_turn = 0;
    put_on_battlefield(&mut state, watcher);
    put_on_battlefield(
        &mut state,
        card_object(
            OTHER,
            creature("Other", KeywordSet::empty()),
            P1,
            ZoneKind::Battlefield,
        ),
    );
    state
}

fn ability(condition: ConditionSpec) -> RegisteredAbility {
    RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: WATCHER,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn event(kind: GameEventKind) -> GameEvent {
    GameEvent::new(EventId(0), SequenceNo(0), kind)
}

/// Whether a watcher ability with `condition` triggers on `kind`.
fn fires(
    state: &GameState,
    runtime: &ScriptRuntime,
    condition: ConditionSpec,
    kind: GameEventKind,
) -> bool {
    let mut state = state.clone();
    state.abilities = vec![ability(condition)];
    let (pending, errors) = collect_triggers(&state, &event(kind), runtime, &[]);
    assert!(errors.is_empty(), "{errors:?}");
    !pending.is_empty()
}

fn lua_fires(source: &str, kind: GameEventKind) -> bool {
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_condition(source).expect("condition loads");
    fires(&state_with(KeywordSet::empty()), &runtime, condition, kind)
}

fn tag_fires(state: &GameState, tag: ConditionPrimitive, kind: GameEventKind) -> bool {
    fires(
        state,
        &ScriptRuntime::new(),
        ConditionSpec::Primitive(tag),
        kind,
    )
}

/// How many events in the engine's ledger a watcher with `tag` triggers on.
fn ledger_fires(engine: &Engine, tag: ConditionPrimitive) -> usize {
    engine
        .state()
        .ledger
        .iter()
        .filter(|e| tag_fires(engine.state(), tag.clone(), e.kind.clone()))
        .count()
}

// CR 603.2
#[test]
fn zone_change_table_has_object_from_and_to() {
    let src = r#"return function(source, controller, event)
        return event.kind == "zone_change" and event.object == 9
            and event.from == "library" and event.to == "hand"
    end"#;
    assert!(lua_fires(
        src,
        GameEventKind::ZoneChange {
            object: ObjectId(9),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: None,
        }
    ));
}

// CR 603.2
#[test]
fn damage_dealt_table_has_source_target_amount_and_combat_flag() {
    let src = r#"return function(source, controller, event)
        return event.kind == "damage_dealt" and event.source == source
            and event.target.kind == "player" and event.target.id == 1
            and event.amount == 3 and event.is_combat_damage == true
    end"#;
    assert!(lua_fires(
        src,
        GameEventKind::DamageDealt {
            source: WATCHER,
            target: Target::Player(P1),
            amount: 3,
            is_combat_damage: true,
        }
    ));
}

// CR 119.3
#[test]
fn life_changed_table_has_player_and_signed_delta() {
    let src = r#"return function(source, controller, event)
        return event.kind == "life_changed" and event.player == controller and event.delta == -2
    end"#;
    assert!(lua_fires(
        src,
        GameEventKind::LifeChanged {
            player: P0,
            delta: -2,
        }
    ));
}

// CR 603.2e
#[test]
fn tapped_and_untapped_tables_have_object() {
    let tapped = r#"return function(source, controller, event)
        return event.kind == "tapped" and event.object == source
    end"#;
    let untapped = r#"return function(source, controller, event)
        return event.kind == "untapped" and event.object == source
    end"#;
    assert!(lua_fires(tapped, GameEventKind::Tapped { object: WATCHER }));
    assert!(lua_fires(
        untapped,
        GameEventKind::Untapped { object: WATCHER }
    ));
}

// CR 122.1
#[test]
fn counter_tables_name_the_counter_kind() {
    let object = r#"return function(source, controller, event)
        return event.kind == "counter_added" and event.object == source
            and event.counter == "PlusOnePlusOne" and event.amount == 2
    end"#;
    let player = r#"return function(source, controller, event)
        return event.kind == "player_counter_added" and event.player == 1
            and event.counter == "Poison" and event.amount == 1
    end"#;
    assert!(lua_fires(
        object,
        GameEventKind::CounterAdded {
            object: WATCHER,
            kind: CounterKind::PlusOnePlusOne,
            amount: 2,
        }
    ));
    assert!(lua_fires(
        player,
        GameEventKind::PlayerCounterAdded {
            player: P1,
            kind: CounterKind::Poison,
            amount: 1,
        }
    ));
}

// CR 508.1m, 509.1i
#[test]
fn declaration_tables_have_attacker_target_and_blocker() {
    let attack = r#"return function(source, controller, event)
        return event.kind == "attacker_declared" and event.attacker == source
            and event.target.kind == "player" and event.target.id == 1
    end"#;
    let block = r#"return function(source, controller, event)
        return event.kind == "blocker_declared" and event.blocker == source and event.attacker == 6
    end"#;
    assert!(lua_fires(
        attack,
        GameEventKind::AttackerDeclared {
            attacker: WATCHER,
            target: Target::Player(P1),
        }
    ));
    assert!(lua_fires(
        block,
        GameEventKind::BlockerDeclared {
            blocker: WATCHER,
            attacker: OTHER,
        }
    ));
}

// CR 701.17a, 701.17c
#[test]
fn cards_milled_table_has_player_and_card_ids() {
    let src = r#"return function(source, controller, event)
        return event.kind == "cards_milled" and event.player == controller
            and #event.cards == 2 and event.cards[1] == 11 and event.cards[2] == 12
    end"#;
    assert!(lua_fires(
        src,
        GameEventKind::CardsMilled {
            player: P0,
            cards: vec![ObjectId(11), ObjectId(12)],
        }
    ));
}

// CR 603.2
#[test]
fn a_three_argument_condition_still_works_and_sees_nil_for_an_unbridged_event() {
    let src = r#"return function(source, controller, event) return event == nil end"#;
    assert!(lua_fires(
        src,
        GameEventKind::StepStarted { step: Step::Upkeep }
    ));
    let zone = r#"return function(source, controller, event)
        return event ~= nil and event.kind == "zone_change"
    end"#;
    assert!(lua_fires(
        zone,
        GameEventKind::ZoneChange {
            object: ObjectId(9),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        }
    ));
}

// CR 701.22a, 701.25a
#[test]
fn scry_and_surveil_stay_nil() {
    let src = r#"return function(source, controller, event) return event == nil end"#;
    assert!(lua_fires(
        src,
        GameEventKind::CardsScried {
            player: P0,
            kept_on_top: vec![ObjectId(20)],
            put_bottom: Vec::new(),
        }
    ));
    assert!(lua_fires(
        src,
        GameEventKind::CardsSurveiled {
            player: P0,
            kept_on_top: vec![ObjectId(20)],
            put_graveyard: Vec::new(),
        }
    ));
}

// CR 603.2e
#[test]
fn becomes_tapped_and_untapped_fire_only_for_the_source() {
    let state = state_with(KeywordSet::empty());
    let tag = ConditionPrimitive::BecomesTapped;
    assert!(tag_fires(
        &state,
        tag.clone(),
        GameEventKind::Tapped { object: WATCHER }
    ));
    assert!(!tag_fires(
        &state,
        tag.clone(),
        GameEventKind::Tapped { object: OTHER }
    ));
    assert!(!tag_fires(
        &state,
        tag,
        GameEventKind::Untapped { object: WATCHER }
    ));
    let tag = ConditionPrimitive::BecomesUntapped;
    assert!(tag_fires(
        &state,
        tag.clone(),
        GameEventKind::Untapped { object: WATCHER }
    ));
    assert!(!tag_fires(
        &state,
        tag,
        GameEventKind::Untapped { object: OTHER }
    ));
}

// CR 119.3, 119.9
#[test]
fn gains_and_loses_life_split_on_the_sign_for_the_controller_only() {
    let state = state_with(KeywordSet::empty());
    let change = |player, delta| GameEventKind::LifeChanged { player, delta };
    assert!(tag_fires(
        &state,
        ConditionPrimitive::GainsLife,
        change(P0, 3)
    ));
    assert!(!tag_fires(
        &state,
        ConditionPrimitive::GainsLife,
        change(P0, -3)
    ));
    assert!(!tag_fires(
        &state,
        ConditionPrimitive::GainsLife,
        change(P1, 3)
    ));
    assert!(tag_fires(
        &state,
        ConditionPrimitive::LosesLife,
        change(P0, -1)
    ));
    assert!(!tag_fires(
        &state,
        ConditionPrimitive::LosesLife,
        change(P0, 1)
    ));
    assert!(
        !tag_fires(&state, ConditionPrimitive::LosesLife, change(P1, -1)),
        "an opponent's life loss is not yours"
    );
}

// CR 508.3a, 509.3a
#[test]
fn attacks_and_blocks_fire_only_for_the_source() {
    let state = state_with(KeywordSet::empty());
    let attack = |attacker| GameEventKind::AttackerDeclared {
        attacker,
        target: Target::Player(P1),
    };
    assert!(tag_fires(
        &state,
        ConditionPrimitive::Attacks,
        attack(WATCHER)
    ));
    assert!(!tag_fires(
        &state,
        ConditionPrimitive::Attacks,
        attack(OTHER)
    ));
    let block = |blocker| GameEventKind::BlockerDeclared {
        blocker,
        attacker: ObjectId(99),
    };
    assert!(tag_fires(
        &state,
        ConditionPrimitive::Blocks,
        block(WATCHER)
    ));
    assert!(!tag_fires(&state, ConditionPrimitive::Blocks, block(OTHER)));
}

// CR 701.17a
#[test]
fn mills_fires_for_the_controller_even_with_no_public_cards() {
    let state = state_with(KeywordSet::empty());
    let milled = |player| GameEventKind::CardsMilled {
        player,
        cards: Vec::new(),
    };
    assert!(tag_fires(&state, ConditionPrimitive::Mills, milled(P0)));
    assert!(
        !tag_fires(&state, ConditionPrimitive::Mills, milled(P1)),
        "an opponent's mill is not yours"
    );
}

// CR 603.2e, 702.20a
#[test]
fn a_vigilance_attacker_fires_attacks_but_not_becomes_tapped() {
    let mut state = state_with(KeywordSet::VIGILANCE);
    state.phase = Phase::Combat;
    state.step = Step::DeclareAttackers;
    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, P0, vec![(WATCHER, Target::Player(P1))]).unwrap();

    assert_eq!(ledger_fires(&engine, ConditionPrimitive::Attacks), 1);
    assert_eq!(ledger_fires(&engine, ConditionPrimitive::BecomesTapped), 0);
}

// CR 603.2e, 508.1f
#[test]
fn an_ordinary_attacker_fires_attacks_and_becomes_tapped() {
    let mut state = state_with(KeywordSet::empty());
    state.phase = Phase::Combat;
    state.step = Step::DeclareAttackers;
    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, P0, vec![(WATCHER, Target::Player(P1))]).unwrap();

    assert_eq!(ledger_fires(&engine, ConditionPrimitive::Attacks), 1);
    assert_eq!(ledger_fires(&engine, ConditionPrimitive::BecomesTapped), 1);
}

fn combat_damage_engine(keywords: KeywordSet) -> Engine {
    let mut state = state_with(keywords);
    state.phase = Phase::Combat;
    state.step = Step::CombatDamage;
    state.combat = Some(CombatState {
        attackers: vec![(WATCHER, Target::Player(P1))],
        blocks: Vec::new(),
        declared_defenders: vec![P1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        bands_declared: false,
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    });
    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, P0, Vec::new());
    engine
}

/// The ledger events after the combat damage, as Lua `kind` names a condition sees.
fn seen_kinds(engine: &Engine) -> Vec<&'static str> {
    let probes = ["damage_dealt", "life_changed", "player_counter_added"];
    let mut seen = Vec::new();
    for e in &engine.state().ledger {
        for kind in probes {
            let src = format!(
                "return function(source, controller, event) return event ~= nil and event.kind == \"{kind}\" end"
            );
            let mut runtime = ScriptRuntime::new();
            let condition = runtime.load_condition(&src).unwrap();
            if fires(engine.state(), &runtime, condition, e.kind.clone()) {
                seen.push(kind);
            }
        }
    }
    seen
}

// CR 120.3a, 603.2
#[test]
fn damage_to_a_player_is_seen_as_damage_then_a_separate_life_change() {
    let engine = combat_damage_engine(KeywordSet::empty());
    assert_eq!(seen_kinds(&engine), vec!["damage_dealt", "life_changed"]);
}

// CR 702.90b
#[test]
fn infect_damage_to_a_player_is_seen_as_poison_and_no_life_change() {
    let engine = combat_damage_engine(KeywordSet::INFECT);
    assert_eq!(
        seen_kinds(&engine),
        vec!["damage_dealt", "player_counter_added"]
    );
}

// CR 400.7
#[test]
fn a_drawn_cards_bridged_object_is_its_new_hand_id() {
    let mut state = state_with(KeywordSet::empty());
    let library_card = ObjectId(30);
    state.objects.insert(
        library_card,
        card_object(
            library_card,
            creature("Drawn", KeywordSet::empty()),
            P0,
            ZoneKind::Library,
        ),
    );
    state
        .zone_mut(ZoneKind::Library, Some(P0))
        .unwrap()
        .push_bottom(library_card);
    let spell = ObjectId(31);
    let definition = CardDefinition {
        effect: Some(AbilityEffect::DrawCards(1)),
        ..minimal_card_definition(
            2,
            "Divination",
            "Divination",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::SORCERY,
        )
    };
    state
        .objects
        .insert(spell, card_object(spell, definition, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(spell);
    let mut engine = Engine::new(state);
    cast_spell(&mut engine, P0, spell, Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);

    let hand = &engine
        .state()
        .zone(ZoneKind::Hand, Some(P0))
        .unwrap()
        .contents;
    assert_eq!(hand.len(), 1);
    let hand_id = hand[0];
    assert_ne!(hand_id, library_card);
    let draw = engine
        .state()
        .ledger
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                GameEventKind::ZoneChange {
                    from: ZoneKind::Library,
                    to: ZoneKind::Hand,
                    ..
                }
            )
        })
        .expect("the draw is logged");
    let src = format!(
        "return function(source, controller, event) return event.object == {} end",
        hand_id.0
    );
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_condition(&src).unwrap();
    assert!(fires(
        engine.state(),
        &runtime,
        condition,
        draw.kind.clone()
    ));
}

// CR 603.2e
#[test]
fn becomes_tapped_binds_through_set_condition_and_once_per_turn() {
    for (condition, once) in [
        ("Game.Conditions.BecomesTapped", false),
        (
            "Game.Conditions.OncePerTurn(Game.Conditions.BecomesTapped)",
            true,
        ),
    ] {
        let mut runtime = ScriptRuntime::new();
        let spec = runtime
            .load_condition(&format!("return {condition}"))
            .unwrap();
        let expected = if once {
            ConditionSpec::PrimitiveOncePerTurn(ConditionPrimitive::BecomesTapped)
        } else {
            ConditionSpec::Primitive(ConditionPrimitive::BecomesTapped)
        };
        assert_eq!(spec, expected);
    }
    for name in [
        "BecomesUntapped",
        "GainsLife",
        "LosesLife",
        "Attacks",
        "Blocks",
        "Mills",
    ] {
        let mut runtime = ScriptRuntime::new();
        assert!(
            matches!(
                runtime.load_condition(&format!("return Game.Conditions.{name}")),
                Ok(ConditionSpec::Primitive(_))
            ),
            "{name}"
        );
    }
}
