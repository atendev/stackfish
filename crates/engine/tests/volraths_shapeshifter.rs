//! CR 612.6: Volrath's Shapeshifter changes the full text of another object (name, mana cost, color indicator, P/T, type line, keywords) as a live layer 3 `TextChanging` `ContinuousEffect`, so its "until end of turn" duration reuses `EffectDuration`.
//! `EffectPrimitive::SetFullText` and `FullTextOverride` cover the characteristic half only, because `effective_characteristics` is a read-only query that cannot re-run scripts or grant abilities.
// TODO: an activated ability that captures "target creature card in a graveyard" into a `SetFullText` effect, so tests build the `ContinuousEffect` directly

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

fn creature_definition(definition_id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: vec![Subtype("Shapeshifter".to_string())],
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("vanilla".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn dread_wanderer_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Dread Wanderer".to_string(),
        mana_cost: ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Black))] },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: vec![Subtype("Zombie".to_string())],
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("dread_wanderer".to_string()),
        keywords: KeywordSet::MENACE,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState { step: Step::PreCombatMain, ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard])) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn full_text_effect(source: ObjectId, donor: &CardDefinition, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::TextChanging,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::SetFullText(Box::new(FullTextOverride::from(donor)))),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::UntilEndOfTurn,
        condition: None,
    }
}

#[test]
fn full_text_override_colors_follows_mana_cost_when_no_color_indicator() {
    let donor = dread_wanderer_definition();
    let override_ = FullTextOverride::from(&donor);
    assert_eq!(override_.colors(), ColorSet::BLACK);
}

#[test]
fn set_full_text_swaps_name_mana_cost_color_type_line_keywords_and_pt() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let shifter = ObjectId(1);
    put_on_battlefield(&mut state, card_object(shifter, creature_definition(1, "Volrath's Shapeshifter", 2, 2), p0, ZoneKind::Battlefield));
    let donor = dread_wanderer_definition();
    state.continuous_effects.push(full_text_effect(shifter, &donor, 0));

    let runtime = ScriptRuntime::new();
    let effective = layers::effective_characteristics(&state, &runtime, shifter);

    assert_eq!(effective.name, "Dread Wanderer");
    assert_eq!(effective.mana_cost.symbols, donor.mana_cost.symbols);
    assert_eq!(effective.colors, ColorSet::BLACK);
    assert_eq!(effective.card_types, CardTypeSet::CREATURE);
    assert_eq!(effective.subtypes, vec![Subtype("Zombie".to_string())], "type line - subtypes come from the donor too");
    assert!(effective.keywords.contains(KeywordSet::MENACE), "text box - keywords come from the donor too");
    assert_eq!(effective.power, Some(2));
    assert_eq!(effective.toughness, Some(1));
}

#[test]
fn set_full_text_does_not_affect_an_unrelated_object() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let shifter = ObjectId(1);
    let bear = ObjectId(2);
    put_on_battlefield(&mut state, card_object(shifter, creature_definition(1, "Volrath's Shapeshifter", 2, 2), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(bear, creature_definition(3, "Bear", 2, 2), p0, ZoneKind::Battlefield));
    let donor = dread_wanderer_definition();
    state.continuous_effects.push(full_text_effect(shifter, &donor, 0));

    let runtime = ScriptRuntime::new();
    let effective = layers::effective_characteristics(&state, &runtime, bear);
    assert_eq!(effective.name, "Bear", "self_only - the effect never applies to a different object");
}

#[test]
fn a_later_plain_set_name_overrides_just_the_name_on_top_of_set_full_text() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let shifter = ObjectId(1);
    put_on_battlefield(&mut state, card_object(shifter, creature_definition(1, "Volrath's Shapeshifter", 2, 2), p0, ZoneKind::Battlefield));
    let donor = dread_wanderer_definition();
    state.continuous_effects.push(full_text_effect(shifter, &donor, 0));
    state.continuous_effects.push(ContinuousEffect {
        source: shifter,
        layer: Layer::TextChanging,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::SetName("Renamed".to_string())),
        timestamp: SequenceNo(1),
        duration: EffectDuration::UntilEndOfTurn,
        condition: None,
    });

    let runtime = ScriptRuntime::new();
    let effective = layers::effective_characteristics(&state, &runtime, shifter);

    assert_eq!(effective.name, "Renamed", "the later-timestamp plain SetName wins for the name specifically");
    assert_eq!(effective.mana_cost.symbols, donor.mana_cost.symbols, "every other field still comes from the earlier SetFullText");
    assert_eq!(effective.power, Some(2));
}

#[test]
fn an_earlier_plain_set_name_is_superseded_by_a_later_set_full_text() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let shifter = ObjectId(1);
    put_on_battlefield(&mut state, card_object(shifter, creature_definition(1, "Volrath's Shapeshifter", 2, 2), p0, ZoneKind::Battlefield));
    state.continuous_effects.push(ContinuousEffect {
        source: shifter,
        layer: Layer::TextChanging,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::SetName("Renamed".to_string())),
        timestamp: SequenceNo(0),
        duration: EffectDuration::UntilEndOfTurn,
        condition: None,
    });
    let donor = dread_wanderer_definition();
    state.continuous_effects.push(full_text_effect(shifter, &donor, 1));

    let runtime = ScriptRuntime::new();
    let effective = layers::effective_characteristics(&state, &runtime, shifter);

    assert_eq!(effective.name, "Dread Wanderer", "SetFullText's own later timestamp wins outright, including the name");
}

fn state_at_end_step() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        next_object_id: 100,
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Command, ZoneKind::Battlefield, ZoneKind::Exile]))
    }
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
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).expect("PassPriority should be accepted");
}

fn pass_through_end_step(engine: &mut Engine) -> StepStatus {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p0), "expected p0 end-step priority, got {s:?}");
    pass(engine, p0);
    step_until_pause(engine);
    pass(engine, p1);
    step_until_pause(engine)
}

#[test]
fn set_full_text_expires_at_cleanup_like_any_other_until_end_of_turn_effect() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let shifter = ObjectId(1);
    put_on_battlefield(&mut state, card_object(shifter, creature_definition(1, "Volrath's Shapeshifter", 2, 2), p0, ZoneKind::Battlefield));
    let donor = dread_wanderer_definition();
    state.continuous_effects.push(full_text_effect(shifter, &donor, 0));

    let mut engine = Engine::new(state);
    let runtime = ScriptRuntime::new();
    assert_eq!(layers::effective_characteristics(engine.state(), &runtime, shifter).name, "Dread Wanderer");

    pass_through_end_step(&mut engine);

    assert!(engine.state().continuous_effects.is_empty(), "the UntilEndOfTurn SetFullText effect is drained at cleanup");
    let effective = layers::effective_characteristics(engine.state(), &runtime, shifter);
    assert_eq!(effective.name, "Volrath's Shapeshifter", "reverted to its own printed identity");
    assert_eq!(effective.power, Some(2));
}
