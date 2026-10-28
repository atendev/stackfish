//! CR 702.11 Hexproof and CR 702.18 Shroud: `KeywordSet` bits read through `effective_characteristics` and enforced in `commands::target_is_legal`, at cast, activation, the CR 603.3d trigger scan and the CR 608.2b recheck.
//! Hexproof stops only an opponent's spell or ability (CR 702.11b), shroud stops everyone (CR 702.18a), and untargeted effects never ask (CR 601.2c).
//! Attached Auras and Equipment are not targeting, so SBA rechecks and the CR 303.4f entry scan use `attachment_target_is_legal`, which skips both keywords but keeps protection (CR 702.16d).
// TODO: hexproof and shroud on players (CR 702.11c)
// TODO: "hexproof from [quality]" (CR 702.11d-g)
// TODO: "can be the target as though it didn't have hexproof" (CR 702.11e)

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

const ICY_MANIPULATOR: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:AddCost(Game.Costs.TapSelf)
    a:SetTarget({ type = "object", card_types = { "Artifact", "Creature", "Land" } })
    a:SetOperation(Game.Ops.Tap)
    a:Register()
end
return s
"#;

const TAP_SELF_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.TapSelf)
    a:Register()
end
return s
"#;

fn static_self_effect(effect: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect({effect})
    e:SetAffected({{ self_only = true }})
    e:Register()
end
return s
"#
    )
}

fn creature_requirement() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            excluded_card_types: None,
            excluded_subtypes: None,
            card_types: Some(CardTypeSet::CREATURE),
            subtypes: None,
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

fn targeted_spell(effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: Some(TargetSpec::Primitive(creature_requirement())),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn untargeted_spell(effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn aura_spell() -> CardDefinition {
    CardDefinition {
        color_indicator: Some(ColorSet::GREEN),
        subtypes: vec![Subtype("Aura".to_string())],
        target_requirement: Some(TargetSpec::Primitive(creature_requirement())),
        ..minimal_card_definition(98, "Test Aura", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::ENCHANTMENT)
    }
}

struct Placed {
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    keywords: KeywordSet,
    subtype: Option<&'static str>,
    script: Option<&'static str>,
    owner: PlayerId,
    zone: ZoneKind,
}

fn creature(id: u64, owner: PlayerId, keywords: KeywordSet) -> Placed {
    Placed { id, name: "Bear", types: CardTypeSet::CREATURE, keywords, subtype: None, script: None, owner, zone: ZoneKind::Battlefield }
}

fn aura(id: u64, owner: PlayerId) -> Placed {
    Placed { name: "Attached Aura", types: CardTypeSet::ENCHANTMENT, subtype: Some("Aura"), ..creature(id, owner, KeywordSet::empty()) }
}

fn equipment(id: u64, owner: PlayerId) -> Placed {
    Placed { name: "Attached Equipment", types: CardTypeSet::ARTIFACT, subtype: Some("Equipment"), ..creature(id, owner, KeywordSet::empty()) }
}

fn definition_for(c: &Placed) -> CardDefinition {
    let script_ref = c.script.map(|s| ScriptReference::Inline(s.to_string())).unwrap_or(ScriptReference::ByKey(c.name.to_string()));
    let is_creature = c.types.contains(CardTypeSet::CREATURE);
    CardDefinition {
        script_ref,
        power: is_creature.then_some(PTValue::Fixed(2)),
        toughness: is_creature.then_some(PTValue::Fixed(2)),
        keywords: c.keywords,
        subtypes: c.subtype.map(|s| vec![Subtype(s.to_string())]).unwrap_or_default(),
        color_indicator: (c.subtype == Some("Aura")).then_some(ColorSet::GREEN),
        target_requirement: (c.subtype == Some("Aura")).then(|| TargetSpec::Primitive(creature_requirement())),
        ..minimal_card_definition(c.id, c.name, "unused", ManaCost { symbols: Vec::new() }, c.types)
    }
}

fn engine_with(cards: Vec<Placed>, spell: Option<CardDefinition>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut runtime = ScriptRuntime::new();
    for c in cards {
        let object = card_object(ObjectId(c.id), definition_for(&c), c.owner, c.zone);
        let mut object = object;
        object.controlled_since_turn = 0;
        state.objects.insert(ObjectId(c.id), object);
        let zone_owner = if matches!(c.zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(c.owner) };
        state.zone_mut(c.zone, zone_owner).unwrap().push_bottom(ObjectId(c.id));
        if c.zone == ZoneKind::Battlefield && c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime.run_card_script(&mut state, &definition, ObjectId(c.id)).expect("script registers");
        }
    }
    if let Some(definition) = spell {
        state.objects.insert(SPELL, card_object(SPELL, definition, P0, ZoneKind::Hand));
        state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    }
    Engine::new(state)
}

fn pass_both(engine: &mut Engine) {
    for player in [P0, P1] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

fn cast(engine: &mut Engine, targets: Vec<Target>) -> Result<(), String> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .map_err(|e| format!("{e:?}"))?;
    let status = drain(engine);
    if let StepStatus::Error(e) = status {
        return Err(format!("{e:?}"));
    }
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
    Ok(())
}

fn cast_and_resolve(engine: &mut Engine, targets: Vec<Target>) {
    cast(engine, targets).expect("the targets are legal");
    pass_both(engine);
}

fn activate(engine: &mut Engine, source: u64, targets: Vec<Target>, payments: Vec<CostPayment>) -> Result<(), String> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility { source: ObjectId(source), ability_index: 0, targets, chosen_colors: Vec::new(), x_value: None },
        })
        .map_err(|e| format!("{e:?}"))?;
    let status = drain(engine);
    if let StepStatus::Error(e) = status {
        return Err(format!("{e:?}"));
    }
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments }).unwrap();
    drain(engine);
    Ok(())
}

