//! Phase 5 vertical slice: CR 702.6 "Equip" as a real activated ability -
//! the natural follow-up `phase5/aura-attach`'s own UNRESOLVED list named.
//! Reuses that slice's `commands::attach_permanent`/`GameObject.
//! attached_to`/`.attachments` wholesale; the new pieces are
//! `AbilityEffect::Attach` (the resolution-time primitive), a CR 702.6a
//! "activate only as a sorcery" check, and CR 704.5n's SBA cleanup - a
//! structurally different shape from Auras' own CR 704.5m (an illegally-
//! attached Equipment becomes unattached and *stays on the battlefield*,
//! it doesn't die).
//!
//! Scoping notes:
//! - CR 702.6a's timing restriction is enforced by matching on `effect ==
//!   Some(AbilityEffect::Attach)` in `ActivateAbility`'s own command
//!   handler, not a new `RegisteredAbility` field - see that variant's own
//!   doc comment for why (~94 existing struct literals would need editing
//!   for a restriction only this one effect shape needs today).
//! - CR 704.5n's own legality recheck deliberately does *not* reuse the
//!   equip ability's "creature you control" filter - CR 702.6c is explicit
//!   that restriction only gates who may *activate* equip, not what stays
//!   legally attached afterward. A creature changing controllers after
//!   being equipped does not knock the Equipment off (tested below).
//!
//! Not attempted (named TODOs): CR 702.6c (further equip restrictions
//! beyond "creature you control"), CR 702.6d (multiple equip abilities per
//! card), CR 702.6e (Equip planeswalker), Fortifications, phasing (CR
//! 702.26i).

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const STATIC_EQUIP: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "object", card_types = { "Creature" }, controller = "you" })
    a:SetOperation(Game.Ops.Attach)
    a:Register()
end
return s
"#;

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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Equipment: Equip {0}." Green, so the protection-DEBT test has a
/// real color to grant protection from. `script_ref` is `Inline` (rather
/// than the usual `ByKey` placeholder) only for the one test that actually
/// runs it (Lua self-registration) - every engine-driven test registers
/// the equip ability directly via `equip_ability` instead.
fn equipment_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Equipment".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: Some(ColorSet::GREEN),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Equipment".to_string())],
        oracle_text: "Test Equipment: Equip {0}.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_equipment".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// CR 702.6a's own ability, directly constructed - standing in for the
/// ETB-registered version, same convention every other activated-ability
/// test file in this repo uses. Free ({0}) cost, since cost payment isn't
/// this slice's own concern.
fn equip_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None, excluded_supertypes: None,
                controller: Some(FilterController::You),
                zone: None,
            },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Attach),
        intervening_if: None,
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
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        .expect("player holds priority");
}

fn try_activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the ability's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
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
// Lua wiring
// ---------------------------------------------------------------------

#[test]
fn game_ops_attach_self_registers_a_real_equip_ability() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let equipment = ObjectId(0);
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert_eq!(ability.kind, AbilityKind::Activated);
    assert!(matches!(ability.effect, Some(AbilityEffect::Attach)));
    assert!(matches!(
        &ability.target,
        Some(TargetSpec::Primitive(req)) if matches!(
            req.filter,
            TargetFilter::Object { mana_value: None, card_types: Some(t), controller: Some(FilterController::You), zone: None, .. }
                if t == CardTypeSet::CREATURE
        )
    ));
}

// ---------------------------------------------------------------------
// CR 702.6a: activating Equip
// ---------------------------------------------------------------------

#[test]
fn equipping_a_creature_you_control_attaches_it() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear", 2, 2), p0));
    put_on_battlefield(&mut state, permanent(equipment, equipment_definition(), p0));
    state.abilities.push(equip_ability(equipment));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, equipment, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    let p1 = PlayerId(1);
    pass_both_and_drain(&mut engine, p0, p1);

    let equipment_now = engine.state().object(equipment).unwrap();
    assert_eq!(equipment_now.attached_to, Some(bear));
    assert!(engine.state().object(bear).unwrap().attachments.contains(&equipment));
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Attached { object, attached_to } if object == equipment && attached_to == bear))
    );
}

#[test]
fn equipping_a_creature_you_dont_control_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear", 2, 2), p1));
    put_on_battlefield(&mut state, permanent(equipment, equipment_definition(), p0));
    state.abilities.push(equip_ability(equipment));

    let mut engine = Engine::new(state);
    let err = try_activate(&mut engine, p0, equipment, vec![Target::Object(bear)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "CR 702.6a requires a creature *you control* - got {err:?}");
}

#[test]
fn activating_equip_with_a_nonempty_stack_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Bear", 2, 2), p0));
    put_on_battlefield(&mut state, permanent(equipment, equipment_definition(), p0));
    state.abilities.push(equip_ability(equipment));

    // A dummy stack object, just to make state.stack non-empty - CR
    // 702.6a's "activate only as a sorcery" needs an empty stack.
    let already_on_stack = ObjectId(2);
    state.objects.insert(already_on_stack, permanent(already_on_stack, creature_definition(2, "Bystander", 1, 1), p0));
    state.stack.push(StackObject {
        stack_id: already_on_stack,
        kind: StackEntryKind::Spell,
        source: already_on_stack,
        source_lki: None,
        owner: p0,
        controller: p0,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });

    let mut engine = Engine::new(state);
    let err = try_activate(&mut engine, p0, equipment, vec![Target::Object(bear)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "Equip can only be activated as a sorcery (empty stack), got {err:?}");
}

