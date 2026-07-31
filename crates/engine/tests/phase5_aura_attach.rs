//! Phase 5 vertical slice: CR 303.4's Aura attach machinery - the "known
//! unresolved gap" every protection/gain-control slice's UNRESOLVED note
//! named but deferred (`GameObject.attached_to`/`.attachments` existed as
//! inert placeholder fields, never written to anywhere, until this slice).
//! Scoped to Auras only - Equipment (CR 301.5/702.6, the `Equip` activated
//! ability) needs a whole second `AbilityEffect::Attach`-style primitive
//! plus an activate-only-as-sorcery restriction, named as its own follow-up
//! slice rather than bundled in.
//!
//! Two halves:
//! - `commands::attach_permanent`, called from `engine.rs`'s
//!   `ResolveStackTop` right after an Aura resolves onto the battlefield
//!   (CR 303.4a/b) - the target is already legality-checked by the
//!   pre-existing CR 608.2b recheck (`legal_targets`), so this slice adds
//!   no new targeting mechanism, just the attach itself.
//! - `sba::collect_illegally_attached_auras` (CR 704.5m/303.4c): each SBA
//!   pass, an Aura with no `attached_to`, or whose target no longer passes
//!   its own `target_requirement` filter (re-checked via the same
//!   `commands::target_is_legal` CR 608.2b already uses - which means a
//!   target that gained protection *from this Aura* after it attached is
//!   caught too, closing the one piece of CR 702.16's Enchant "DEBT"
//!   restriction the six protection slices couldn't reach on their own),
//!   dies via the same `resolve_deaths` pipeline every other SBA death
//!   uses.
//!
//! CR 303.4d (an Aura can't enchant itself / an Aura-creature can't enchant
//! anything - `sba::collect_self_enchanted_auras`/`collect_creature_auras`)
//! was added later, in the `phase5/aura-self-enchant` slice - see the tests
//! under that header below. Self-enchant is real as an SBA check but only
//! reachable today via a directly-constructed fixture (no attach call site
//! can target an Aura at itself); the creature-Aura case is reachable via
//! any layer-4 `AddCardType` continuous effect.
//!
//! Still not attempted (named TODOs, not silently narrowed): CR
//! 303.4f/h/i/k/m (non-spell attachment entry, "enchanted X" ability
//! reference - no effect in this engine puts an Aura onto the battlefield
//! except by resolving as a spell, so these have no reachable trigger yet),
//! enchant-subtype restrictions ("Enchant Human" - `target.rs`'s
//! pre-existing subtype-filter gap), and all of Equipment.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

fn creature_definition(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
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
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Aura: enchant creature." Green, so the fizzle/protection tests
/// have a real color to grant protection from.
fn aura_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Aura".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: Some(ColorSet::GREEN),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Test Aura: enchant creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_aura".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// A plain (non-Aura) enchantment, for the "unaffected" control case.
fn plain_enchantment_definition() -> CardDefinition {
    CardDefinition { definition_id: 31, name: "Test Plain Enchantment".to_string(), subtypes: Vec::new(), ..aura_definition() }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, obj: GameObject) {
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

fn protection_from_green(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionColor(ColorSet::GREEN)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
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
// CR 303.4a/b: casting an Aura attaches it on resolution
// ---------------------------------------------------------------------

#[test]
fn casting_an_aura_at_a_legal_creature_attaches_it_on_resolution() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear", 2, 2), p1));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, aura_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let aura = find_battlefield_object_named(engine.state(), "Test Aura").expect("the Aura should have resolved onto the battlefield");
    assert_eq!(aura.attached_to, Some(bear), "the Aura should be attached to its target on resolution");
    let bear_now = engine.state().object(bear).expect("the target is still on the battlefield");
    assert!(bear_now.attachments.contains(&aura.object_id), "the target's own attachments list should include the Aura");
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Attached { object, attached_to } if object == aura.object_id && attached_to == bear)),
        "an Attached event should be logged"
    );
}

#[test]
fn casting_an_aura_at_a_target_that_becomes_illegal_before_resolution_fizzles_and_never_attaches() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear", 2, 2), p1));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, aura_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);

    // CR 608.2b: granting the target protection from the Aura's own color
    // mid-stack makes it an illegal target by resolution time - same
    // mechanism the protection slices' own fizzle tests already exercise.
    engine.state_mut().continuous_effects.push(protection_from_green(bear));

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: the Aura's only target became illegal, so it must fizzle rather than resolve"
    );
    assert!(
        find_battlefield_object_named(engine.state(), "Test Aura").is_none(),
        "a fizzled Aura never enters the battlefield, so there's nothing to attach"
    );
    assert!(engine.state().object(bear).unwrap().attachments.is_empty(), "the target's attachments list stays empty");
}

// ---------------------------------------------------------------------
// CR 704.5m/303.4c: SBA cleanup for an illegally-attached Aura
// ---------------------------------------------------------------------

#[test]
fn an_aura_not_attached_to_anything_dies_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(0);
    put_on_battlefield(&mut state, permanent(aura, aura_definition(), p0));
    // attached_to left at its default None (permanent()'s own default).

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura),
        "an unattached Aura should die (CR 704.5m), minting a new graveyard-side id, got {:?}",
        events[0].kind
    );
}