fn tapped(engine: &Engine, id: u64) -> bool {
    engine.state().object(ObjectId(id)).expect("object exists").tapped
}

fn fizzled(engine: &Engine) -> bool {
    engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }))
}

fn target(id: u64) -> Target {
    Target::Object(ObjectId(id))
}

const HEXPROOF: KeywordSet = KeywordSet::HEXPROOF;
const SHROUD: KeywordSet = KeywordSet::SHROUD;
const NONE: KeywordSet = KeywordSet::empty();

fn grant_keyword(source: u64, keyword: KeywordSet) -> ContinuousEffect {
    ContinuousEffect {
        source: ObjectId(source),
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
        effect: EffectPayload::Primitive(EffectPrimitive::AddKeyword(keyword)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn protection_from_green(source: u64) -> ContinuousEffect {
    ContinuousEffect {
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionColor(ColorSet::GREEN)),
        ..grant_keyword(source, NONE)
    }
}

fn attach(engine: &mut Engine, attachment: u64, host: u64) {
    let state = engine.state_mut();
    state.objects.get_mut(&ObjectId(attachment)).unwrap().attached_to = Some(ObjectId(host));
    state.objects.get_mut(&ObjectId(host)).unwrap().attachments.push(ObjectId(attachment));
}

fn sba_events(engine: &mut Engine) -> Vec<GameEvent> {
    match check_state_based_actions(engine.state_mut(), &ScriptRuntime::new()) {
        SbaOutcome::Progressed(events, _) => events,
        _ => panic!("fixture didn't expect an SBA choice pause"),
    }
}

#[test]
fn hexproof_blocks_an_opponents_spell_at_cast_time() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF)], Some(targeted_spell(AbilityEffect::Tap)));
    assert!(cast(&mut engine, vec![target(1)]).is_err(), "CR 702.11b: P0 is an opponent of P1");
    assert!(!tapped(&engine, 1));
}

#[test]
fn hexproof_does_not_stop_its_controllers_own_spell() {
    let mut engine = engine_with(vec![creature(1, P0, HEXPROOF)], Some(targeted_spell(AbilityEffect::Tap)));
    cast_and_resolve(&mut engine, vec![target(1)]);
    assert!(tapped(&engine, 1), "CR 702.11b only names opponents");
}

#[test]
fn shroud_blocks_the_controllers_own_spell() {
    let mut engine = engine_with(vec![creature(1, P0, SHROUD)], Some(targeted_spell(AbilityEffect::Tap)));
    assert!(cast(&mut engine, vec![target(1)]).is_err(), "CR 702.18a: shroud stops everyone, including its controller");
    assert!(!tapped(&engine, 1));
}

#[test]
fn shroud_blocks_an_opponents_spell() {
    let mut engine = engine_with(vec![creature(1, P1, SHROUD)], Some(targeted_spell(AbilityEffect::Tap)));
    assert!(cast(&mut engine, vec![target(1)]).is_err());
}

