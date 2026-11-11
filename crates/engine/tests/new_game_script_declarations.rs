//! `Engine::new_game` copies a spell's `s.operation()`, `s.target_requirement()`, `s.alternative_costs()` and `s.buyback_cost()` onto its `CardDefinition`, so a script-only spell such as Shock casts with its target and resolves.

use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::minimal_card_definition;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scripted(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            1,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

const SHOCK: &str = r#"
local s = {}
function s.target_requirement()
    return { type = "any", count = 1 }
end
function s.operation()
    return Game.Ops.Damage(2)
end
return s
"#;

const PEEK: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.Scry(2)
end
return s
"#;

const VANILLA: &str = "local s = {} return s";

const RECOUP: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.Draw(1)
end
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
function s.buyback_cost()
    return "{3}"
end
return s
"#;

const BAD_BUYBACK: &str = r#"
local s = {}
function s.buyback_cost()
    return "not a cost"
end
return s
"#;

fn script_costs(source: &str) -> (Vec<AlternativeCost>, Option<ManaCost>) {
    let runtime = stackfish_engine::script::ScriptRuntime::new();
    (
        runtime
            .build_alternative_costs(source)
            .expect("script loads"),
        runtime.build_buyback_cost(source).expect("script loads"),
    )
}

fn script_errors(engine: &Engine) -> usize {
    engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(e.kind, GameEventKind::ScriptError { .. }))
        .count()
}

fn game(cards: Vec<CardDefinition>) -> Engine {
    let player = |id: PlayerId| PlayerConfig {
        player_id: id,
        library: cards.clone(),
        commanders: Vec::new(),
        life_override: None,
    };
    Engine::new_game(GameConfig {
        players: vec![player(P0), player(P1)],
        turn_order: vec![P0, P1],
        settings: GameSettings::default(),
        rng_seed: 1,
    })
}

fn definitions(engine: &Engine, name: &str) -> Vec<CardDefinition> {
    let mut found: Vec<_> = engine
        .state()
        .objects
        .values()
        .filter(|o| o.definition.name == name)
        .map(|o| o.definition.clone())
        .collect();
    found.sort_by_key(|d| d.definition_id);
    found
}

#[test]
fn a_spells_script_operation_and_target_requirement_are_loaded() {
    let engine = game(vec![
        scripted("Shock", SHOCK),
        scripted("Peek", PEEK),
        scripted("Vanilla", VANILLA),
    ]);

    let shock = definitions(&engine, "Shock");
    assert_eq!(shock.len(), 2, "one per player");
    for card in &shock {
        assert_eq!(card.effect, Some(AbilityEffect::Damage(2)));
        assert!(matches!(
            card.target_requirement,
            Some(TargetSpec::Primitive(TargetRequirement {
                filter: TargetFilter::AnyTarget,
                count: 1,
                ..
            }))
        ));
    }
    for card in definitions(&engine, "Peek") {
        assert_eq!(card.effect, Some(AbilityEffect::Scry(2)));
        assert!(card.target_requirement.is_none());
    }
    for card in definitions(&engine, "Vanilla") {
        assert!(
            card.effect.is_none() && card.target_requirement.is_none(),
            "a script that declares neither adds neither"
        );
    }
}

#[test]
fn a_value_set_in_rust_wins_over_the_script() {
    let mut card = scripted("Shock", SHOCK);
    card.effect = Some(AbilityEffect::GainLife(5));
    card.target_requirement = None;
    let engine = game(vec![card]);
    for card in definitions(&engine, "Shock") {
        assert_eq!(
            card.effect,
            Some(AbilityEffect::GainLife(5)),
            "an explicit effect is kept"
        );
        assert!(
            card.target_requirement.is_some(),
            "the unset target requirement is still filled in"
        );
    }
}

#[test]
fn a_broken_script_logs_a_script_error_and_the_game_still_builds() {
    let engine = game(vec![scripted("Broken", "this is not lua (")]);
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::ScriptError { .. })),
        "a script that fails to load is logged"
    );
    for card in definitions(&engine, "Broken") {
        assert!(card.effect.is_none() && card.target_requirement.is_none());
    }
}

#[test]
fn a_spells_script_alternative_costs_and_buyback_cost_are_loaded() {
    // CR 702.34a
    let (flashback, buyback) = script_costs(RECOUP);
    assert_eq!(flashback.len(), 1);
    assert!(buyback.is_some());
    let engine = game(vec![scripted("Recoup", RECOUP)]);
    let recoup = definitions(&engine, "Recoup");
    assert_eq!(recoup.len(), 2, "one per player");
    for card in recoup {
        assert_eq!(card.alternative_costs, flashback);
        assert_eq!(card.buyback_cost, buyback);
    }
    for card in definitions(&engine, "Shock") {
        assert!(card.alternative_costs.is_empty() && card.buyback_cost.is_none());
    }
}

#[test]
fn rust_set_costs_win_over_the_script() {
    // CR 601.2b
    let (flashback, buyback) = script_costs(RECOUP);
    let rust_cost = AlternativeCost::Replacement {
        description: "Rust".to_string(),
        cost: CostSpec::mana_only(ManaCost {
            symbols: Vec::new(),
        }),
        from_zone: ZoneKind::Hand,
        exile_on_leaving_stack: false,
        escape: false,
    };
    let mut keeps_alternative = scripted("Recoup", RECOUP);
    keeps_alternative.alternative_costs = vec![rust_cost.clone()];
    let mut keeps_buyback = scripted("Recoup B", RECOUP);
    keeps_buyback.buyback_cost = Some(ManaCost {
        symbols: Vec::new(),
    });
    let engine = game(vec![keeps_alternative, keeps_buyback]);
    for card in definitions(&engine, "Recoup") {
        assert_eq!(card.alternative_costs, vec![rust_cost.clone()]);
        assert_eq!(card.buyback_cost, buyback, "the unset buyback is filled in");
    }
    for card in definitions(&engine, "Recoup B") {
        assert_eq!(
            card.alternative_costs, flashback,
            "the unset costs are filled in"
        );
        assert_eq!(
            card.buyback_cost,
            Some(ManaCost {
                symbols: Vec::new()
            })
        );
    }
}

#[test]
fn a_card_with_rust_effect_and_target_still_gets_its_script_costs() {
    // CR 702.27a
    let (flashback, buyback) = script_costs(RECOUP);
    let mut card = scripted("Recoup", RECOUP);
    card.effect = Some(AbilityEffect::GainLife(5));
    card.target_requirement = Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }));
    let engine = game(vec![card]);
    for card in definitions(&engine, "Recoup") {
        assert_eq!(card.effect, Some(AbilityEffect::GainLife(5)));
        assert_eq!(card.alternative_costs, flashback);
        assert_eq!(card.buyback_cost, buyback);
    }
}

#[test]
fn a_malformed_buyback_cost_logs_once_per_copy_and_loads_none() {
    // CR 702.27a
    let engine = game(vec![scripted("Bad Buyback", BAD_BUYBACK)]);
    assert_eq!(script_errors(&engine), 2, "a failed script is not cached");
    for card in definitions(&engine, "Bad Buyback") {
        assert!(card.buyback_cost.is_none());
    }
}
