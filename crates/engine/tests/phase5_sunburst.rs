//! Phase 5 vertical slice: CR 702.44 Sunburst - "this object enters with a
//! +1/+1 counter (as a creature) or charge counter (otherwise) on it for
//! each color of mana spent to cast it." Narrowed out of the broader
//! "dynamic ETB counts" backlog item alongside Bloodthirst
//! (`phase5_bloodthirst.rs`) and ahead of Devour/Amplify (still deferred -
//! both need a genuine mid-ETB player choice, a new pause/resume
//! `ProcessState`, unlike Sunburst's fully deterministic amount).
//!
//! Unlike every other `EntersWithCounters*` payload, `EntersWithCountersSunburst`
//! carries no fields at all - both `kind` and `amount` are derived at apply
//! time from `colors_spent`, a new parameter threaded onto
//! `replacement::apply_enters_battlefield_modifiers` itself. The one real
//! producer of a nonempty `colors_spent` is `ResolveStackTop`'s own
//! resolve-a-spell-off-the-stack path, which reads `StackObject::costs_paid`
//! (already recorded at payment time, CR 118.13a) via the new
//! `cost::colors_spent` helper - `PlayLand`/`Search`'s battlefield-entry
//! paths always pass `ColorSet::empty()`, which is exactly correct per CR
//! 702.44b (no cast, no colors spent, no counters), not a placeholder.

use stackfish_engine::engine::Engine;
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::types::cost::colors_spent;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

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

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn sunburst_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::EntersWithCountersSunburst,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

// ---------------------------------------------------------------------
// cost::colors_spent - pure helper, unit level.
// ---------------------------------------------------------------------

#[test]
fn colors_spent_dedups_repeated_colors_across_payments() {
    let payments = vec![
        CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Green), ManaUnit::Colored(Color::Green)], phyrexian_life: 0 },
        CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Blue), ManaUnit::Colorless], phyrexian_life: 0 },
    ];
    assert_eq!(colors_spent(&payments), ColorSet::GREEN | ColorSet::BLUE);
}

#[test]
fn colors_spent_counts_snow_colored_the_same_as_plain() {
    let payments = vec![CostPayment::Mana { units: vec![ManaUnit::SnowColored(Color::White)], phyrexian_life: 0 }];
    assert_eq!(colors_spent(&payments), ColorSet::WHITE);
}

#[test]
fn colors_spent_ignores_colorless_and_non_mana_payments() {
    let payments = vec![
        CostPayment::Mana { units: vec![ManaUnit::Colorless, ManaUnit::SnowColorless], phyrexian_life: 0 },
        CostPayment::Sacrifice(ObjectId(1)),
    ];
    assert_eq!(colors_spent(&payments), ColorSet::empty());
}

// ---------------------------------------------------------------------
// apply_enters_battlefield_modifiers - unit level, direct driver-function
// calls, same convention phase5_bloodthirst.rs's own unit tests use.
// ---------------------------------------------------------------------

#[test]
fn creature_entrant_gets_a_plus_one_counter_per_distinct_color_spent() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Sunny", ManaCost { symbols: Vec::new() }, ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(sunburst_effect(entrant));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(
        &mut state,
        &runtime,
        entrant,
        ZoneKind::Stack,
        ColorSet::GREEN | ColorSet::BLUE,
    );

    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount: 2 } if object == entrant
    ));
}

#[test]
fn noncreature_entrant_gets_charge_counters_instead() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, artifact_definition("Prism", ManaCost { symbols: Vec::new() }, ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(sunburst_effect(entrant));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(
        &mut state,
        &runtime,
        entrant,
        ZoneKind::Stack,
        ColorSet::WHITE | ColorSet::BLUE | ColorSet::BLACK,
    );

    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        GameEventKind::CounterAdded { object, kind: CounterKind::Charge, amount: 3 } if object == entrant
    ));
}

#[test]
fn no_colored_mana_spent_grants_no_counters() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Sunny", ManaCost { symbols: Vec::new() }, ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(sunburst_effect(entrant));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Stack, ColorSet::empty());

    assert!(events.is_empty(), "CR 702.44b: no colored mana spent - no counters, got {events:?}");
}

#[test]
fn non_cast_entry_paths_always_pass_an_empty_color_set() {
    // PlayLand/Search never have a cast to spend colors on - simulated here
    // by calling the driver with ZoneKind::Hand (PlayLand's own `from`) and
    // an empty ColorSet, same as those real call sites always pass.
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Sunny", ManaCost { symbols: Vec::new() }, ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(sunburst_effect(entrant));

    let runtime = ScriptRuntime::new();
    let events = replacement::apply_enters_battlefield_modifiers(&mut state, &runtime, entrant, ZoneKind::Hand, ColorSet::empty());

    assert!(events.is_empty());
}

// ---------------------------------------------------------------------
// End to end: Lua self-registration + CastSpell resolution with a real
// colored mana payment.
// ---------------------------------------------------------------------

const SUNBURST: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersSunburst)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

#[test]
fn set_payload_sunburst_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition("Sunny", ManaCost { symbols: Vec::new() }, SUNBURST);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { to: Some(ZoneKind::Battlefield), .. },
            payload: ReplacementPayload::EntersWithCountersSunburst,
        } => {}
        other => panic!("expected a WouldMoveZone(_, Battlefield)/EntersWithCountersSunburst spec, got {other:?}"),
    }
}

fn state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let mut state = two_player_state();
    state.players[0].mana_pool = pool;
    let _ = p0;
    state
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

#[test]
fn cast_creature_resolution_gets_one_plus_one_counter_per_color_paid() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Blue)),
        ],
    };
    let card = ObjectId(0);
    let mut state = state_with_pool(ManaPool { green: 1, blue: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, creature_definition("Sunny", mana_cost, SUNBURST), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colored(Color::Green), ManaUnit::Colored(Color::Blue)],
                phyrexian_life: 0,
            }],
        })
        .expect("pool covers {G}{U} exactly");
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let (resolved_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Battlefield));
    assert_eq!(
        engine.state().object(resolved_id).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        2,
        "2 distinct colors (G, U) were spent to cast this creature"
    );
}

#[test]
fn cast_noncreature_resolution_gets_charge_counters_per_color_paid() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Black)),
        ],
    };
    let card = ObjectId(0);
    let mut state = state_with_pool(ManaPool { white: 1, black: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, artifact_definition("Prism", mana_cost, SUNBURST), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colored(Color::White), ManaUnit::Colored(Color::Black)],
                phyrexian_life: 0,
            }],
        })
        .expect("pool covers {W}{B} exactly");
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let (resolved_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Battlefield));
    assert_eq!(
        engine.state().object(resolved_id).unwrap().counters.count(&CounterKind::Charge),
        2,
        "not a creature - charge counters instead of +1/+1, one per distinct color paid"
    );
}