fn icy() -> Placed {
    Placed { name: "Icy Manipulator", types: CardTypeSet::ARTIFACT, script: Some(ICY_MANIPULATOR), ..creature(9, P0, NONE) }
}

#[test]
fn hexproof_blocks_an_opponents_activated_ability_but_not_the_controllers() {
    let mut engine = engine_with(vec![icy(), creature(1, P1, HEXPROOF), creature(2, P0, HEXPROOF)], None);
    pass_both(&mut engine);
    assert!(activate(&mut engine, 9, vec![target(1)], vec![CostPayment::Tap(ObjectId(9))]).is_err(), "opponent's hexproof creature");
    assert!(!tapped(&engine, 1));

    assert!(activate(&mut engine, 9, vec![target(2)], vec![CostPayment::Tap(ObjectId(9))]).is_ok(), "own hexproof creature");
    pass_both(&mut engine);
    assert!(tapped(&engine, 2));
}

#[test]
fn shroud_blocks_an_activated_ability_even_from_its_controller() {
    let mut engine = engine_with(vec![icy(), creature(2, P0, SHROUD)], None);
    pass_both(&mut engine);
    assert!(activate(&mut engine, 9, vec![target(2)], vec![CostPayment::Tap(ObjectId(9))]).is_err());
    assert!(!tapped(&engine, 2));
}

fn equip_ability(source: u64) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: ObjectId(source),
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: Some(FilterController::You),
                zone: None,
            },
            ..creature_requirement()
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Attach),
        intervening_if: None,
    }
}

#[test]
fn equip_can_target_your_own_hexproof_creature_but_not_your_own_shroud_creature() {
    let mut engine = engine_with(vec![equipment(5, P0), creature(1, P0, HEXPROOF), creature(2, P0, SHROUD)], None);
    engine.state_mut().abilities.push(equip_ability(5));

    assert!(activate(&mut engine, 5, vec![target(2)], Vec::new()).is_err(), "equip targets (CR 702.6a), so shroud stops it");
    assert!(activate(&mut engine, 5, vec![target(1)], Vec::new()).is_ok(), "hexproof doesn't stop its own controller");
    pass_both(&mut engine);
    assert_eq!(engine.state().object(ObjectId(5)).unwrap().attached_to, Some(ObjectId(1)));
}

#[test]
fn spell_fizzles_when_its_target_gains_hexproof_before_resolution() {
    let mut engine = engine_with(vec![creature(1, P1, NONE)], Some(targeted_spell(AbilityEffect::Tap)));
    cast(&mut engine, vec![target(1)]).expect("no hexproof yet");
    engine.state_mut().continuous_effects.push(grant_keyword(1, HEXPROOF));
    pass_both(&mut engine);

    assert!(fizzled(&engine), "CR 608.2b: the only target became illegal");
    assert!(!tapped(&engine, 1));
}

#[test]
fn a_multi_target_spell_only_skips_the_target_that_gained_hexproof() {
    let spell = CardDefinition {
        target_requirement: Some(TargetSpec::Multiple(vec![creature_requirement(), creature_requirement()])),
        ..targeted_spell(AbilityEffect::Tap)
    };
    let mut engine = engine_with(vec![creature(1, P1, NONE), creature(2, P1, NONE)], Some(spell));
    cast(&mut engine, vec![target(1), target(2)]).expect("both legal at cast time");
    engine.state_mut().continuous_effects.push(grant_keyword(1, HEXPROOF));
    pass_both(&mut engine);

    assert!(!fizzled(&engine), "one target is still legal");
    assert!(!tapped(&engine, 1), "the hexproof target is skipped");
    assert!(tapped(&engine, 2), "the other target still resolves");
}

