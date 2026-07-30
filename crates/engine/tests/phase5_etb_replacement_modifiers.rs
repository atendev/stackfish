//! Phase 5 vertical slice: CR 603.6d/614.1c "how a permanent enters the
//! battlefield" replacement effects - `ReplacementPayload::{EntersTapped,
//! EntersWithCounters}`, paired with the already-real `EventFilter::
//! WouldMoveZone(_, Battlefield)` shape. Covers both a card's own
//! self-registered ability (CR 113.6h: functions before it's a real
//! battlefield permanent) and an externally granted one (e.g. "lands your
//! opponents control enter tapped"), applied by `replacement::
//! apply_enters_battlefield_modifiers` - a narrow sibling of
//! `apply_replacements` that mutates the just-created `GameObject` directly
//! rather than rewriting the proposed `ZoneChange` event (see that
//! function's own doc comment for why). Wired into the three real
//! battlefield-entry producers: `PlayLand`, `CastSpell` resolution
//! (`move_resolving_spell`), and `submit_search_choice`'s
//! `SearchDestination::Battlefield` arm.

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

/// Blood Moon-style external granter: "lands your opponents control enter
/// tapped" - `controller = "opponent"` is relative to *this* source's own
/// controller, checked via `event_target_matches` the same way every other
/// `:SetAffected` filter already is.
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

/// `phase5/effect-filter-subtypes`: same shape as `OPPONENTS_LANDS_ENTER_TAPPED`
/// above, but narrowed by subtype instead of card type - proves
/// `ReplacementEffectBuilder::SetAffected` (which now delegates to the shared
/// `decode_effect_filter_table`, see that function's own doc comment) reads
/// `subtypes` correctly, not just `card_types`.
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

/// CR 603.6d/614.1c only mean something for a move landing on the
/// battlefield - `to = "Graveyard"` must be rejected at `:Register()` time
/// (`validate_replacement_filter_payload_pairing`), not silently accepted as
/// an effect that can never match anything.
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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn player_state(id: PlayerId) -> PlayerState {
    PlayerState {
        player_id: id,
        life: 20,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

/// Every fixture card in this file has an empty `mana_cost` - pays it via the
/// explicit empty `PaymentPlan` CR 118.5a still requires, same helper
/// `phase5_cast_spell.rs` already uses.
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

// ---------------------------------------------------------------------
// Lua registration mechanics.
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// End to end: PlayLand.
// ---------------------------------------------------------------------

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
    // ByKey - no script runs, same "plain unscripted card" convention
    // phase5_replacement_effect_shapes.rs's own vanilla_creature_definition
    // uses.
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
    // The granter (p1's own permanent) is already on the battlefield before
    // p0 plays their land - same "already registered ahead of the event it
    // watches" shape phase5_replacement_effect_shapes.rs's own combat-damage
    // shield tests use.
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

// ---------------------------------------------------------------------
// End to end: CastSpell resolution (move_resolving_spell).
// ---------------------------------------------------------------------

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
                alternative_cost: None,
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

// ---------------------------------------------------------------------
// Consumption-policy bookkeeping (unit-level, direct driver-function call).
// ---------------------------------------------------------------------

#[test]
fn limited_uses_enters_with_counters_is_consumed_after_first_entry() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let granter = ObjectId(0);
    state.objects.insert(granter, card_object(granter, creature_definition("Granter", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(ReplacementEffect {
        source: granter,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: None, to: Some(ZoneKind::Battlefield), affected: EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, } },
            payload: ReplacementPayload::EntersWithCounters { kind: CounterKind::PlusOnePlusOne, amount: 1 },
        },
        consumption: ConsumptionPolicy::LimitedUses(1),
    });

    // `apply_enters_battlefield_modifiers` only *proposes* a `CounterAdded`
    // event for this payload (see its own doc comment) - it doesn't mutate
    // `counters` itself, so a bare direct call like this one (no
    // surrounding `CommitEvents` pipeline) only has the returned event to
    // assert against, not `state.object(..).counters`.
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
            filter: EventFilter::WouldMoveZone { from: None, to: Some(ZoneKind::Battlefield), affected: EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, } },
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
