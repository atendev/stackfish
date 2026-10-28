//! CR 612.5: Exchange of Words exchanges the text boxes of two objects. Only `oracle_text`, `keywords` and `script_ref` cross over, and name, cost, color, P/T and types stay (CR 612.1), unlike a CR 707 copy.
//! `ability::apply_text_box` overwrites those fields, then `purge_granted_abilities` and a fresh `run_card_script` run for both targets, as `BecomeCopy` does. It is one-time, with no reversion.
//! The effect is pairwise: `ResolveStackTop` rechecks both targets and no-ops the whole effect unless both are still legal.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const SELF_REGISTER_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

fn flying_creature(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Flying".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(SELF_REGISTER_GAIN_LIFE.to_string()),
        keywords: KeywordSet::FLYING,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn trample_creature(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Trample".to_string(),
        power: Some(PTValue::Fixed(5)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::TRAMPLE,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_word() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object { mana_value: None,
            card_types: Some(CardTypeSet::CREATURE),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }
}

fn exchange_of_words_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Exchange of Words".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Exchange the text boxes of target creature and target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Multiple(vec![creature_word(), creature_word()])),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::ExchangeTextBoxes),
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand");
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

#[test]
fn exchanging_swaps_keywords_oracle_text_and_registered_abilities_not_name_or_pt() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let flyer = ObjectId(0);
    let trampler = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(flyer, flying_creature(1, "Bird"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(trampler, trample_creature(2, "Rhino"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, exchange_of_words_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(flyer), Target::Object(trampler)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::TextBoxesExchanged { a, b } if a == flyer && b == trampler)),
        "the exchange event was logged"
    );

    let runtime = ScriptRuntime::new();
    let bird_now = effective_characteristics(engine.state(), &runtime, flyer);
    let rhino_now = effective_characteristics(engine.state(), &runtime, trampler);

    // Text box crossed over.
    assert!(bird_now.keywords.contains(KeywordSet::TRAMPLE) && !bird_now.keywords.contains(KeywordSet::FLYING));
    assert!(rhino_now.keywords.contains(KeywordSet::FLYING) && !rhino_now.keywords.contains(KeywordSet::TRAMPLE));
    assert_eq!(engine.state().object(flyer).unwrap().copiable_override.as_ref().unwrap().oracle_text, "Trample");
    assert_eq!(engine.state().object(trampler).unwrap().copiable_override.as_ref().unwrap().oracle_text, "Flying");

    // Name/P&T stayed each permanent's own - not a CR 707 copy.
    assert_eq!(bird_now.name, "Bird");
    assert_eq!(rhino_now.name, "Rhino");
    assert_eq!((bird_now.power, bird_now.toughness), (Some(2), Some(2)));
    assert_eq!((rhino_now.power, rhino_now.toughness), (Some(5), Some(5)));

    // Registered abilities crossed over too: the flyer's triggered ability is now on the trampler's id, and the flyer has none.
    assert_eq!(engine.state().abilities.iter().filter(|a| a.source == flyer).count(), 0);
    assert_eq!(engine.state().abilities.iter().filter(|a| a.source == trampler).count(), 1);
}

#[test]
fn one_target_leaving_before_resolution_no_ops_the_whole_effect() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let flyer = ObjectId(0);
    let trampler = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(flyer, flying_creature(1, "Bird"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(trampler, trample_creature(2, "Rhino"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, exchange_of_words_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(flyer), Target::Object(trampler)]);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    // The trampler leaves the battlefield before resolution, so only one target remains legal.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(trampler);
    engine.state_mut().objects.remove(&trampler);

    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::TextBoxesExchanged { .. })),
        "no partner to exchange with - the effect is a no-op, not a one-sided change"
    );
    let runtime = ScriptRuntime::new();
    let bird_now = effective_characteristics(engine.state(), &runtime, flyer);
    assert!(bird_now.keywords.contains(KeywordSet::FLYING), "the surviving target's own text box is untouched");
    assert!(engine.state().object(flyer).unwrap().copiable_override.is_none());
}

#[test]
fn both_targets_leaving_before_resolution_fizzles_the_whole_spell() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let flyer = ObjectId(0);
    let trampler = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(flyer, flying_creature(1, "Bird"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(trampler, trample_creature(2, "Rhino"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, exchange_of_words_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(flyer), Target::Object(trampler)]);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    for id in [flyer, trampler] {
        engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(id);
        engine.state_mut().objects.remove(&id);
    }

    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: every target across every word is illegal - the whole spell fizzles"
    );
}

#[test]
fn game_ops_exchange_text_boxes_decodes() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime.load_operation("return Game.Ops.ExchangeTextBoxes").expect("primitive loads");
    assert!(matches!(effect, Some(AbilityEffect::ExchangeTextBoxes)), "expected Some(AbilityEffect::ExchangeTextBoxes), got {effect:?}");
}