#[test]
fn a_hexproof_granted_through_lua_add_keyword_works() {
    let script = static_self_effect(r#"Game.Effects.AddKeyword("Hexproof")"#);
    let mut granted = creature(1, P1, NONE);
    granted.script = Some(Box::leak(script.into_boxed_str()));
    let mut engine = engine_with(vec![granted], Some(targeted_spell(AbilityEffect::Tap)));
    assert!(cast(&mut engine, vec![target(1)]).is_err(), "the layer-6 grant makes it untargetable");
}

#[test]
fn a_shroud_granted_through_lua_add_keyword_works() {
    let script = static_self_effect(r#"Game.Effects.AddKeyword("Shroud")"#);
    let mut granted = creature(1, P0, NONE);
    granted.script = Some(Box::leak(script.into_boxed_str()));
    let mut engine = engine_with(vec![granted], Some(targeted_spell(AbilityEffect::Tap)));
    assert!(cast(&mut engine, vec![target(1)]).is_err());
}

#[test]
fn remove_keyword_makes_a_printed_hexproof_creature_targetable_again() {
    let script = static_self_effect(r#"Game.Effects.RemoveKeyword("Hexproof")"#);
    let mut stripped = creature(1, P1, HEXPROOF);
    stripped.script = Some(Box::leak(script.into_boxed_str()));
    let mut engine = engine_with(vec![stripped], Some(targeted_spell(AbilityEffect::Tap)));
    cast_and_resolve(&mut engine, vec![target(1)]);
    assert!(tapped(&engine, 1));
}

#[test]
fn remove_all_abilities_makes_a_printed_shroud_creature_targetable_again() {
    let script = static_self_effect("Game.Effects.RemoveAllAbilities");
    let mut stripped = creature(1, P1, SHROUD);
    stripped.script = Some(Box::leak(script.into_boxed_str()));
    let mut engine = engine_with(vec![stripped], Some(targeted_spell(AbilityEffect::Tap)));
    cast_and_resolve(&mut engine, vec![target(1)]);
    assert!(tapped(&engine, 1));
}

#[test]
fn lua_accepts_hexproof_and_shroud_but_still_rejects_ward() {
    let mut state = GameState { step: Step::PreCombatMain, ..base_two_player_state(zones(P0, P1, &[ZoneKind::Battlefield])) };
    for (name, expect_ok) in [("Hexproof", true), ("Shroud", true), ("Ward", false)] {
        let script = static_self_effect(&format!(r#"Game.Effects.AddKeyword("{name}")"#));
        let mut c = creature(1, P1, NONE);
        c.script = Some(Box::leak(script.into_boxed_str()));
        let definition = definition_for(&c);
        let result = ScriptRuntime::new().run_card_script(&mut state, &definition, ObjectId(1));
        assert_eq!(result.is_ok(), expect_ok, "{name}: {result:?}");
    }
}

fn upkeep_destroy_trigger(source: u64, exclude_source: bool) -> RegisteredAbility {
    RegisteredAbility {
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        target: Some(TargetSpec::Primitive(TargetRequirement { exclude_source, ..creature_requirement() })),
        effect: Some(AbilityEffect::Destroy),
        ..equip_ability(source)
    }
}

// Starts at PreGame, so one pass_both lands on P0's upkeep.
fn trigger_engine(cards: Vec<Placed>, exclude_source: bool) -> Engine {
    let mut state = GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    for c in cards {
        state.objects.insert(ObjectId(c.id), card_object(ObjectId(c.id), definition_for(&c), c.owner, c.zone));
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(c.id));
    }
    state.abilities.push(upkeep_destroy_trigger(1, exclude_source));
    Engine::new(state)
}

fn pass_both_to_upkeep(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn trigger_candidates_skip_shroud_and_opponent_hexproof_creatures() {
    let mut engine = trigger_engine(
        vec![
            creature(1, P0, NONE),    // the trigger's source
            creature(2, P0, HEXPROOF), // own hexproof: still a candidate
            creature(3, P1, HEXPROOF), // opponent's hexproof: not a candidate
            creature(4, P0, SHROUD),   // shroud: never a candidate
            creature(5, P1, NONE),
        ],
        false,
    );
    let StepStatus::AwaitingTriggerTargets { candidates, .. } = pass_both_to_upkeep(&mut engine) else {
        panic!("expected a CR 603.3d target-choice pause")
    };
    let mut ids: Vec<u64> = candidates[0]
        .iter()
        .map(|t| match t {
            Target::Object(id) => id.0,
            other => panic!("unexpected candidate {other:?}"),
        })
        .collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2, 5]);
}

#[test]
fn a_trigger_whose_only_candidate_has_shroud_is_removed_from_the_stack() {
    let mut engine = trigger_engine(vec![creature(1, P0, NONE), creature(2, P1, SHROUD)], true);
    let status = pass_both_to_upkeep(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == P0), "no pause: {status:?}");
    assert!(engine.state().stack.is_empty(), "CR 603.3d: no legal target, so the ability was never pushed");
    assert!(engine.state().object(ObjectId(2)).unwrap().zone == ZoneKind::Battlefield);
}

#[test]
fn an_aura_spell_cannot_target_an_opponents_hexproof_creature_but_can_target_your_own() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF), creature(2, P0, HEXPROOF)], Some(aura_spell()));
    assert!(cast(&mut engine, vec![target(1)]).is_err(), "CR 115.1b: an Aura spell targets");

    let mut engine = engine_with(vec![creature(2, P0, HEXPROOF)], Some(aura_spell()));
    cast_and_resolve(&mut engine, vec![target(2)]);
    let aura = engine.state().objects.values().find(|o| o.definition.name == "Test Aura" && o.zone == ZoneKind::Battlefield).expect("the Aura resolved");
    assert_eq!(aura.attached_to, Some(ObjectId(2)));
}

