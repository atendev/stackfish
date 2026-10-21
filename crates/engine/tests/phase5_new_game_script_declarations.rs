//! `Engine::new_game` loads what a card's script declares for a plain spell:
//! `s.operation()` (its resolution effect) and `s.target_requirement()`. The
//! host builds every card through `new_game`, and nothing else copied these
//! two values onto the `CardDefinition`, so a script-only spell such as Shock
//! or Peek was cast with no target and resolved doing nothing. The proof-game
//! atomicity work found it: no scry, search, mode or target Decision ever
//! opened in 160 games.

use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::minimal_card_definition;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scripted(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, name, name, ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
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

fn game(cards: Vec<CardDefinition>) -> Engine {
    let player = |id: PlayerId| PlayerConfig { player_id: id, library: cards.clone(), commanders: Vec::new(), life_override: None };
    Engine::new_game(GameConfig {
        players: vec![player(P0), player(P1)],
        turn_order: vec![P0, P1],
        settings: GameSettings::default(),
        rng_seed: 1,
    })
}

fn definitions(engine: &Engine, name: &str) -> Vec<CardDefinition> {
    let mut found: Vec<_> =
        engine.state().objects.values().filter(|o| o.definition.name == name).map(|o| o.definition.clone()).collect();
    found.sort_by_key(|d| d.definition_id);
    found
}

#[test]
fn a_spells_script_operation_and_target_requirement_are_loaded() {
    let engine = game(vec![scripted("Shock", SHOCK), scripted("Peek", PEEK), scripted("Vanilla", VANILLA)]);

    let shock = definitions(&engine, "Shock");
    assert_eq!(shock.len(), 2, "one per player");
    for card in &shock {
        assert_eq!(card.effect, Some(AbilityEffect::Damage(2)));
        assert!(matches!(
            card.target_requirement,
            Some(TargetSpec::Primitive(TargetRequirement { filter: TargetFilter::AnyTarget, count: 1, .. }))
        ));
    }
    for card in definitions(&engine, "Peek") {
        assert_eq!(card.effect, Some(AbilityEffect::Scry(2)));
        assert!(card.target_requirement.is_none());
    }
    for card in definitions(&engine, "Vanilla") {
        assert!(card.effect.is_none() && card.target_requirement.is_none(), "a script that declares neither adds neither");
    }
}

#[test]
fn a_value_set_in_rust_wins_over_the_script() {
    let mut card = scripted("Shock", SHOCK);
    card.effect = Some(AbilityEffect::GainLife(5));
    card.target_requirement = None;
    let engine = game(vec![card]);
    for card in definitions(&engine, "Shock") {
        assert_eq!(card.effect, Some(AbilityEffect::GainLife(5)), "an explicit effect is kept");
        assert!(card.target_requirement.is_some(), "the unset target requirement is still filled in");
    }
}

#[test]
fn a_broken_script_logs_a_script_error_and_the_game_still_builds() {
    let engine = game(vec![scripted("Broken", "this is not lua (")]);
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::ScriptError { .. })),
        "a script that fails to load is logged"
    );
    for card in definitions(&engine, "Broken") {
        assert!(card.effect.is_none() && card.target_requirement.is_none());
    }
}
