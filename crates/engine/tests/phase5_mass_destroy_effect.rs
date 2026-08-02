//! Phase 5 vertical slice: CR 701.8/601.2c mass destruction - "destroy all
//! X," which doesn't target at all (unlike `AbilityEffect::Destroy`, which
//! only ever destroys the resolving spell/ability's own already-chosen
//! target). `AbilityEffect::DestroyAll(EffectFilter)` reuses the exact
//! `EffectFilter` shape (`controller`/`card_types`/`self_only`, plus
//! `subtypes`/`excluded_subtypes` - `phase5/effect-filter-subtypes`, CR
//! 205.4b - "destroy all Zombies" - and `supertypes`/`excluded_supertypes` -
//! `phase5/effect-filter-supertypes`, CR 205.4a - "destroy all legendary
//! permanents") `Game.CreateStaticAbility`'s own
//! `:SetAffected` already uses, and the
//! same `continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, ...)`
//! pipeline the single-target `Destroy` effect already delegates to - CR
//! 702.12b indestructibility and the CR 614.13 `WouldDie` replacement check
//! both apply for free, no new destruction logic.
//!
//! Combined with the previous slice's `s.operation()`, Armageddon becomes a
//! real, fully self-contained script:
//!
//! ```lua
//! function s.operation()
//!     return Game.Ops.DestroyAll({ card_types = { "Land" } })
//! end
//! ```

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

// ---------------------------------------------------------------------
// Decode-level: Game.Ops.DestroyAll
// ---------------------------------------------------------------------

#[test]
fn destroy_all_decodes_a_card_types_and_controller_filter() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.DestroyAll({ card_types = { "Land" }, controller = "you" })
        end
        return s
    "#;
    let (effect, _handle) =
        runtime.build_operation(source).expect("script should load").expect("declares an operation");
    match effect {
        Some(AbilityEffect::DestroyAll(filter)) => {
            assert_eq!(filter.card_types, Some(CardTypeSet::LAND));
            assert_eq!(filter.controller, Some(FilterController::You));
            assert!(!filter.self_only);
        }
        other => panic!("expected DestroyAll, got {other:?}"),
    }
}

#[test]
fn destroy_all_decodes_a_subtypes_and_excluded_subtypes_filter() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.DestroyAll({ subtypes = { "Zombie" }, excluded_subtypes = { "Wall" } })
        end
        return s
    "#;
    let (effect, _handle) =
        runtime.build_operation(source).expect("script should load").expect("declares an operation");
    match effect {
        Some(AbilityEffect::DestroyAll(filter)) => {
            assert_eq!(filter.subtypes, Some(vec![Subtype("Zombie".to_string())]));
            assert_eq!(filter.excluded_subtypes, Some(vec![Subtype("Wall".to_string())]));
        }
        other => panic!("expected DestroyAll, got {other:?}"),
    }
}