#[test]
fn an_attached_aura_stays_on_a_creature_that_has_hexproof_and_shroud() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF | SHROUD), aura(2, P0), aura(3, P1)], None);
    attach(&mut engine, 2, 1);
    attach(&mut engine, 3, 1);

    let events = sba_events(&mut engine);
    assert!(events.is_empty(), "an Aura permanent doesn't target (CR 115.1b), got {events:?}");
    assert_eq!(engine.state().object(ObjectId(2)).unwrap().attached_to, Some(ObjectId(1)));
    assert_eq!(engine.state().object(ObjectId(3)).unwrap().attached_to, Some(ObjectId(1)));
}

#[test]
fn an_aura_stays_when_its_creature_gains_hexproof_after_attaching() {
    let mut engine = engine_with(vec![creature(1, P1, NONE), aura(2, P0)], None);
    attach(&mut engine, 2, 1);
    engine.state_mut().continuous_effects.push(grant_keyword(1, HEXPROOF | SHROUD));

    assert!(sba_events(&mut engine).is_empty());
    assert_eq!(engine.state().object(ObjectId(2)).unwrap().attached_to, Some(ObjectId(1)));
}

#[test]
fn equipment_stays_attached_to_a_creature_that_has_hexproof_and_shroud() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF | SHROUD), equipment(2, P0)], None);
    attach(&mut engine, 2, 1);

    assert!(sba_events(&mut engine).is_empty(), "attachment by SBA isn't targeting");
    assert_eq!(engine.state().object(ObjectId(2)).unwrap().attached_to, Some(ObjectId(1)));
}

#[test]
fn protection_still_drops_an_aura_while_hexproof_does_not() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF), creature(3, P1, HEXPROOF), aura(2, P0), aura(4, P0)], None);
    attach(&mut engine, 2, 1);
    attach(&mut engine, 4, 3);
    engine.state_mut().continuous_effects.push(protection_from_green(3));

    let events = sba_events(&mut engine);
    assert_eq!(events.len(), 1, "only the Aura on the protected creature falls off (CR 702.16d): {events:?}");
    assert_eq!(engine.state().object(ObjectId(2)).unwrap().attached_to, Some(ObjectId(1)), "hexproof alone keeps its Aura");
    assert!(
        matches!(&events[0].kind, GameEventKind::ZoneChange { last_known_info: Some(o), .. } if o.object_id == ObjectId(4)),
        "the Aura that fell off is the one on the protected creature"
    );
}

