//! Phase 5 vertical slice: `AbilityEffect::Regenerate` — `Game.Ops.
//! Regenerate()` (CR 701.19a), the first real creation path for a
//! regeneration shield (the existing `phase5_regeneration.rs` coverage only
//! ever constructs a `ReplacementEffect` directly in test fixtures - no
//! spell/ability resolution ever created one). Also closes CR 701.19c: a
//! target with `KeywordSet::CANT_BE_REGENERATED` gets no shield at all,
//! checked at the moment the shield would be created - the resolving spell/
//! ability still fully resolves regardless (CR 701.19c's own wording:
//! activating/casting isn't precluded, only the shield fails to come into
//! being).
//!
//! Same harness as `phase5_pump_effect.rs`: `CardDefinition.effect` set
//! directly exercises the Rust-native fast path `ResolveStackTop` already
//! takes for a recognized primitive, without needing the actual `Game.Ops.
//! Regenerate` Lua binding invoked (same precedent every other
//! `AbilityEffect` primitive's own test file already follows).

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(toughness: i32, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Regenerate: regenerate target creature."
fn regenerate_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Regenerate".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Regenerate target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_regenerate".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::AnyTarget,
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Regenerate),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
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

#[test]
fn regenerate_spell_creates_a_shield_that_saves_its_target_from_lethal_damage() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(2, KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, regenerate_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().replacement_effects.len(), 1, "a shield was created");
    assert_eq!(engine.state().replacement_effects[0].source, target);

    // Lethal damage arrives only now (e.g. from combat) - after the shield
    // already exists, same real-game order CR 701.19a assumes (regenerate
    // in response to damage, before the SBA pass that would otherwise kill
    // it). Marking it any earlier would let the engine's own automatic SBA
    // checks kill (or fizzle the still-unresolved spell against) the
    // creature before this test ever gets to prove anything.
    engine.state_mut().object_mut(target).unwrap().damage_marked = 2;

    let outcome = check_state_based_actions(engine.state_mut(), &ScriptRuntime::new());
    match outcome {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(e.kind, GameEventKind::Regenerated { object } if object == target)),
                "expected a Regenerated event, got {events:?}"
            );
        }
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
    let saved = engine.state().object(target).expect("regenerated, not destroyed");
    assert_eq!(saved.damage_marked, 0, "regeneration clears marked damage");
    assert!(saved.tapped, "regeneration taps the permanent");
}

#[test]
fn regenerate_spell_creates_no_shield_for_a_target_that_cant_be_regenerated() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(target, creature_definition(2, KeywordSet::CANT_BE_REGENERATED), p1, ZoneKind::Battlefield),
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, regenerate_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    // CR 701.19c: the spell still fully resolved (cast, resolved, stack
    // empty) - only the shield itself failed to come into being.
    assert!(engine.state().stack.is_empty(), "the spell still resolved normally");
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "expected priority back to p0 with nothing pending, got {status:?}"
    );
    assert!(engine.state().replacement_effects.is_empty(), "no shield was created");

    // Lethal damage arrives only now, same reasoning as the sibling test
    // above - marking it in the initial state would let the engine's own
    // automatic SBA checks resolve it before this test gets to prove
    // anything about the (nonexistent) shield.
    engine.state_mut().object_mut(target).unwrap().damage_marked = 2;

    // Without a shield, the creature actually dies to its lethal damage.
    let outcome = check_state_based_actions(engine.state_mut(), &ScriptRuntime::new());
    match outcome {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(e.kind, GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. })),
                "expected the creature to die for real, got {events:?}"
            );
        }
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
}

#[test]
fn regenerate_spell_shield_is_pinned_to_its_target_not_a_bystander() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let bystander = ObjectId(1);
    state.objects.insert(target, card_object(target, creature_definition(2, KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.objects.insert(bystander, card_object(bystander, creature_definition(2, KeywordSet::empty()), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bystander);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, regenerate_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().replacement_effects.len(), 1, "only the target got a shield");
    assert_eq!(engine.state().replacement_effects[0].source, target);
}
