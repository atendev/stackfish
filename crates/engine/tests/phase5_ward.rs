//! Phase 5 vertical slice: Ward (CR 702.21). "Whenever this permanent becomes
//! the target of a spell or ability an opponent controls, counter that spell
//! or ability unless that player pays [cost]."
//!
//! Three pieces make it up: `GameEventKind::BecameTarget` (logged once the
//! spell is cast, the activation is paid for, or the trigger is put on the
//! stack), `ConditionPrimitive::BecomesTargetOfOpponent`, and
//! `TargetFilter::TriggeringStackObject`, which lets the ward trigger target
//! the spell or ability that caused it without a CR 603.3d choice. The
//! counter itself is the existing `AbilityEffect::UnlessPaid`. Cards are
//! written with `Game.RegisterWard(c, "{2}")`.
//!
//! Also covers the `Game.SelectTargets` fix: its legality check used to pass
//! the resolving stack id as the protection source, which is not a real object
//! for an ability.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const WARD_2: &str = r#"
local s = {}
function s.register_abilities(c)
    Game.RegisterWard(c, "{2}")
end
return s
"#;

const WARD_1_AND_2: &str = r#"
local s = {}
function s.register_abilities(c)
    Game.RegisterWard(c, "{1}")
    Game.RegisterWard(c, "{2}")
end
return s
"#;

fn bear_definition(definition_id: u64, name: &str, script: Option<&str>) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: match script {
            Some(source) => ScriptReference::Inline(source.to_string()),
            None => ScriptReference::ByKey("bear".to_string()),
        },
        ..minimal_card_definition(definition_id, name, "bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn creature_word() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
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
    }
}