#[test]
fn destroy_all_decodes_a_supertypes_and_excluded_supertypes_filter() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.DestroyAll({ supertypes = { "Legendary" }, excluded_supertypes = { "Snow" } })
        end
        return s
    "#;
    let (effect, _handle) =
        runtime.build_operation(source).expect("script should load").expect("declares an operation");
    match effect {
        Some(AbilityEffect::DestroyAll(filter)) => {
            assert_eq!(filter.supertypes, Some(SupertypeSet::LEGENDARY));
            assert_eq!(filter.excluded_supertypes, Some(SupertypeSet::SNOW));
        }
        other => panic!("expected DestroyAll, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// End-to-end: a script-declared Armageddon
// ---------------------------------------------------------------------

const DESTROY_ALL_LANDS_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DestroyAll({ card_types = { "Land" } })
end
return s
"#;

const DESTROY_ALL_LANDS_YOU_CONTROL_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DestroyAll({ card_types = { "Land" }, controller = "you" })
end
return s
"#;

const DESTROY_ALL_ZOMBIES_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DestroyAll({ subtypes = { "Zombie" } })
end
return s
"#;

const DESTROY_ALL_LEGENDARY_PERMANENTS_SCRIPT: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.DestroyAll({ supertypes = { "Legendary" } })
end
return s
"#;

fn armageddon_definition(definition_id: u64, source: &str) -> CardDefinition {
    let runtime = ScriptRuntime::new();
    let (effect, handle) = runtime
        .build_operation(source)
        .expect("script should load")
        .expect("script declares an operation");
    let effect = effect.or(Some(AbilityEffect::Script(handle)));
    CardDefinition {
        definition_id,
        name: "Test Armageddon".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(source.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land_definition(definition_id: u64, name: &str, indestructible: bool) -> CardDefinition {
    CardDefinition {
        definition_id,
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
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: if indestructible { KeywordSet::INDESTRUCTIBLE } else { KeywordSet::empty() },
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_definition(definition_id: u64) -> CardDefinition {
    subtyped_creature_definition(definition_id, "Test Creature", Vec::new())
}

fn subtyped_creature_definition(definition_id: u64, name: &str, subtypes: Vec<Subtype>) -> CardDefinition {
    supertyped_creature_definition(definition_id, name, subtypes, SupertypeSet::empty())
}

fn supertyped_creature_definition(
    definition_id: u64,
    name: &str,
    subtypes: Vec<Subtype>,
    supertypes: SupertypeSet,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes,
        card_types: CardTypeSet::CREATURE,
        subtypes,
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn put_on_battlefield(state: &mut GameState, id: ObjectId, definition: CardDefinition, owner: PlayerId) {
    state.objects.insert(id, card_object(id, definition, owner, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
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

fn is_on_battlefield(engine: &Engine, name: &str) -> bool {
    engine.state().objects.values().any(|o| o.zone == ZoneKind::Battlefield && o.definition.name == name)
}

#[test]
fn destroy_all_lands_kills_every_land_but_spares_indestructible_ones_and_non_lands() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    put_on_battlefield(&mut state, ObjectId(0), land_definition(1, "Test Land A", false), p0);
    put_on_battlefield(&mut state, ObjectId(1), land_definition(2, "Test Indestructible Land", true), p0);
    put_on_battlefield(&mut state, ObjectId(2), creature_definition(3), p0);
    put_on_battlefield(&mut state, ObjectId(3), land_definition(4, "Test Land C", false), p1);
    let card = ObjectId(4);
    state.objects.insert(
        card,
        card_object(card, armageddon_definition(5, DESTROY_ALL_LANDS_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!is_on_battlefield(&engine, "Test Land A"), "a plain land should have been destroyed");
    assert!(!is_on_battlefield(&engine, "Test Land C"), "an opponent's land should have been destroyed too - no controller filter here");
    assert!(is_on_battlefield(&engine, "Test Indestructible Land"), "CR 702.12b: indestructible should survive");
    assert!(is_on_battlefield(&engine, "Test Creature"), "a non-land should be untouched");
}

#[test]
fn destroy_all_lands_you_control_only_kills_the_casters_own_lands() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    put_on_battlefield(&mut state, ObjectId(0), land_definition(1, "Test Land A", false), p0);
    put_on_battlefield(&mut state, ObjectId(3), land_definition(4, "Test Land C", false), p1);
    let card = ObjectId(4);
    state.objects.insert(
        card,
        card_object(card, armageddon_definition(5, DESTROY_ALL_LANDS_YOU_CONTROL_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!is_on_battlefield(&engine, "Test Land A"), "the caster's own land should have been destroyed");
    assert!(is_on_battlefield(&engine, "Test Land C"), "controller=\"you\" should leave the opponent's land alone");
}

#[test]
fn destroy_all_zombies_only_kills_creatures_with_the_subtype() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    put_on_battlefield(
        &mut state,
        ObjectId(0),
        subtyped_creature_definition(1, "Test Zombie", vec![Subtype("Zombie".to_string())]),
        p0,
    );
    put_on_battlefield(&mut state, ObjectId(1), creature_definition(2), p1);
    let card = ObjectId(4);
    state.objects.insert(
        card,
        card_object(card, armageddon_definition(5, DESTROY_ALL_ZOMBIES_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!is_on_battlefield(&engine, "Test Zombie"), "a Zombie should have been destroyed");
    assert!(is_on_battlefield(&engine, "Test Creature"), "a non-Zombie creature should be untouched");
}

#[test]
fn destroy_all_legendary_permanents_only_kills_the_matching_supertype() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    put_on_battlefield(
        &mut state,
        ObjectId(0),
        supertyped_creature_definition(1, "Test Legend", Vec::new(), SupertypeSet::LEGENDARY),
        p0,
    );
    put_on_battlefield(&mut state, ObjectId(1), creature_definition(2), p1);
    let card = ObjectId(4);
    state.objects.insert(
        card,
        card_object(card, armageddon_definition(5, DESTROY_ALL_LEGENDARY_PERMANENTS_SCRIPT), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!is_on_battlefield(&engine, "Test Legend"), "a legendary permanent should have been destroyed");
    assert!(is_on_battlefield(&engine, "Test Creature"), "a non-legendary creature should be untouched");
}
