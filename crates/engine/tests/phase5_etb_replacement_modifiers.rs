//! CR 603.6d, 614.1c: how a permanent enters the battlefield, via `ReplacementPayload::{EntersTapped, EntersWithCounters}` on `EventFilter::WouldMoveZone(_, Battlefield)`.
//! Covers a card's own registered ability (CR 113.6h) and an external grant, applied by `apply_enters_battlefield_modifiers` from `PlayLand`, `CastSpell` resolution and `submit_search_choice`.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const ENTERS_TAPPED: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const ENTERS_WITH_TWO_PLUS_ONE_COUNTERS: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCounters("PlusOnePlusOne", 2))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const OPPONENTS_LANDS_ENTER_TAPPED: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:SetAffected({ controller = "opponent", card_types = { "Land" } })
    e:Register()
end
return s
"#;

// Narrowed by subtype, which checks that SetAffected reads `subtypes` through the shared decode_effect_filter_table.
const OPPONENTS_SWAMPS_ENTER_TAPPED: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:SetAffected({ controller = "opponent", subtypes = { "Swamp" } })
    e:Register()
end
return s
"#;

// Narrowed by supertype, which checks that event_target_matches reads `supertypes`.
const OPPONENTS_LEGENDARY_LANDS_ENTER_TAPPED: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:SetAffected({ controller = "opponent", supertypes = { "Legendary" } })
    e:Register()
end
return s
"#;

// CR 603.6d, 614.1c
// A move to the Graveyard must be rejected at :Register() time and not accepted as an effect that can never match.
const MISMATCHED_ENTERS_TAPPED_FILTER: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:Register()
end
return s
"#;

fn land_definition(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
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

fn subtyped_land_definition(name: &str, subtypes: Vec<Subtype>) -> CardDefinition {
    let mut definition = land_definition(name, "");
    definition.script_ref = ScriptReference::ByKey(name.to_string());
    definition.subtypes = subtypes;
    definition
}

fn supertyped_land_definition(name: &str, supertypes: SupertypeSet) -> CardDefinition {
    let mut definition = land_definition(name, "");
    definition.script_ref = ScriptReference::ByKey(name.to_string());
    definition.supertypes = supertypes;
    definition
}

fn creature_definition(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
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

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected at least one ZoneChange event in the ledger");
}

#[test]
fn set_filter_would_move_zone_battlefield_and_enters_tapped_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = land_definition("Tapped Forest", ENTERS_TAPPED);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: None, to: Some(ZoneKind::Battlefield), affected },
            payload: ReplacementPayload::EntersTapped,
        } => {
            assert!(affected.self_only);
        }
        other => panic!("expected a WouldMoveZone(nil, Battlefield)/EntersTapped primitive spec, got {other:?}"),
    }
}

#[test]
fn set_filter_would_move_zone_battlefield_and_enters_with_counters_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition("Fading Bear", ENTERS_WITH_TWO_PLUS_ONE_COUNTERS);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { to: Some(ZoneKind::Battlefield), .. },
            payload: ReplacementPayload::EntersWithCounters { kind: CounterKind::PlusOnePlusOne, amount: 2 },
        } => {}
        other => panic!("expected a WouldMoveZone(_, Battlefield)/EntersWithCounters(PlusOnePlusOne, 2) spec, got {other:?}"),
    }
}

#[test]
fn enters_tapped_payload_rejects_a_non_battlefield_destination_filter() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = land_definition("Broken", MISMATCHED_ENTERS_TAPPED_FILTER);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let result = ScriptRuntime::new().run_card_script(&mut state, &definition, source);

    assert!(result.is_err(), "WouldMoveZone(Battlefield, Graveyard) + EntersTapped is a real mismatch");
    assert!(state.replacement_effects.is_empty(), "no partial effect should have been committed");
}

#[test]
fn play_land_self_registered_enters_tapped_makes_land_enter_tapped() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition("Tapped Forest", ENTERS_TAPPED), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } }).unwrap();
    drain(&mut engine);

    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Hand, ZoneKind::Battlefield));
    assert!(engine.state().object(new_id).unwrap().tapped, "the land's own EntersTapped ability must have applied");
}