#[test]
fn reequipping_to_a_different_creature_moves_the_attachment() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear1 = ObjectId(0);
    let bear2 = ObjectId(1);
    let equipment = ObjectId(2);
    put_on_battlefield(&mut state, permanent(bear1, creature_definition(1, "Bear One", 2, 2), p0));
    put_on_battlefield(&mut state, permanent(bear2, creature_definition(2, "Bear Two", 2, 2), p0));
    let mut equipment_obj = permanent(equipment, equipment_definition(), p0);
    equipment_obj.attached_to = Some(bear1);
    put_on_battlefield(&mut state, equipment_obj);
    if let Some(bear1_obj) = state.object_mut(bear1) {
        bear1_obj.attachments.push(equipment);
    }
    state.abilities.push(equip_ability(equipment));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, equipment, vec![Target::Object(bear2)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(equipment).unwrap().attached_to, Some(bear2));
    assert!(
        !engine.state().object(bear1).unwrap().attachments.contains(&equipment),
        "re-equipping should clean the old target's attachments list"
    );
    assert!(engine.state().object(bear2).unwrap().attachments.contains(&equipment));
}

// ---------------------------------------------------------------------
// CR 704.5n: SBA cleanup for an illegally-attached Equipment
// ---------------------------------------------------------------------

#[test]
fn an_equipped_creature_dying_makes_the_equipment_fall_off_but_stay_on_the_battlefield() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.damage_marked = 2; // lethal
    bear_obj.attachments.push(equipment);
    put_on_battlefield(&mut state, bear_obj);
    let mut equipment_obj = permanent(equipment, equipment_definition(), p0);
    equipment_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, equipment_obj);

    // Pass 1: the creature dies (CR 704.5g) - same one-pass-behind timing
    // as an Aura's own CR 704.5m check.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }));
    assert_eq!(state.object(equipment).unwrap().attached_to, Some(bear), "not yet noticed this same pass");

    // Pass 2: the creature is gone - CR 704.5n unattaches the Equipment,
    // but unlike an Aura it *stays on the battlefield*, no ZoneChange.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::Unattached { object, was_attached_to } if object == equipment && was_attached_to == bear));
    let equipment_now = state.object(equipment).expect("the Equipment must still exist");
    assert_eq!(equipment_now.zone, ZoneKind::Battlefield, "CR 704.5n: stays on the battlefield, unlike an Aura");
    assert_eq!(equipment_now.attached_to, None);
}

#[test]
fn an_equipped_creatures_controller_changing_does_not_unattach_the_equipment() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(equipment);
    // Simulates a control-change effect - CR 702.6c's own last sentence
    // says the equip ability's "creature you control" restriction never
    // re-applies once attached, only at activation time.
    bear_obj.controller = p1;
    put_on_battlefield(&mut state, bear_obj);
    let mut equipment_obj = permanent(equipment, equipment_definition(), p0);
    equipment_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, equipment_obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(events.is_empty(), "a controller change alone must never knock an Equipment off, got {:?}", events.iter().map(|e| &e.kind).collect::<Vec<_>>());
    assert_eq!(state.object(equipment).unwrap().attached_to, Some(bear));
}

#[test]
fn a_legally_equipped_creature_is_unaffected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(equipment);
    put_on_battlefield(&mut state, bear_obj);
    let mut equipment_obj = permanent(equipment, equipment_definition(), p0);
    equipment_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, equipment_obj);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(events.is_empty(), "a healthy, legally-attached Equipment should be left alone, got {:?}", events.iter().map(|e| &e.kind).collect::<Vec<_>>());
}

#[test]
fn an_unattached_equipment_is_not_itself_a_violation() {
    // CR 301.5b: Equipment can sit unattached indefinitely - unlike an
    // Aura (CR 704.5m), there's no "must be attached" requirement.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let equipment = ObjectId(0);
    put_on_battlefield(&mut state, permanent(equipment, equipment_definition(), p0));
    // attached_to left at its default None.

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(events.is_empty(), "an unattached Equipment is legal on its own, got {:?}", events.iter().map(|e| &e.kind).collect::<Vec<_>>());
}

/// Bonus: closes the Equip half of CR 702.16's DEBT restriction, same as
/// `phase5/aura-attach` closed the Enchant half - an equipped creature
/// gaining protection from the Equipment's own color makes it fall off.
#[test]
fn an_equipped_creature_gaining_protection_from_the_equipment_makes_it_fall_off() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    let mut bear_obj = permanent(bear, creature_definition(1, "Bear", 2, 2), p0);
    bear_obj.attachments.push(equipment);
    put_on_battlefield(&mut state, bear_obj);
    let mut equipment_obj = permanent(equipment, equipment_definition(), p0);
    equipment_obj.attached_to = Some(bear);
    put_on_battlefield(&mut state, equipment_obj);

    state.continuous_effects.push(protection_from_green(bear));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::Unattached { object, was_attached_to } if object == equipment && was_attached_to == bear));
    let equipment_now = state.object(equipment).unwrap();
    assert_eq!(equipment_now.zone, ZoneKind::Battlefield);
    assert_eq!(equipment_now.attached_to, None);
    assert!(!state.object(bear).unwrap().attachments.contains(&equipment));
}
