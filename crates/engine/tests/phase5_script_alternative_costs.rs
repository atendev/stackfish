//! Phase 5 vertical slice: a card script's own declaration of CR 601.2b
//! alternative costs (flashback/escape-style, `AlternativeCost::Replacement`)
//! and CR 702.27a Buyback (`CardDefinition::buyback_cost`), via two new
//! sibling functions to `s.register_abilities(c)`/`s.target_requirement()`/
//! `s.operation()`: `s.alternative_costs()` and `s.buyback_cost()`.
//!
//! Both engine mechanisms (`AlternativeCost::Replacement`, `buyback_cost`,
//! `PlayerActionKind::CastSpell::alternative_cost`/`pay_buyback`) already
//! work end to end - proven by `phase5_flashback_alternative_costs.rs`,
//! `phase5_escape.rs`, `phase5_buyback.rs`. This slice closes the "no way to
//! declare either from a card script, only direct Rust/TOML construction"
//! gap `docs/lua/not-available-yet.md` named - it adds no new engine
//! mechanism, only a Lua-facing builder for what already exists.
//!
//! `alternative_costs()` returns an *array* of entry tables - a card can
//! grant more than one alternative cost, already fully representable and
//! selectable (`alternative_cost: Option<usize>` is an index into the
//! `Vec`, not a single flag). Each entry's `costs` array reuses
//! `Game.Costs.*` verbatim, decoded the same way
//! `ActivatedAbilityBuilder::AddCost` accumulates a `CostSpec` - no new cost
//! vocabulary. `buyback_cost()` returns a plain mana-cost string, parsed
//! directly via `ManaCost::parse` (the same parser `Game.Costs.Mana(...)`
//! itself calls) rather than routed through `Game.Costs.*`, since
//! `CardDefinition.buyback_cost` is always a flat `ManaCost`.
//!
//! Not attempted (named TODOs, not silently narrowed): the TOML
//! scenario-fixture format still has no field for either
//! (`crates/scenario/src/lib.rs`'s own pre-existing TODOs on
//! `alternative_costs`/`buyback_cost`/`alternative_cost`/`pay_buyback`) - a
//! script-driven scenario fixture would need a
//! `register_script_alternative_costs`/`register_script_buyback_cost`
//! sibling to `register_script_target_requirement`/`register_script_operation`
//! (`crates/scenario/src/lib.rs:1905,1927`), left for its own slice.

use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

// ---------------------------------------------------------------------
// Decode-level: ScriptRuntime::build_alternative_costs / build_buyback_cost
// ---------------------------------------------------------------------

#[test]
fn a_card_with_no_alternative_costs_function_returns_empty() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.register_abilities(c) end
        return s
    "#;
    let costs = runtime.build_alternative_costs(source).expect("script should load");
    assert!(costs.is_empty(), "a card script with no alternative_costs() function grants none");
}

#[test]
fn a_card_with_no_buyback_cost_function_returns_none() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.register_abilities(c) end
        return s
    "#;
    let cost = runtime.build_buyback_cost(source).expect("script should load");
    assert!(cost.is_none(), "a card script with no buyback_cost() function has no buyback");
}

#[test]
fn alternative_costs_function_decodes_a_flashback_shaped_entry() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.alternative_costs()
            return {
                {
                    description = "Flashback {R}",
                    from_zone = "Graveyard",
                    costs = { Game.Costs.Mana("{R}") },
                    exile_on_leaving_stack = true,
                },
            }
        end
        return s
    "#;
    let costs = runtime.build_alternative_costs(source).expect("script should load");
    assert_eq!(costs.len(), 1);
    let AlternativeCost::Replacement { description, cost, from_zone, exile_on_leaving_stack, escape } = &costs[0];
    assert_eq!(description, "Flashback {R}");
    assert_eq!(*from_zone, ZoneKind::Graveyard);
    assert!(*exile_on_leaving_stack);
    assert!(!escape, "escape defaults to false when omitted");
    let mana = cost.mana.as_ref().expect("flashback cost has a mana component");
    assert_eq!(mana.symbols, vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))]);
    assert!(cost.additional.is_empty());
}

#[test]
fn alternative_costs_function_decodes_an_escape_shaped_entry() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.alternative_costs()
            return {
                {
                    description = "Escape {2}{B}, Exile four other cards from your graveyard",
                    from_zone = "Graveyard",
                    costs = { Game.Costs.Mana("{2}{B}"), Game.Costs.ExileFromGraveyard({}, 4) },
                    escape = true,
                },
            }
        end
        return s
    "#;
    let costs = runtime.build_alternative_costs(source).expect("script should load");
    assert_eq!(costs.len(), 1);
    let AlternativeCost::Replacement { cost, escape, exile_on_leaving_stack, .. } = &costs[0];
    assert!(*escape);
    assert!(!exile_on_leaving_stack, "exile_on_leaving_stack defaults to false when omitted");
    assert!(cost.mana.is_some());
    match &cost.additional[..] {
        [AdditionalCost::ExileFromGraveyard { count, .. }] => assert_eq!(*count, 4),
        other => panic!("expected a single ExileFromGraveyard additional cost, got {other:?}"),
    }
}

