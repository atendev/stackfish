//! Phase 5 vertical slice: CR 303.4b's "or player" half of Aura attachment -
//! an Aura can enchant a player instead of an object. Closes a real,
//! previously-broken path: casting an Aura at a player already passed
//! cast-time legality (`TargetFilter::Player` was already fully generic),
//! but `ResolveStackTop`'s attach branch only ever matched `Target::Object`,
//! so the Aura silently entered completely unattached and immediately died
//! via the pre-existing CR 704.5m SBA check (`attached_to: None` is
//! illegal) - a card that resolved and instantly died for no visible
//! reason.
//!
//! `GameObject.attached_to_player: Option<PlayerId>` is a new sibling field
//! to `attached_to: Option<ObjectId>`, not a widened `Option<Target>` - same
//! "additive sibling, not a widened payload" precedent this backlog's own
//! `ManaUnit`/`EffectPrimitive` variants already established. Mutually
//! exclusive by construction (`commands::attach_permanent`/
//! `attach_permanent_to_player` each clear the other).
//!
//! Deliberately deferred, named not silently dropped: CR 303.4f/g (entering
//! already-attached via Search/token, not casting) for a player target,
//! CR 303.4d's multi-candidate choice mixing object and player candidates,
//! and CR 303.4m's "enchanted player" reference for continuous/replacement
//! effects (this engine's continuous effects only ever affect objects, a
//! separate and larger gap).

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

/// "Test Aura: enchant player." Any player, no restriction beyond CR
/// 303.4b's own base shape.
fn aura_enchant_player_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Enchant Player".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant player".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_enchant_player".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Player,
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Aura: enchant player with 10 or less life" - a scripted predicate
/// (`TargetFilter::PlayerScript`), so the SBA re-check path (not just plain
/// existence/elimination) is actually exercised. Takes the same
/// `ScriptRuntime` the test's own `check_state_based_actions` call will use
/// - a `ScriptHandle` is only valid against the runtime that registered it
/// (the exact cross-runtime trap `phase5/script-target-requirement-
/// predicate`'s own test construction already hit once).
fn aura_enchant_low_life_player_definition(runtime: &mut ScriptRuntime) -> CardDefinition {
    let handle = runtime
        .load_function("return function(player, source, controller, state) return state.life[player] <= 10 end")
        .expect("predicate should load");
    CardDefinition {
        name: "Test Enchant Low Life Player".to_string(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::PlayerScript(handle),
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        ..aura_enchant_player_definition()
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, obj: GameObject) {
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
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

fn find_battlefield_object_named<'a>(state: &'a GameState, name: &str) -> Option<&'a GameObject> {
    state.objects.values().find(|o| o.zone == ZoneKind::Battlefield && o.definition.name == name)
}

fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
}

// ---------------------------------------------------------------------
// CR 303.4a/b: casting an Aura at a player attaches it on resolution
// ---------------------------------------------------------------------

#[test]
fn casting_an_aura_at_a_player_attaches_it_on_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, aura_enchant_player_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let aura = find_battlefield_object_named(engine.state(), "Test Enchant Player")
        .expect("the Aura should have resolved onto the battlefield");
    assert_eq!(aura.attached_to_player, Some(p1), "the Aura should be attached to the player it targeted");
    assert_eq!(aura.attached_to, None, "attached_to (the object half) should stay empty");
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::AttachedToPlayer { object, player } if object == aura.object_id && player == p1)),
        "an AttachedToPlayer event should be logged"
    );
}

// ---------------------------------------------------------------------
// CR 704.5m/303.4c: SBA cleanup for an illegally player-attached Aura
// ---------------------------------------------------------------------

#[test]
fn a_healthy_player_attached_aura_is_unaffected_by_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(0);
    let mut obj = permanent(aura, aura_enchant_player_definition(), p0);
    obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(events.is_empty(), "a legally player-attached Aura shouldn't be touched by SBA, got {events:?}");
}

#[test]
fn an_aura_enchanting_an_eliminated_player_dies_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(0);
    let mut obj = permanent(aura, aura_enchant_player_definition(), p0);
    obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, obj);
    state.players.iter_mut().find(|p| p.player_id == p1).unwrap().eliminated = true;

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura),
        "CR 303.4c: an Aura enchanting a player who left the game should die, got {:?}",
        events[0].kind
    );
}

#[test]
fn an_aura_not_attached_to_anything_still_dies_via_sba_with_the_new_field_present() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(0);
    // Neither attached_to nor attached_to_player set - permanent()'s own default.
    put_on_battlefield(&mut state, permanent(aura, aura_enchant_player_definition(), p0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1, "an Aura attached to neither an object nor a player is still illegal (CR 704.5m)");
}

#[test]
fn an_aura_enchanting_a_player_who_stops_matching_its_own_scripted_filter_falls_off() {
    let mut runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(0);
    let mut obj = permanent(aura, aura_enchant_low_life_player_definition(&mut runtime), p0);
    obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, obj);
    // p1 starts at 20 life - doesn't match "10 or less."
    state.players.iter_mut().find(|p| p.player_id == p1).unwrap().life = 20;

    let events = progressed(check_state_based_actions(&mut state, &runtime));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura),
        "the enchanted player no longer matches the Aura's own filter, so it should fall off and die"
    );
}

#[test]
fn an_aura_enchanting_a_player_who_still_matches_its_scripted_filter_is_unaffected() {
    let mut runtime = ScriptRuntime::new();
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(0);
    let mut obj = permanent(aura, aura_enchant_low_life_player_definition(&mut runtime), p0);
    obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, obj);
    state.players.iter_mut().find(|p| p.player_id == p1).unwrap().life = 5;

    let events = progressed(check_state_based_actions(&mut state, &runtime));
    assert!(events.is_empty(), "the enchanted player still matches the low-life filter, so nothing should happen, got {events:?}");
}
