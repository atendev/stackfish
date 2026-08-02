//! Phase 5 vertical slice: CR 509.1h's "target attacking creature becomes
//! blocked" effect (`AbilityEffect::BecomeBlocked`) plus CR 702.22i's own
//! banding propagation riding on top of it.
//!
//! `BecomeBlocked` is a genuinely new, generic primitive - not banding-
//! specific - since this engine had zero producer for "a creature becomes
//! blocked" outside the normal `DeclareBlockers` player action before this
//! slice. `CombatState.blocks` (a flat list of real `(blocker, attacker)`
//! pairs) can't represent "blocked, but by nothing," so a new sibling field,
//! `CombatState.effect_blocked: Vec<ObjectId>`, holds it - consulted
//! alongside `blocks` by `damage.rs`'s `declared_blocked_attackers`, the one
//! real consumer: CR 510.1c already means a blocked attacker with zero
//! *living* blockers deals no combat damage (unless trample), the exact
//! same rule this reuses, just reached with zero blockers from the start.
//!
//! CR 702.22i itself: `commands::combat::mark_effect_blocked` (the shared
//! mutation both the resolve arm and every test below exercise indirectly,
//! through the real `Engine::submit_action` pipeline - it's `pub(crate)`,
//! not reachable directly from an integration test) propagates to every
//! other living, still-attacking member of the target's own band -
//! independently blocked by no one each, not by a shared blocker the way CR
//! 702.22h's declared-blocker propagation works.
//!
//! Narrowed target legality (named, not hidden): no `TargetFilter`
//! predicate for "is attacking" exists, and the Lua state snapshot doesn't
//! expose combat data either, so `target_requirement` here is just "target
//! creature" - targeting a non-attacking creature is a silent no-op at
//! resolution (checked inside `mark_effect_blocked` itself), not a real CR
//! 608.2b illegal-target fizzle.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str, power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Fog Effect: Target attacking creature becomes blocked."
fn become_blocked_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 99,
        name: "Test Fog Effect".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Target attacking creature becomes blocked.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_become_blocked".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::BecomeBlocked),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield]))
    }
}

/// Same shape as `phase5_trample.rs`'s own `combat_damage_state` - starts
/// right at `Step::CombatDamage` with `combat` already populated, for the
/// two damage-assignment consumer tests below (no cast needed - `effect_
/// blocked` is populated directly, same as those tests populate `blocks`
/// with a never-placed "blocker" to stand in for one that died).
fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_bands(engine: &mut Engine, player: PlayerId, bands: Vec<Vec<ObjectId>>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBands { bands } })
        .expect("player holds priority");
    drain(engine)
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

/// Same as `pass_both_and_drain`, reversed order - for the tests below
/// where p1 (not p0) is the one who cast the spell and so holds priority
/// back after casting (CR 601.2i).
fn pass_p1_then_p0_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

fn p1_life(engine: &Engine, p1: PlayerId) -> i32 {
    engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life
}

#[test]
fn become_blocked_spell_marks_the_target_attacker_blocked() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Bear", 3, 3, KeywordSet::empty()), p0));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, become_blocked_spell_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    // p0 passes so p1 (the defending player, holding the fog spell) gets priority.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    cast_spell(&mut engine, p1, card, vec![Target::Object(attacker)]);
    pay_free_and_drain(&mut engine);
    pass_p1_then_p0_and_drain(&mut engine, p0, p1);

    let combat = engine.state().combat.as_ref().unwrap();
    assert_eq!(combat.effect_blocked, vec![attacker], "CR 509.1h: the targeted attacker is now blocked, by no one");
    assert!(combat.blocks.is_empty(), "no real blocker was ever declared");
}