#[test]
fn play_land_without_a_modifier_still_enters_untapped() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    // ByKey, so no script runs, as with the vanilla definitions in phase5_replacement_effect_shapes.rs.
    let mut definition = land_definition("Plain Forest", "");
    definition.script_ref = ScriptReference::ByKey("plain_forest".to_string());
    state.objects.insert(card, card_object(card, definition, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } }).unwrap();
    drain(&mut engine);

    let (new_id, ..) = last_zone_change(&engine);
    assert!(!engine.state().object(new_id).unwrap().tapped, "no modifier registered - must enter untapped");
}

#[test]
fn externally_granted_enters_tapped_affects_an_opponents_land_via_play_land() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // The granter (p1's permanent) is on the battlefield before p0 plays their land, as with the combat-damage shield tests.
    let granter = ObjectId(0);
    let granter_definition = creature_definition("Tangle Wire Wannabe", OPPONENTS_LANDS_ENTER_TAPPED);
    state.objects.insert(granter, card_object(granter, granter_definition.clone(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(granter);
    ScriptRuntime::new()
        .run_card_script(&mut state, &granter_definition, granter)
        .expect("granter script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let card = ObjectId(1);
    let mut definition = land_definition("Plain Forest", "");
    definition.script_ref = ScriptReference::ByKey("plain_forest".to_string());
    state.objects.insert(card, card_object(card, definition, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } }).unwrap();
    drain(&mut engine);

    let (new_id, ..) = last_zone_change(&engine);
    assert!(
        engine.state().object(new_id).unwrap().tapped,
        "p0's land must enter tapped - p1's externally granted effect applies to p0 as p1's opponent"
    );
    assert_eq!(engine.state().replacement_effects.len(), 1, "a Repeatable granter stays registered");
}

#[test]
fn externally_granted_enters_tapped_only_affects_the_matching_subtype() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let granter = ObjectId(0);
    let granter_definition = creature_definition("Subtype Tangle Wire Wannabe", OPPONENTS_SWAMPS_ENTER_TAPPED);
    state.objects.insert(granter, card_object(granter, granter_definition.clone(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(granter);
    ScriptRuntime::new()
        .run_card_script(&mut state, &granter_definition, granter)
        .expect("granter script self-registers cleanly");

    let swamp = ObjectId(1);
    state.objects.insert(
        swamp,
        card_object(swamp, subtyped_land_definition("Plain Swamp", vec![Subtype("Swamp".to_string())]), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(swamp);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: swamp } }).unwrap();
    drain(&mut engine);

    let (new_id, ..) = last_zone_change(&engine);
    assert!(engine.state().object(new_id).unwrap().tapped, "p0's Swamp must enter tapped - matches the subtype filter");
}

#[test]
fn externally_granted_enters_tapped_leaves_a_non_matching_subtype_alone() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let granter = ObjectId(0);
    let granter_definition = creature_definition("Subtype Tangle Wire Wannabe", OPPONENTS_SWAMPS_ENTER_TAPPED);
    state.objects.insert(granter, card_object(granter, granter_definition.clone(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(granter);
    ScriptRuntime::new()
        .run_card_script(&mut state, &granter_definition, granter)
        .expect("granter script self-registers cleanly");

    let forest = ObjectId(1);
    state.objects.insert(
        forest,
        card_object(forest, subtyped_land_definition("Plain Forest Two", vec![Subtype("Forest".to_string())]), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(forest);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: forest } }).unwrap();
    drain(&mut engine);

    let (new_id, ..) = last_zone_change(&engine);
    assert!(
        !engine.state().object(new_id).unwrap().tapped,
        "p0's Forest must enter untapped - doesn't match the Swamp-only subtype filter"
    );
}

#[test]
fn externally_granted_enters_tapped_only_affects_the_matching_supertype() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let granter = ObjectId(0);
    let granter_definition =
        creature_definition("Supertype Tangle Wire Wannabe", OPPONENTS_LEGENDARY_LANDS_ENTER_TAPPED);
    state.objects.insert(granter, card_object(granter, granter_definition.clone(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(granter);
    ScriptRuntime::new()
        .run_card_script(&mut state, &granter_definition, granter)
        .expect("granter script self-registers cleanly");

    let legendary_land = ObjectId(1);
    state.objects.insert(
        legendary_land,
        card_object(legendary_land, supertyped_land_definition("Legendary Land", SupertypeSet::LEGENDARY), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(legendary_land);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: legendary_land } }).unwrap();
    drain(&mut engine);

    let (new_id, ..) = last_zone_change(&engine);
    assert!(
        engine.state().object(new_id).unwrap().tapped,
        "p0's legendary land must enter tapped - matches the supertype filter"
    );
}

#[test]
fn externally_granted_enters_tapped_leaves_a_non_matching_supertype_alone() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let granter = ObjectId(0);
    let granter_definition =
        creature_definition("Supertype Tangle Wire Wannabe", OPPONENTS_LEGENDARY_LANDS_ENTER_TAPPED);
    state.objects.insert(granter, card_object(granter, granter_definition.clone(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(granter);
    ScriptRuntime::new()
        .run_card_script(&mut state, &granter_definition, granter)
        .expect("granter script self-registers cleanly");

    let plain_land = ObjectId(1);
    state.objects.insert(
        plain_land,
        card_object(plain_land, supertyped_land_definition("Plain Nonlegendary Land", SupertypeSet::empty()), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(plain_land);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: plain_land } }).unwrap();
    drain(&mut engine);

    let (new_id, ..) = last_zone_change(&engine);
    assert!(
        !engine.state().object(new_id).unwrap().tapped,
        "p0's plain land must enter untapped - doesn't match the Legendary-only supertype filter"
    );
}

#[test]
fn cast_creature_resolution_self_registered_enters_with_counters() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, creature_definition("Fading Bear", ENTERS_WITH_TWO_PLUS_ONE_COUNTERS), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .unwrap();
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let (resolved_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Battlefield));
    let resolved = engine.state().object(resolved_id).unwrap();
    assert_eq!(resolved.zone, ZoneKind::Battlefield);
    assert_eq!(
        resolved.counters.count(&CounterKind::PlusOnePlusOne),
        2,
        "the creature's own EntersWithCounters ability must have applied"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(&e.kind, GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount: 2 } if *object == resolved_id)),
        "a CounterAdded event should be logged for observability, same convention Game.Ops.Counters uses"
    );
}

#[test]
fn limited_uses_enters_with_counters_is_consumed_after_first_entry() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let granter = ObjectId(0);
    state.objects.insert(granter, card_object(granter, creature_definition("Granter", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(ReplacementEffect {
        source: granter,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: None, to: Some(ZoneKind::Battlefield), affected: EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            payload: ReplacementPayload::EntersWithCounters { kind: CounterKind::PlusOnePlusOne, amount: 1 },
        },
        consumption: ConsumptionPolicy::LimitedUses(1),
    });

    // The payload only proposes a CounterAdded event and does not mutate counters, so a direct call asserts on the returned event.
    let entrant_a = ObjectId(1);
    state.objects.insert(entrant_a, card_object(entrant_a, creature_definition("A", ""), p0, ZoneKind::Battlefield));
    let runtime = ScriptRuntime::new();
    let events_a = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant_a, ZoneKind::Hand, ColorSet::empty());
    assert_eq!(events_a.len(), 1, "first entry should be granted the counter, got {events_a:?}");
    assert!(matches!(
        events_a[0],
        GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount: 1 } if object == entrant_a
    ));
    assert!(state.replacement_effects.is_empty(), "LimitedUses(1) must be fully consumed after its one use");

    let entrant_b = ObjectId(2);
    state.objects.insert(entrant_b, card_object(entrant_b, creature_definition("B", ""), p0, ZoneKind::Battlefield));
    let events_b = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant_b, ZoneKind::Hand, ColorSet::empty());
    assert!(events_b.is_empty(), "the shield is gone - a second entry gets nothing");
}

#[test]
fn repeatable_enters_tapped_applies_to_every_entry() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let granter = ObjectId(0);
    state.objects.insert(granter, card_object(granter, creature_definition("Granter", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(ReplacementEffect {
        source: granter,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: None, to: Some(ZoneKind::Battlefield), affected: EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            payload: ReplacementPayload::EntersTapped,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    let runtime = ScriptRuntime::new();
    for i in 1..=2 {
        let entrant = ObjectId(i);
        state.objects.insert(entrant, card_object(entrant, creature_definition("Entrant", ""), p0, ZoneKind::Battlefield));
        replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Hand, ColorSet::empty());
        assert!(state.object(entrant).unwrap().tapped, "entry {i} should have been tapped by the Repeatable granter");
    }
    assert_eq!(state.replacement_effects.len(), 1, "a Repeatable effect is never removed");
}