fn aura_entry_state(library: Vec<ObjectId>) -> GameState {
    let mut zone_list = hand_battlefield_graveyard_zones(P0, P1);
    zone_list.push(Zone { kind: ZoneKind::Library, owner: Some(P0), contents: VecDeque::from(library) });
    zone_list.push(Zone::new(ZoneKind::Library, Some(P1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..base_two_player_state(zone_list)
    }
}

#[test]
fn an_aura_entering_the_battlefield_attached_can_pick_a_hexproof_or_shroud_creature() {
    let aura_card = ObjectId(10);
    let mut state = aura_entry_state(vec![aura_card]);
    state.objects.insert(aura_card, card_object(aura_card, definition_for(&aura(10, P0)), P0, ZoneKind::Library));
    let bear = creature(11, P1, HEXPROOF | SHROUD);
    state.objects.insert(ObjectId(11), card_object(ObjectId(11), definition_for(&bear), P1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(11));
    let search = CardDefinition {
        effect: Some(AbilityEffect::Search {
            filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            count: 1,
            destination: SearchDestination::Battlefield { tapped: false, enters_attached: true },
            shuffle_after: true,
        }),
        ..minimal_card_definition(40, "Test Search Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::SORCERY)
    };
    state.objects.insert(SPELL, card_object(SPELL, search, P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);

    let mut engine = Engine::new(state);
    cast(&mut engine, Vec::new()).expect("untargeted search spell");
    let status = {
        pass_both(&mut engine);
        drain(&mut engine)
    };
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else { panic!("expected a Search pause, got {status:?}") };
    assert_eq!(candidates, vec![aura_card]);
    engine.submit_search_choice(vec![aura_card]).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == P0), "one legal candidate, no attach pause: {status:?}");

    let new_aura = engine.state().objects.values().find(|o| o.definition.name == "Attached Aura" && o.zone == ZoneKind::Battlefield).expect("the Aura entered");
    assert_eq!(new_aura.attached_to, Some(ObjectId(11)), "CR 303.4f: entering attached isn't targeting");
}

#[test]
fn hexproof_and_shroud_only_apply_on_the_battlefield() {
    let graveyard_creature = TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            excluded_card_types: None,
            excluded_subtypes: None,
            card_types: Some(CardTypeSet::CREATURE),
            subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: Some(ZoneKind::Graveyard),
        },
        ..creature_requirement()
    };
    let spell = CardDefinition {
        target_requirement: Some(TargetSpec::Primitive(graveyard_creature)),
        ..targeted_spell(AbilityEffect::ReturnToHand)
    };
    let mut dead = creature(1, P1, HEXPROOF | SHROUD);
    dead.zone = ZoneKind::Graveyard;
    let mut engine = engine_with(vec![dead], Some(spell));
    cast_and_resolve(&mut engine, vec![target(1)]);
    assert!(!fizzled(&engine), "a hexproof/shroud card in a graveyard is still a legal target");
}

fn creature_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

#[test]
fn destroy_all_is_not_stopped_by_hexproof_or_shroud() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF), creature(2, P0, SHROUD)], Some(untargeted_spell(AbilityEffect::DestroyAll(creature_filter()))));
    cast_and_resolve(&mut engine, Vec::new());
    let on_battlefield = |id: u64| engine.state().object(ObjectId(id)).is_some_and(|o| o.zone == ZoneKind::Battlefield);
    assert!(!on_battlefield(1) && !on_battlefield(2), "CR 601.2c: an 'all' effect doesn't target");
}

#[test]
fn exile_all_is_not_stopped_by_hexproof_or_shroud() {
    let effect = AbilityEffect::ExileAll { filter: creature_filter(), zone: ZoneKind::Battlefield };
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF), creature(2, P0, SHROUD)], Some(untargeted_spell(effect)));
    cast_and_resolve(&mut engine, Vec::new());
    let on_battlefield = |id: u64| engine.state().object(ObjectId(id)).is_some_and(|o| o.zone == ZoneKind::Battlefield);
    assert!(!on_battlefield(1) && !on_battlefield(2));
}

#[test]
fn tap_all_is_not_stopped_by_hexproof() {
    let mut engine = engine_with(vec![creature(1, P1, HEXPROOF)], Some(untargeted_spell(AbilityEffect::TapAll { filter: creature_filter() })));
    cast_and_resolve(&mut engine, Vec::new());
    assert!(tapped(&engine, 1), "replaces the protection-from-everything stand-in in phase5_tap_untap_effects.rs");
}

#[test]
fn tap_all_is_not_stopped_by_shroud() {
    let mut engine = engine_with(vec![creature(1, P0, SHROUD), creature(2, P1, SHROUD)], Some(untargeted_spell(AbilityEffect::TapAll { filter: creature_filter() })));
    cast_and_resolve(&mut engine, Vec::new());
    assert!(tapped(&engine, 1) && tapped(&engine, 2));
}

#[test]
fn a_shroud_permanents_own_untargeted_ability_still_works() {
    let mut bear = creature(1, P0, SHROUD);
    bear.script = Some(TAP_SELF_ABILITY);
    let mut engine = engine_with(vec![bear], None);
    pass_both(&mut engine);
    activate(&mut engine, 1, Vec::new(), Vec::new()).expect("TapSelf doesn't target");
    pass_both(&mut engine);
    assert!(tapped(&engine, 1));
}