#[test]
fn cr_702_22i_propagates_to_every_living_band_member() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let a = ObjectId(0);
    let b = ObjectId(1);
    put_on_battlefield(&mut state, permanent(a, creature_definition(1, "Wing", 2, 2, KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(b, creature_definition(2, "Tail", 2, 2, KeywordSet::empty()), p0));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, become_blocked_spell_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(a, Target::Player(p1)), (b, Target::Player(p1))]);
    let status = declare_bands(&mut engine, p0, vec![vec![a, b]]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "band declaration failed: {status:?}");
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    // Only `a` is targeted - `b` should still become blocked via CR 702.22i.
    cast_spell(&mut engine, p1, card, vec![Target::Object(a)]);
    pay_free_and_drain(&mut engine);
    pass_p1_then_p0_and_drain(&mut engine, p0, p1);

    let combat = engine.state().combat.as_ref().unwrap();
    assert_eq!(combat.effect_blocked.len(), 2, "both band members should be blocked, got {:?}", combat.effect_blocked);
    assert!(combat.effect_blocked.contains(&a));
    assert!(combat.effect_blocked.contains(&b), "CR 702.22i: the untargeted band member becomes blocked too");
}

#[test]
fn a_band_member_already_removed_from_combat_is_not_propagated_to() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let a = ObjectId(0);
    let b = ObjectId(1);
    put_on_battlefield(&mut state, permanent(a, creature_definition(1, "Wing", 2, 2, KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(b, creature_definition(2, "Tail", 2, 2, KeywordSet::empty()), p0));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, become_blocked_spell_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(a, Target::Player(p1)), (b, Target::Player(p1))]);
    let status = declare_bands(&mut engine, p0, vec![vec![a, b]]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "band declaration failed: {status:?}");
    // CR 702.22f: `b` is removed from combat (e.g. destroyed) before the
    // fog effect resolves - it should leave its band, per that same rule
    // `phase5/bands-with-other`'s own combat.attackers-membership check
    // already established.
    engine.state_mut().combat.as_mut().unwrap().attackers.retain(|(id, _)| *id != b);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    cast_spell(&mut engine, p1, card, vec![Target::Object(a)]);
    pay_free_and_drain(&mut engine);
    pass_p1_then_p0_and_drain(&mut engine, p0, p1);

    let combat = engine.state().combat.as_ref().unwrap();
    assert_eq!(combat.effect_blocked, vec![a], "b already left combat - it must not be marked blocked");
}

#[test]
fn an_attacker_already_blocked_is_left_untouched() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Bear", 3, 3, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(2, "Wall", 0, 4, KeywordSet::empty()), p1));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, become_blocked_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    engine.state_mut().combat.as_mut().unwrap().blocks.push((blocker, attacker));
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::CastSpell {
        card, modes: Vec::new(), targets: vec![Target::Object(attacker)], x_value: None, alternative_cost: None, pay_buyback: false,
    } }).expect("player holds priority, spell is in hand");
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let combat = engine.state().combat.as_ref().unwrap();
    assert!(combat.effect_blocked.is_empty(), "already blocked via a real declared blocker - no redundant effect_blocked entry needed");
}

#[test]
fn targeting_a_non_attacking_creature_is_a_silent_no_op() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bystander = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bystander, creature_definition(1, "Bystander", 2, 2, KeywordSet::empty()), p1));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, become_blocked_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bystander)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "no crash from the no-op resolution, got {status:?}");
    assert!(engine.state().combat.is_none(), "combat never started - nothing to mark blocked");
}

#[test]
fn effect_blocked_attacker_with_no_trample_deals_no_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: vec![attacker],
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Bear", 3, 3, KeywordSet::empty()), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(p1_life(&engine, p1), 20, "CR 509.1h/510.1c: effect-blocked with zero blockers deals no damage, no trample");
}

#[test]
fn effect_blocked_trampler_deals_full_power_per_702_19d() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: vec![attacker],
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Rampager", 5, 5, KeywordSet::TRAMPLE), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(p1_life(&engine, p1), 15, "CR 702.19d: trample treats zero blockers as though they all got lethal - full power tramples over");
}
