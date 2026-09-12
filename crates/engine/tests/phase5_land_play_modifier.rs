//! Phase 5 vertical slice: CR 305.2 "you may play an additional land each
//! turn"-style effects (Exploration, Azusa, ...) — the fourth player-level
//! continuous effect in this cluster (`HandSizeModifier`/`HandRevealModifier`/
//! `DrawLimitModifier` are the other three), same "distinct mechanism from
//! the CR 613 layer system" reasoning all three already establish.
//!
//! Deliberately not shaped like `DrawLimitModifier` despite the close family
//! resemblance: CR 305.2's land-play increases genuinely stack (two
//! Explorations means 3 land plays, not 2 — the max of the two), so
//! `effective_land_play_limit` sums every matching modifier's own
//! `additional` rather than taking the latest timestamp — see
//! `LandPlayModifier`'s own doc comment (types/player.rs). Direct Rust-native
//! construction + `PlayLand` enforcement (stacking, "You"/"Opponent" scoping)
//! is covered in `phase5_play_land.rs`; this file covers the Lua-facing
//! `Game.CreateLandPlayModifier` builder round trip specifically.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn ward_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 999,
        name: "Test Exploration".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: Vec::new(),
        oracle_text: "You may play an additional land on each of your turns.".to_string(),
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

const ONE_ADDITIONAL_LAND: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateLandPlayModifier(c)
    m:SetAffected(Game.LandPlayAffected.You)
    m:SetAdditional(1)
    m:Register()
end
return s
"#;

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

#[test]
fn run_card_script_self_registers_a_land_play_modifier() {
    let mut state = two_player_state();
    let source = ObjectId(999);
    let definition = ward_definition(ONE_ADDITIONAL_LAND);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(1), ZoneKind::Battlefield));

    let mut runtime = stackfish_engine::script::ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.land_play_modifiers.len(), 1);
    let modifier = &state.land_play_modifiers[0];
    assert_eq!(modifier.source, source);
    assert_eq!(modifier.controller, PlayerId(1), "controller resolved from the source's own live controller at drain time");
    assert!(matches!(modifier.affected, LandPlayAffected::Filter(FilterController::You)));
    assert_eq!(modifier.additional, 1);
}

#[test]
fn a_land_play_modifier_from_a_real_script_lets_a_second_land_through() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let source = ObjectId(999);
    let definition = ward_definition(ONE_ADDITIONAL_LAND);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(source, card_object(source, definition.clone(), p0, ZoneKind::Battlefield));

    let mut runtime = stackfish_engine::script::ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    let land_definition = |id: u64, name: &str| CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    let first = ObjectId(0);
    let second = ObjectId(1);
    state.objects.insert(first, card_object(first, land_definition(1, "Forest"), p0, ZoneKind::Hand));
    state.objects.insert(second, card_object(second, land_definition(2, "Island"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(first);
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(second);

    let mut engine = Engine::new(state);
    for card in [first, second] {
        engine
            .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
            .unwrap_or_else(|e| panic!("expected {card:?} to be playable: {e:?}"));
        stackfish_testutil::drain(&mut engine);
    }

    assert_eq!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.len(),
        3,
        "the source enchantment plus both lands should be on the battlefield"
    );
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty());
}

/// "Test Destroy Permanent: Destroy target permanent." — used only to force
/// a real `commands::retire_object` call on the modifier's own source.
fn destroy_permanent_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Destroy Permanent".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy target permanent.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_destroy_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: None, subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// CR 611.3b: a static ability's continuous effect only applies while its
/// source remains on the battlefield — `phase5/modifier-source-cleanup`.
#[test]
fn a_land_play_modifier_is_purged_once_its_source_leaves_the_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    state.objects.insert(source, card_object(source, ward_definition(""), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.land_play_modifiers.push(LandPlayModifier {
        source,
        controller: p0,
        affected: LandPlayAffected::Filter(FilterController::You),
        additional: 1,
    });
    let removal = ObjectId(1);
    state.objects.insert(removal, card_object(removal, destroy_permanent_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(removal);

    assert_eq!(effective_land_play_limit(&state.land_play_modifiers, p0), 2, "the modifier is live before its source is destroyed");

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: removal, modes: Vec::new(), targets: vec![Target::Object(source)], x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority, spell is in hand");
    let status = stackfish_testutil::drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    stackfish_testutil::drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    stackfish_testutil::drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    stackfish_testutil::drain(&mut engine);

    assert!(engine.state().object(source).is_none(), "the source enchantment was destroyed");
    assert!(engine.state().land_play_modifiers.is_empty(), "the modifier must be purged along with its source");
    assert_eq!(effective_land_play_limit(&engine.state().land_play_modifiers, p0), 1, "back to the base one-land-per-turn limit");
}