#[test]
fn an_auras_target_dying_makes_it_fall_off_on_the_next_sba_pass_not_this_one() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.damage_marked = 2; // lethal
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    // Pass 1: the creature dies to lethal damage (CR 704.5g). The Aura's
    // own attached_to is checked against *this pass's* starting state
    // (CR 704.3), where the creature still resolves as a legal target - so
    // the Aura doesn't fall off yet, same "waits one pass" timing
    // collect_ceasing_tokens's own precedent already established.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1, "only the creature's death this pass, got {:?}", events.iter().map(|e| &e.kind).collect::<Vec<_>>());
    assert!(matches!(events[0].kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }));
    assert!(find_battlefield_object_named(&state, "Test Aura").is_some(), "the Aura is still on the battlefield after pass 1");

    // Pass 2: the creature is gone (its old id no longer resolves), so the
    // Aura is now attached to an illegal (nonexistent) target - CR 704.5m.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura));
    assert!(find_battlefield_object_named(&state, "Test Aura").is_none(), "the Aura has now fallen off");
}

#[test]
fn a_healthy_creature_with_an_attached_aura_is_unaffected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(events.is_empty(), "nothing should happen to a healthy, legally-attached Aura, got {:?}", events.iter().map(|e| &e.kind).collect::<Vec<_>>());
}

#[test]
fn a_non_aura_enchantment_is_never_touched_by_the_attach_check() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let relic = ObjectId(0);
    put_on_battlefield(&mut state, permanent(relic, plain_enchantment_definition(), p0));
    // attached_to left at None, same as an unattached Aura would be - but
    // this isn't an Aura (no "Aura" subtype), so CR 704.5m never applies.

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(events.is_empty(), "a plain enchantment with no attachment concept must never be swept up by this check");
}

/// Bonus: closes the one piece of CR 702.16's Enchant "DEBT" restriction
/// the protection slices themselves couldn't reach (their own doc comments
/// named this exact gap) - an Aura's own target gaining protection *from
/// that Aura* after it's already attached makes the Aura fall off, and the
/// dying Aura's id is cleaned out of the target's own `attachments` list
/// (`commands::retire_object`'s new bidirectional-cleanup behavior).
#[test]
fn an_auras_target_gaining_protection_from_it_after_attaching_makes_it_fall_off() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    // The creature gains protection from green (the Aura's own color) some
    // time after the Aura already attached - e.g. from an unrelated static
    // ability. CR 702.16's Enchant restriction says an Aura with the
    // stated quality already attached is put into its owner's graveyard.
    state.continuous_effects.push(protection_from_green(bear));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura));
    assert!(find_battlefield_object_named(&state, "Test Aura").is_none(), "the Aura should have fallen off");
    assert!(
        !state.object(bear).unwrap().attachments.contains(&aura),
        "retire_object should have cleaned the dead Aura's id out of the target's own attachments list"
    );
}

// ---------------------------------------------------------------------
// CR 303.4d: an Aura that can't legally enchant what it's attached to
// ---------------------------------------------------------------------

#[test]
fn a_self_enchanted_aura_dies_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(0);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    // Directly constructed - no real attach call site can produce this
    // (see this file's own header note), but CR 303.4d states the SBA
    // unconditionally.
    aura_obj.attached_to = Some(aura);
    put_on_battlefield(&mut state, aura_obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0].kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura),
        "a self-enchanted Aura should die (CR 303.4d), got {:?}",
        events[0].kind
    );
}

#[test]
fn an_aura_that_becomes_a_creature_unattaches_then_dies_in_the_same_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);

    // A continuous effect adds the Creature type to the Aura itself (layer
    // 4 `AddCardType`) - the mechanism CR 303.4d's "an Aura that's also a
    // creature" clause needs; no printed card does this yet, but the
    // primitive is real.
    state.continuous_effects.push(ContinuousEffect {
        source: aura,
        layer: Layer::TypeChanging,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddCardType(CardTypeSet::CREATURE)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(
        events.iter().any(|e| matches!(e.kind, GameEventKind::Unattached { object, was_attached_to } if object == aura && was_attached_to == bear)),
        "the Aura-creature should become unattached this same pass, got {:?}",
        events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|e| matches!(e.kind, GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. } if object != aura)),
        "the Aura-creature should also die this same pass (CR 303.4d: 'becomes unattached, then is put into its owner's graveyard'), got {:?}",
        events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    assert!(
        !state.object(bear).unwrap().attachments.contains(&aura),
        "the target's own attachments list should no longer include the dead Aura"
    );
}

// ---------------------------------------------------------------------
// CR 303.4e: only the Aura's own controller activates the Aura's own
// abilities, independent of who controls what it enchants
// ---------------------------------------------------------------------

#[test]
fn only_the_auras_controller_activates_its_own_ability_not_the_enchanted_creatures_controller() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let aura = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p1);
    bear_obj.attachments.push(aura);
    put_on_battlefield(&mut state, bear_obj);
    let mut aura_obj = permanent(aura, aura_definition(), p0);
    aura_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, aura_obj);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: aura,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)])),
        intervening_if: None,
    });

    // p1 controls the enchanted creature, not the Aura - CR 303.4e says
    // that's irrelevant to who may activate the Aura's own ability.
    state.priority_player = p1;
    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::ActivateAbility { source: aura, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new() },
        })
        .unwrap();
    assert!(
        matches!(drain(&mut engine), StepStatus::Error(stackfish_engine::commands::CommandError::IllegalAction(_))),
        "the enchanted creature's controller shouldn't be able to activate the Aura's own ability"
    );

    // p0, the Aura's actual controller, can.
    engine.state_mut().priority_player = p0;
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source: aura, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new() },
        })
        .unwrap();
    assert!(
        !matches!(drain(&mut engine), StepStatus::Error(_)),
        "the Aura's own controller should be able to activate its own ability"
    );
}