/// "Test Doom: Destroy target creature." Free, so casting only pauses for the
/// empty payment.
fn doom_definition(spec: TargetSpec) -> CardDefinition {
    CardDefinition {
        target_requirement: Some(spec),
        effect: Some(AbilityEffect::Destroy),
        ..minimal_card_definition(40, "Test Doom", "test_doom", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn single_doom() -> CardDefinition {
    doom_definition(TargetSpec::Primitive(creature_word()))
}

fn two_player_state() -> GameState {
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn put_on_battlefield(state: &mut GameState, id: ObjectId, definition: CardDefinition, owner: PlayerId) {
    state.objects.insert(id, card_object(id, definition, owner, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
}

fn put_in_hand(state: &mut GameState, id: ObjectId, definition: CardDefinition, owner: PlayerId) {
    state.objects.insert(id, card_object(id, definition, owner, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(owner)).unwrap().push_bottom(id);
}

/// Runs the card script of every battlefield object, sharing one runtime so
/// ability ids don't collide (fixture battlefield cards aren't scripted by the
/// engine itself).
fn run_battlefield_scripts(state: &mut GameState, ids: &[ObjectId]) {
    let mut runtime = ScriptRuntime::new();
    for &id in ids {
        let definition = state.object(id).expect("fixture object").definition.clone();
        runtime.run_card_script(state, &definition, id).expect("script registers");
    }
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("player holds priority, spell is in hand");
}

/// Casts a free spell and pays for it.
fn cast_and_pay(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) -> StepStatus {
    cast(engine, player, card, targets);
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected the cast's (free) payment pause, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a free cost");
    drain(engine)
}

/// If several ward triggers matched at once their controller orders them;
/// keeps the offered order.
fn accept_trigger_order(engine: &mut Engine, status: StepStatus) -> StepStatus {
    match status {
        StepStatus::AwaitingTriggerOrderChoice { group, .. } => {
            engine.submit_trigger_order(group).expect("offered order is valid");
            drain(engine)
        }
        other => other,
    }
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn became_target_events(engine: &Engine, object: ObjectId) -> Vec<(ObjectId, PlayerId)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::BecameTarget { object: o, by, by_controller } if o == object => Some((by, by_controller)),
            _ => None,
        })
        .collect()
}

fn ward_triggers_on_stack(engine: &Engine) -> Vec<&StackObject> {
    engine.state().stack.iter().filter(|s| matches!(s.kind, StackEntryKind::TriggeredAbility)).collect()
}

/// A cast spell is a new object (CR 400.7), so the id it had in hand is gone;
/// this finds the spell now on the stack.
fn spell_on_stack(engine: &Engine) -> ObjectId {
    engine.state().stack.iter().find(|s| matches!(s.kind, StackEntryKind::Spell)).expect("a spell is on the stack").stack_id
}

fn on_battlefield(engine: &Engine, id: ObjectId) -> bool {
    engine.state().object(id).is_some_and(|o| o.zone == ZoneKind::Battlefield)
}

// ---------------------------------------------------------------------
// CR 702.21a - the basic trigger and the unless-paid counter.
// ---------------------------------------------------------------------

/// P1 controls a Ward {2} bear, P0 casts a free Doom at it.
fn ward_bear_vs_doom() -> (Engine, ObjectId, ObjectId) {
    let (bear, doom) = (ObjectId(10), ObjectId(11));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear, bear_definition(1, "Ward Bear", Some(WARD_2)), P1);
    put_in_hand(&mut state, doom, single_doom(), P0);
    run_battlefield_scripts(&mut state, &[bear]);
    (Engine::new(state), bear, doom)
}

#[test]
fn an_opponents_spell_targeting_a_ward_permanent_puts_a_ward_trigger_above_it() {
    let (mut engine, bear, doom) = ward_bear_vs_doom();

    let status = cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    let doom = spell_on_stack(&engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == P0), "got {status:?}");
    assert_eq!(engine.state().stack.len(), 2, "Doom and the ward trigger");
    let ward = engine.state().stack.last().unwrap();
    assert!(matches!(ward.kind, StackEntryKind::TriggeredAbility));
    assert_eq!(ward.source, bear);
    assert_eq!(ward.controller, P1, "the ward permanent's controller controls the trigger");
    assert_eq!(ward.targets, vec![Target::Object(doom)], "it targets the spell that caused it");
    assert_eq!(became_target_events(&engine, bear), vec![(doom, P0)], "one BecameTarget, naming the spell and its controller");
    assert!(on_battlefield(&engine, bear), "ward does not stop the spell from targeting");
}

#[test]
fn declining_the_ward_cost_counters_the_spell() {
    let (mut engine, bear, doom) = ward_bear_vs_doom();
    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    let doom = spell_on_stack(&engine);

    let status = pass_both(&mut engine);
    let StepStatus::AwaitingUnlessPaidChoice { payer, cost, otherwise } = status else {
        panic!("expected AwaitingUnlessPaidChoice, got {status:?}")
    };
    assert_eq!(payer, P0, "the player whose spell targeted the permanent pays, not the ward controller");
    assert_eq!(cost.mana.as_ref().expect("mana cost").mana_value(), 2);
    assert!(matches!(otherwise, UnlessPaidConsequence::Counter));

    engine.decline_unless_paid(P0).expect("declining is always legal");
    drain(&mut engine);

    assert!(engine.state().stack.is_empty(), "Doom was countered and never resolved");
    assert!(on_battlefield(&engine, bear), "the ward permanent survived");
    let countered = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Countered { stack_id } if stack_id == doom));
    assert!(countered, "expected a Countered event for Doom");
}

#[test]
fn paying_the_ward_cost_lets_the_spell_resolve() {
    let (mut engine, bear, doom) = ward_bear_vs_doom();
    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    engine.state_mut().players[0].mana_pool.colorless = 2;

    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { payer, .. } if payer == P0), "got {status:?}");
    let units = vec![ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_unless_paid_payment(P0, PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("two generic covers {2}");
    drain(&mut engine);
    assert_eq!(engine.state().player(P0).unwrap().mana_pool.colorless, 0, "the ward cost was really spent");
    assert_eq!(engine.state().stack.len(), 1, "only Doom is left");

    pass_both(&mut engine);
    assert!(!on_battlefield(&engine, bear), "Doom resolved and destroyed the bear");
}

#[test]
fn a_ward_payment_that_is_too_small_is_rejected_and_declining_still_counters() {
    let (mut engine, bear, doom) = ward_bear_vs_doom();
    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    engine.state_mut().players[0].mana_pool.colorless = 1;
    pass_both(&mut engine);

    let units = vec![ManaUnit::Colorless];
    let rejected =
        engine.submit_unless_paid_payment(P0, PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] });
    assert!(rejected.is_err(), "one mana does not cover {{2}}");

    engine.decline_unless_paid(P0).unwrap();
    drain(&mut engine);
    assert!(engine.state().stack.is_empty());
    assert!(on_battlefield(&engine, bear));
}

#[test]
fn a_ward_permanents_own_controller_targeting_it_does_not_trigger_ward() {
    let (bear, doom) = (ObjectId(10), ObjectId(11));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear, bear_definition(1, "Ward Bear", Some(WARD_2)), P0);
    put_in_hand(&mut state, doom, single_doom(), P0);
    run_battlefield_scripts(&mut state, &[bear]);
    let mut engine = Engine::new(state);

    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    let doom = spell_on_stack(&engine);

    assert_eq!(engine.state().stack.len(), 1, "only Doom, no ward trigger");
    assert!(ward_triggers_on_stack(&engine).is_empty());
    assert_eq!(became_target_events(&engine, bear), vec![(doom, P0)], "the event still happens, ward just ignores it");
}