#[test]
fn alternative_costs_function_can_declare_more_than_one_entry() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.alternative_costs()
            return {
                { description = "Flashback {R}", from_zone = "Graveyard", costs = { Game.Costs.Mana("{R}") } },
                { description = "Escape {2}{B}", from_zone = "Graveyard", costs = { Game.Costs.Mana("{2}{B}") }, escape = true },
            }
        end
        return s
    "#;
    let costs = runtime.build_alternative_costs(source).expect("script should load");
    assert_eq!(costs.len(), 2, "a card can grant more than one alternative cost - selected by index at cast time");
}

#[test]
fn alternative_costs_entry_rejects_two_mana_components() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.alternative_costs()
            return {
                {
                    description = "bogus",
                    from_zone = "Graveyard",
                    costs = { Game.Costs.Mana("{R}"), Game.Costs.Mana("{1}") },
                },
            }
        end
        return s
    "#;
    let err = runtime.build_alternative_costs(source).expect_err("two Mana() components should be rejected");
    let message = format!("{err:?}");
    assert!(message.contains("more than once"), "expected the duplicate-mana error, got {message}");
}

#[test]
fn alternative_costs_entry_missing_costs_is_a_real_error() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.alternative_costs()
            return { { description = "bogus", from_zone = "Graveyard" } }
        end
        return s
    "#;
    let err = runtime.build_alternative_costs(source).expect_err("a missing 'costs' key should error, not default to no cost");
    let message = format!("{err:?}");
    assert!(message.contains("costs"), "expected an error naming the missing 'costs' key, got {message}");
}

#[test]
fn alternative_costs_entry_rejects_an_unknown_zone_name() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.alternative_costs()
            return { { description = "bogus", from_zone = "Sideboard", costs = { Game.Costs.Mana("{R}") } } }
        end
        return s
    "#;
    let err = runtime.build_alternative_costs(source).expect_err("an unrecognized zone name should error");
    let message = format!("{err:?}");
    assert!(message.contains("Sideboard"), "expected an error naming the bad zone, got {message}");
}

#[test]
fn buyback_cost_function_decodes_a_flat_mana_string() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.buyback_cost() return "{2}" end
        return s
    "#;
    let cost = runtime.build_buyback_cost(source).expect("script should load").expect("declares a buyback cost");
    assert_eq!(cost.symbols, vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2))]);
}

#[test]
fn buyback_cost_function_surfaces_a_real_parse_error() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.buyback_cost() return "not a mana cost" end
        return s
    "#;
    runtime.build_buyback_cost(source).expect_err("a malformed mana-cost string must be a real error, not a silent None");
}

// ---------------------------------------------------------------------
// End-to-end: a script-declared alternative cost / buyback cost actually
// drives real casting through Engine, same outcome the direct-construction
// fixtures (phase5_flashback_alternative_costs.rs, phase5_buyback.rs) prove.
// ---------------------------------------------------------------------

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(6)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// `{1}{R}` instant, targets a creature, deals 3 damage - `alternative_costs`
/// starts empty and is patched in from the script below, same
/// `register_script_target_requirement`-style "build first, patch after"
/// pattern `crates/scenario/src/lib.rs:1905` already establishes for the
/// identical "the CardDefinition has to exist before an Engine does, but the
/// script needs an Engine's ScriptRuntime to build against" ordering
/// problem.
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Script Flashback Bolt".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Script Flashback Bolt deals 3 damage to target creature. Flashback {R}."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_script_flashback_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(3)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
    alternative_cost: Option<usize>,
    pay_buyback: bool,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost, pay_buyback },
    })
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
fn a_script_declared_flashback_cost_casts_from_the_graveyard_and_exiles_on_resolution() {
    let script = r#"
        local s = {}
        function s.alternative_costs()
            return {
                {
                    description = "Flashback {R}",
                    from_zone = "Graveyard",
                    costs = { Game.Costs.Mana("{R}") },
                    exile_on_leaving_stack = true,
                },
            }
        end
        return s
    "#;

    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);

    let mut engine = Engine::new(state);
    let alternative_costs = engine.build_alternative_costs(script).expect("script should load");
    assert_eq!(alternative_costs.len(), 1);
    engine.state_mut().objects.get_mut(&card).unwrap().definition.alternative_costs = alternative_costs;

    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], Some(0), false)
        .expect("flashback cast from graveyard with the script-declared alternative announced");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .expect("the script-declared flashback's {R} paid with 1 red");
    drain(&mut engine);

    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Graveyard, ZoneKind::Stack));

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Exile), "CR 702.34a: exiled instead of graveyard, same as direct construction");
}

#[test]
fn a_script_declared_buyback_cost_returns_the_spell_to_hand_on_resolution() {
    let script = r#"
        local s = {}
        function s.buyback_cost() return "{2}" end
        return s
    "#;

    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);

    let mut engine = Engine::new(state);
    let buyback_cost = engine.build_buyback_cost(script).expect("script should load").expect("declares a buyback cost");
    engine.state_mut().objects.get_mut(&card).unwrap().definition.buyback_cost = Some(buyback_cost);

    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], None, true)
        .expect("cast paying the script-declared buyback cost");
    drain(&mut engine);

    // {1}{R} mana cost + {2} buyback = {3}{R} total: 1 red + 3 generic.
    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless, ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{1}{R} mana cost plus the script-declared {2} buyback cost");
    drain(&mut engine);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Hand), "CR 702.27a: bought-back spell returns to hand, same as direct construction");
}
