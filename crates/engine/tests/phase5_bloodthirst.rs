//! Phase 5 vertical slice: CR 702.54a Bloodthirst N - "this permanent enters
//! with N +1/+1 counters if an opponent was dealt damage this turn."
//! Narrowed out of the broader "dynamic ETB counts" backlog item to just
//! this keyword: unlike Sunburst (needs the cast payment's chosen mana
//! colors threaded to the ETB step - no such plumbing exists) or Devour/
//! Amplify (need a genuine mid-ETB player choice, and so a new pause/resume
//! `ProcessState`), Bloodthirst's amount is a fixed printed N gated by a
//! pure state query - answerable directly from `state.ledger`, no player
//! interaction and no new plumbing needed.
//!
//! `ReplacementPayload::EntersWithCountersBloodthirst { kind, amount }` -
//! same "propose a CounterAdded event, don't mutate directly" shape
//! `EntersWithCounters` already established, applied by the same
//! `replacement::apply_enters_battlefield_modifiers` sibling driver. The
//! condition itself is `replacement::opponent_damaged_this_turn`: scans
//! `state.ledger` back to the most recent `GameEventKind::TurnStarted`, then
//! checks for a `DamageDealt` whose target is a player other than the
//! entering permanent's own controller.
//!
//! CR 702.54b's "Bloodthirst X" variant (amount = total damage dealt to
//! opponents this turn, not a fixed gate) reuses the identical scan and is
//! a named, cheap follow-up - not built this slice.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
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

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

/// Directly appends a ledger event, same allocate-then-push shape
/// `commands::execute_command`'s own committed-event construction uses -
/// there's no other way to seed ledger history for a fixture, since
/// `GameEvent::new` itself is `todo!()`.
fn push_event(state: &mut GameState, kind: GameEventKind) {
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    state.ledger.push(GameEvent { event_id, sequence_no, kind, caused_by_command: None, parent_event: None });
}

fn bloodthirst_effect(source: ObjectId, amount: i32) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter { controller: None, card_types: None, self_only: true },
            },
            payload: ReplacementPayload::EntersWithCountersBloodthirst { kind: CounterKind::PlusOnePlusOne, amount },
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

// ---------------------------------------------------------------------
// opponent_damaged_this_turn / apply_enters_battlefield_modifiers - unit
// level, direct driver-function calls (same convention this file's sibling,
// phase5_etb_replacement_modifiers.rs, uses for its own consumption-policy
// tests).
// ---------------------------------------------------------------------

#[test]
fn bloodthirst_grants_counters_when_an_opponent_was_damaged_this_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    push_event(&mut state, GameEventKind::TurnStarted { player: p0, turn_number: 1 });
    push_event(&mut state, GameEventKind::DamageDealt { source: ObjectId(99), target: Target::Player(p1), amount: 3, is_combat_damage: false });

    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Bloodhound", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(bloodthirst_effect(entrant, 2));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Hand, ColorSet::empty());

    assert_eq!(events.len(), 1, "an opponent (p1) was damaged this turn - the counters should be granted, got {events:?}");
    assert!(matches!(
        events[0],
        GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount: 2 } if object == entrant
    ));
}

#[test]
fn bloodthirst_grants_nothing_when_no_opponent_was_damaged_this_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    push_event(&mut state, GameEventKind::TurnStarted { player: p0, turn_number: 1 });
    // No DamageDealt at all this turn.

    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Bloodhound", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(bloodthirst_effect(entrant, 2));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Hand, ColorSet::empty());

    assert!(events.is_empty(), "no damage was dealt this turn - Bloodthirst's condition is unmet, got {events:?}");
}

#[test]
fn bloodthirst_ignores_damage_dealt_to_its_own_controller() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    push_event(&mut state, GameEventKind::TurnStarted { player: p0, turn_number: 1 });
    // p0 (the entering permanent's own controller) took damage, not an
    // opponent - CR 702.54a only cares about opponents.
    push_event(&mut state, GameEventKind::DamageDealt { source: ObjectId(99), target: Target::Player(p0), amount: 5, is_combat_damage: false });

    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Bloodhound", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(bloodthirst_effect(entrant, 2));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Hand, ColorSet::empty());

    assert!(events.is_empty(), "only the controller was damaged, not an opponent - condition unmet, got {events:?}");
}

#[test]
fn bloodthirst_condition_resets_across_a_turn_boundary() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // Damage dealt on a prior turn, followed by a fresh TurnStarted - the
    // scan must not look past that boundary.
    push_event(&mut state, GameEventKind::DamageDealt { source: ObjectId(99), target: Target::Player(p1), amount: 3, is_combat_damage: false });
    push_event(&mut state, GameEventKind::TurnStarted { player: p0, turn_number: 2 });

    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Bloodhound", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(bloodthirst_effect(entrant, 2));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Hand, ColorSet::empty());

    assert!(events.is_empty(), "the damage happened last turn, not this one - condition unmet, got {events:?}");
}

// ---------------------------------------------------------------------
// End to end: Lua self-registration + CastSpell resolution.
// ---------------------------------------------------------------------

const BLOODTHIRST_TWO: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersBloodthirst("PlusOnePlusOne", 2))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

#[test]
fn set_payload_bloodthirst_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition("Bloodhound", BLOODTHIRST_TWO);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { to: Some(ZoneKind::Battlefield), .. },
            payload: ReplacementPayload::EntersWithCountersBloodthirst { kind: CounterKind::PlusOnePlusOne, amount: 2 },
        } => {}
        other => panic!("expected a WouldMoveZone(_, Battlefield)/EntersWithCountersBloodthirst(PlusOnePlusOne, 2) spec, got {other:?}"),
    }
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
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
fn cast_creature_resolution_gets_bloodthirst_counters_after_real_combat_damage() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // A real combat-damage event, exactly as ResolveStackTop/AssignCombatDamage
    // would log it, dealt to p1 (p0's opponent) earlier this same turn.
    push_event(&mut state, GameEventKind::TurnStarted { player: p0, turn_number: 1 });
    push_event(&mut state, GameEventKind::DamageDealt { source: ObjectId(50), target: Target::Player(p1), amount: 2, is_combat_damage: true });

    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, creature_definition("Bloodhound", BLOODTHIRST_TWO), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .unwrap();
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let (resolved_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Battlefield));
    assert_eq!(
        engine.state().object(resolved_id).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        2,
        "an opponent was dealt real combat damage this turn - Bloodthirst should have applied"
    );
}