#[test]
fn a_permanent_without_ward_triggers_nothing() {
    let (bear, doom) = (ObjectId(10), ObjectId(11));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear, bear_definition(1, "Plain Bear", None), P1);
    put_in_hand(&mut state, doom, single_doom(), P0);
    let mut engine = Engine::new(state);

    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);

    assert_eq!(engine.state().stack.len(), 1);
}

#[test]
fn the_ward_trigger_fizzles_cleanly_if_the_spell_is_already_gone() {
    let (mut engine, bear, doom) = ward_bear_vs_doom();
    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    let doom = spell_on_stack(&engine);
    // Something removes Doom from the stack before the ward trigger resolves.
    engine.state_mut().stack.retain(|s| s.stack_id != doom);

    let status = pass_both(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "no payment is asked for a spell that is gone, got {status:?}");
    assert!(engine.state().stack.is_empty());
}

// ---------------------------------------------------------------------
// CR 603.2c / 702.21b - how many triggers.
// ---------------------------------------------------------------------

#[test]
fn two_ward_instances_trigger_separately_and_only_the_first_decline_matters() {
    let (bear, doom) = (ObjectId(10), ObjectId(11));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear, bear_definition(1, "Double Ward Bear", Some(WARD_1_AND_2)), P1);
    put_in_hand(&mut state, doom, single_doom(), P0);
    run_battlefield_scripts(&mut state, &[bear]);
    let mut engine = Engine::new(state);

    let status = cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear)]);
    accept_trigger_order(&mut engine, status);

    assert_eq!(ward_triggers_on_stack(&engine).len(), 2, "one trigger per instance of ward");
    assert_eq!(became_target_events(&engine, bear).len(), 1, "but the permanent became a target once");

    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }), "got {status:?}");
    engine.decline_unless_paid(P0).unwrap();
    let status = drain(&mut engine);

    // Doom is countered, so the second ward trigger has nothing left to
    // counter and does not ask again.
    assert!(!matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }), "got {status:?}");
    assert!(on_battlefield(&engine, bear));
}

#[test]
fn a_spell_targeting_two_ward_permanents_triggers_each_of_them() {
    let (bear_a, bear_b, doom) = (ObjectId(10), ObjectId(12), ObjectId(11));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear_a, bear_definition(1, "Ward Bear A", Some(WARD_2)), P1);
    put_on_battlefield(&mut state, bear_b, bear_definition(2, "Ward Bear B", Some(WARD_2)), P1);
    let both = TargetSpec::Multiple(vec![creature_word(), creature_word()]);
    put_in_hand(&mut state, doom, doom_definition(both), P0);
    run_battlefield_scripts(&mut state, &[bear_a, bear_b]);
    let mut engine = Engine::new(state);

    let status = cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear_a), Target::Object(bear_b)]);
    accept_trigger_order(&mut engine, status);

    assert_eq!(ward_triggers_on_stack(&engine).len(), 2, "one ward trigger per ward permanent");
    assert_eq!(became_target_events(&engine, bear_a).len(), 1);
    assert_eq!(became_target_events(&engine, bear_b).len(), 1);
}

#[test]
fn naming_the_same_permanent_under_two_target_words_is_one_trigger() {
    let (bear, doom) = (ObjectId(10), ObjectId(11));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear, bear_definition(1, "Ward Bear", Some(WARD_2)), P1);
    let twice = TargetSpec::Multiple(vec![creature_word(), creature_word()]);
    put_in_hand(&mut state, doom, doom_definition(twice), P0);
    run_battlefield_scripts(&mut state, &[bear]);
    let mut engine = Engine::new(state);

    cast_and_pay(&mut engine, P0, doom, vec![Target::Object(bear), Target::Object(bear)]);

    assert_eq!(became_target_events(&engine, bear).len(), 1, "CR 603.2c: one event for the one permanent");
    assert_eq!(ward_triggers_on_stack(&engine).len(), 1);
}

// ---------------------------------------------------------------------
// Activated and triggered abilities.
// ---------------------------------------------------------------------

#[test]
fn an_activated_ability_targeting_a_ward_permanent_triggers_ward_and_can_be_countered() {
    let (bear, pinger) = (ObjectId(10), ObjectId(13));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, bear, bear_definition(1, "Ward Bear", Some(WARD_2)), P1);
    put_on_battlefield(
        &mut state,
        pinger,
        minimal_card_definition(3, "Test Pinger", "pinger", ManaCost { symbols: Vec::new() }, CardTypeSet::ARTIFACT),
        P0,
    );
    run_battlefield_scripts(&mut state, &[bear]);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(900),
        source: pinger,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: Vec::new(), alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(creature_word())),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Destroy),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: pinger,
                ability_index: 0,
                targets: vec![Target::Object(bear)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("P0 holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().stack.len(), 2, "the activated ability and the ward trigger");
    let activated = engine.state().stack.iter().find(|s| matches!(s.kind, StackEntryKind::ActivatedAbility)).unwrap().stack_id;
    let ward = engine.state().stack.last().unwrap();
    assert_eq!(ward.targets, vec![Target::Ability(activated)], "an ability is targeted as Target::Ability");

    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingUnlessPaidChoice { payer, .. } if payer == P0), "got {status:?}");
    engine.decline_unless_paid(P0).unwrap();
    drain(&mut engine);

    assert!(engine.state().stack.is_empty(), "the activated ability was countered");
    assert!(on_battlefield(&engine, bear));
}

#[test]
fn a_triggered_ability_targeting_a_ward_permanent_triggers_ward() {
    let (bear, reaper) = (ObjectId(10), ObjectId(14));
    let mut state = GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    put_on_battlefield(&mut state, bear, bear_definition(1, "Ward Bear", Some(WARD_2)), P1);
    put_on_battlefield(
        &mut state,
        reaper,
        minimal_card_definition(4, "Test Reaper", "reaper", ManaCost { symbols: Vec::new() }, CardTypeSet::ARTIFACT),
        P0,
    );
    run_battlefield_scripts(&mut state, &[bear]);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(900),
        source: reaper,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: Some(TargetSpec::Primitive(creature_word())),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Destroy),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);

    // Upkeep: Reaper's trigger asks P0 for its target.
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "got {status:?}");
    engine.submit_trigger_targets(vec![Target::Object(bear)]).expect("the bear is an offered candidate");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(engine.state().stack.len(), 2, "Reaper's trigger and the ward trigger it caused");
    let reaper_trigger = engine.state().stack.iter().find(|s| s.source == reaper).unwrap().stack_id;
    let ward = engine.state().stack.last().unwrap();
    assert_eq!(ward.source, bear);
    assert_eq!(ward.targets, vec![Target::Ability(reaper_trigger)]);
}

// ---------------------------------------------------------------------
// The Game.SelectTargets source fix.
// ---------------------------------------------------------------------

const RED_LAND_SELECT_CREATURE_THEN_DESTROY: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        local target = Game.SelectTargets({ card_types = { "Creature" } })
        return { gain_life = 1 }
    end)
    e:Register()
end
return s
"#;

fn protection_from_red(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionColor(ColorSet::RED)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

#[test]
fn a_selecttargets_answer_checks_protection_against_the_abilitys_source_not_its_stack_id() {
    // An ability's stack id is not an object in the state, so the old call
    // (`stack_id` as the protection source) could never match a quality
    // protection and accepted a protected creature. The check now gets the
    // real source permanent, here a red land.
    let (land, protected_bear, plain_bear) = (ObjectId(20), ObjectId(10), ObjectId(12));
    let mut state = two_player_state();
    put_on_battlefield(&mut state, protected_bear, bear_definition(1, "Protected Bear", None), P1);
    put_on_battlefield(&mut state, plain_bear, bear_definition(2, "Plain Bear", None), P1);
    state.continuous_effects.push(protection_from_red(protected_bear));
    let red_land = CardDefinition {
        color_indicator: Some(ColorSet::RED),
        script_ref: ScriptReference::Inline(RED_LAND_SELECT_CREATURE_THEN_DESTROY.to_string()),
        ..minimal_card_definition(5, "Red Land", "red_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    };
    put_in_hand(&mut state, land, red_land, P0);
    let mut engine = Engine::new(state);

    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PlayLand { card: land } }).unwrap();
    drain(&mut engine);
    let land_on_battlefield = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.back().unwrap();
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: land_on_battlefield,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap();
    drain(&mut engine);
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(&mut engine);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingScriptChoice { .. }), "got {status:?}");

    let rejected = engine.submit_script_choice(Target::Object(protected_bear));
    assert!(
        matches!(rejected, Err(stackfish_engine::engine::ScriptChoiceError::InvalidCandidate)),
        "a creature with protection from red cannot be chosen by a red source, got {rejected:?}"
    );
    engine.submit_script_choice(Target::Object(plain_bear)).expect("an unprotected creature is a legal choice");
}
