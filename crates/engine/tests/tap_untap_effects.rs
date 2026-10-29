//! CR 701.26: `Game.Ops.Tap`/`Untap` act on the ability's targets, `TapSelf`/`UntapSelf` on its source, and `TapAll(filter)`/`UntapAll(filter)` on every matching battlefield permanent. A no-op flip changes nothing and emits no event (CR 701.26a/b).

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

const TAP_SELF_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.TapSelf)
    a:Register()
end
return s
"#;

const UNTAP_SELF_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.UntapSelf)
    a:Register()
end
return s
"#;

const SACRIFICE_THEN_TAP_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:AddCost(Game.Costs.SacrificeSelf)
    a:SetOperation(Game.Ops.TapSelf)
    a:Register()
end
return s
"#;

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

const PROTECTION_FROM_EVERYTHING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionEverything)
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

const TAP_ALL_CREATURES: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.TapAll({ card_types = { "Creature" }, controller = "opponent" })
end
return s
"#;

const UNTAP_ALL_CREATURES: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.UntapAll({ card_types = { "Creature" } })
end
return s
"#;

const BARE_TAGS: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.Tap
end
return s
"#;

const BAD_FILTER_TYPE: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.TapAll({ card_types = { "Bogus" } })
end
return s
"#;

const NOT_A_TABLE: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.UntapAll("Battlefield")
end
return s
"#;

fn card(id: u64, name: &str, types: CardTypeSet, script: Option<&str>) -> CardDefinition {
    CardDefinition {
        script_ref: script.map(|s| ScriptReference::Inline(s.to_string())).unwrap_or(ScriptReference::ByKey(name.to_string())),
        power: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        toughness: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(id, name, "unused", ManaCost { symbols: Vec::new() }, types)
    }
}

fn creature_target() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
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
    })
}

fn spell(effect: AbilityEffect, targeted: bool) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: targeted.then(creature_target),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

struct Placed {
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    script: Option<&'static str>,
    owner: PlayerId,
    zone: ZoneKind,
    tapped: bool,
}

fn placed(id: u64, name: &'static str, types: CardTypeSet, owner: PlayerId, tapped: bool) -> Placed {
    Placed { id, name, types, script: None, owner, zone: ZoneKind::Battlefield, tapped }
}

fn engine_with(cards: Vec<Placed>, effect: Option<(AbilityEffect, bool)>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut runtime = ScriptRuntime::new();
    for c in cards {
        let mut object = card_object(ObjectId(c.id), card(c.id, c.name, c.types, c.script), c.owner, c.zone);
        object.tapped = c.tapped;
        // Every fixture permanent counts as long-controlled.
        object.controlled_since_turn = 0;
        state.objects.insert(ObjectId(c.id), object);
        let zone_owner = if matches!(c.zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(c.owner) };
        state.zone_mut(c.zone, zone_owner).unwrap().push_bottom(ObjectId(c.id));
        // Fixture battlefield cards never "entered", so register their script by hand.
        if c.zone == ZoneKind::Battlefield && c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime.run_card_script(&mut state, &definition, ObjectId(c.id)).expect("script registers");
        }
    }
    if let Some((effect, targeted)) = effect {
        state.objects.insert(SPELL, card_object(SPELL, spell(effect, targeted), P0, ZoneKind::Hand));
        state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    }
    Engine::new(state)
}

fn creature_filter(controller: Option<FilterController>) -> EffectFilter {
    EffectFilter { controller, card_types: Some(CardTypeSet::CREATURE), subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
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
    cast(engine, targets).expect("p0 holds priority and the targets are legal");
    pass_both(engine);
}

fn activate_and_resolve(engine: &mut Engine, source: u64, targets: Vec<Target>, payments: Vec<CostPayment>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility { source: ObjectId(source), ability_index: 0, targets, chosen_colors: Vec::new(), x_value: None },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments }).unwrap();
    drain(engine);
    pass_both(engine);
}

fn tapped(engine: &Engine, id: u64) -> bool {
    engine.state().object(ObjectId(id)).expect("object exists").tapped
}

fn count_events(engine: &Engine, tapped_kind: bool) -> usize {
    engine
        .state()
        .ledger
        .iter()
        .filter(|e| match &e.kind {
            GameEventKind::Tapped { .. } => tapped_kind,
            GameEventKind::Untapped { .. } => !tapped_kind,
            _ => false,
        })
        .count()
}

fn tap_events(engine: &Engine) -> Vec<ObjectId> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match &e.kind {
            GameEventKind::Tapped { object } => Some(*object),
            _ => None,
        })
        .collect()
}

#[test]
fn tap_taps_an_untapped_target_and_emits_one_tapped_event() {
    let mut engine = engine_with(vec![placed(1, "Bear", CardTypeSet::CREATURE, P1, false)], Some((AbilityEffect::Tap, true)));
    cast_and_resolve(&mut engine, vec![Target::Object(ObjectId(1))]);

    assert!(tapped(&engine, 1));
    assert_eq!(tap_events(&engine), vec![ObjectId(1)]);
    assert_eq!(count_events(&engine, false), 0);
}

#[test]
fn tap_on_an_already_tapped_target_does_nothing_and_emits_no_event() {
    let mut engine = engine_with(vec![placed(1, "Bear", CardTypeSet::CREATURE, P1, true)], Some((AbilityEffect::Tap, true)));
    cast_and_resolve(&mut engine, vec![Target::Object(ObjectId(1))]);

    assert!(tapped(&engine, 1), "CR 701.26a: stays tapped");
    assert_eq!(count_events(&engine, true), 0, "no flip, no event");
    assert!(engine.state().stack.is_empty(), "a legal target that is already tapped resolves; it doesn't fizzle");
}

#[test]
fn untap_untaps_a_tapped_target_and_emits_one_untapped_event() {
    let mut engine = engine_with(vec![placed(1, "Bear", CardTypeSet::CREATURE, P1, true)], Some((AbilityEffect::Untap, true)));
    cast_and_resolve(&mut engine, vec![Target::Object(ObjectId(1))]);

    assert!(!tapped(&engine, 1));
    assert_eq!(count_events(&engine, false), 1);
    assert_eq!(count_events(&engine, true), 0);
}

#[test]
fn untap_on_an_untapped_target_does_nothing_and_emits_no_event() {
    let mut engine = engine_with(vec![placed(1, "Bear", CardTypeSet::CREATURE, P1, false)], Some((AbilityEffect::Untap, true)));
    cast_and_resolve(&mut engine, vec![Target::Object(ObjectId(1))]);

    assert!(!tapped(&engine, 1), "CR 701.26b");
    assert_eq!(count_events(&engine, false), 0);
}

#[test]
fn tap_fizzles_when_the_target_leaves_the_battlefield() {
    let mut engine = engine_with(vec![placed(1, "Bear", CardTypeSet::CREATURE, P1, false)], Some((AbilityEffect::Tap, true)));
    cast(&mut engine, vec![Target::Object(ObjectId(1))]).unwrap();
    // The target dies in response (CR 608.2b: it is no longer in the zone it was targeted in).
    {
        let state = engine.state_mut();
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().contents.retain(|id| *id != ObjectId(1));
        state.zone_mut(ZoneKind::Graveyard, Some(P1)).unwrap().push_bottom(ObjectId(1));
        state.object_mut(ObjectId(1)).unwrap().zone = ZoneKind::Graveyard;
    }
    pass_both(&mut engine);

    assert!(engine.state().stack.is_empty());
    assert_eq!(count_events(&engine, true), 0, "an illegal target is never tapped");
    assert!(!tapped(&engine, 1));
}

#[test]
fn tap_ignores_summoning_sickness() {
    let mut engine = engine_with(vec![placed(1, "Fresh Bear", CardTypeSet::CREATURE, P1, false)], Some((AbilityEffect::Tap, true)));
    engine.state_mut().object_mut(ObjectId(1)).unwrap().controlled_since_turn = 99;
    cast_and_resolve(&mut engine, vec![Target::Object(ObjectId(1))]);

    assert!(tapped(&engine, 1), "CR 302.6 restricts {{T}} costs and attacking, not an effect that taps");
}

#[test]
fn icy_manipulator_style_ability_taps_its_target() {
    let mut icy = placed(1, "Icy Manipulator", CardTypeSet::ARTIFACT, P0, false);
    icy.script = Some(ICY_MANIPULATOR);
    let mut engine = engine_with(vec![icy, placed(2, "Bear", CardTypeSet::CREATURE, P1, false), placed(3, "Forest", CardTypeSet::LAND, P1, false)], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, vec![Target::Object(ObjectId(2))], vec![CostPayment::Tap(ObjectId(1))]);

    assert!(tapped(&engine, 1), "the {{T}} cost");
    assert!(tapped(&engine, 2), "the target");
    assert!(!tapped(&engine, 3), "only the chosen target");
    assert_eq!(tap_events(&engine), vec![ObjectId(1), ObjectId(2)], "cost first, then the effect");
}

#[test]
fn tap_self_taps_the_source_and_emits_one_event() {
    let mut bear = placed(1, "Bear", CardTypeSet::CREATURE, P0, false);
    bear.script = Some(TAP_SELF_ABILITY);
    let mut engine = engine_with(vec![bear], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert!(tapped(&engine, 1));
    assert_eq!(tap_events(&engine), vec![ObjectId(1)]);
}

#[test]
fn untap_self_untaps_the_source() {
    let mut bear = placed(1, "Bear", CardTypeSet::CREATURE, P0, true);
    bear.script = Some(UNTAP_SELF_ABILITY);
    let mut engine = engine_with(vec![bear], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert!(!tapped(&engine, 1));
    assert_eq!(count_events(&engine, false), 1);
}

#[test]
fn tap_self_with_a_stale_source_does_nothing() {
    let mut bear = placed(1, "Bear", CardTypeSet::CREATURE, P0, false);
    bear.script = Some(SACRIFICE_THEN_TAP_SELF);
    let mut engine = engine_with(vec![bear], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, Vec::new(), vec![CostPayment::Sacrifice(ObjectId(1))]);

    assert_eq!(count_events(&engine, true), 0, "CR 400.7: the graveyard object isn't the source");
    assert!(engine.state().objects.values().all(|o| !o.tapped));
}

#[test]
fn tap_all_taps_every_matching_permanent_and_leaves_the_rest() {
    let mut engine = engine_with(
        vec![
            placed(1, "Mine", CardTypeSet::CREATURE, P0, false),
            placed(2, "Theirs", CardTypeSet::CREATURE, P1, false),
            placed(3, "Forest", CardTypeSet::LAND, P0, false),
        ],
        Some((AbilityEffect::TapAll { filter: creature_filter(None) }, false)),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert!(tapped(&engine, 1));
    assert!(tapped(&engine, 2));
    assert!(!tapped(&engine, 3), "a land doesn't match the creature filter");
    assert_eq!(tap_events(&engine), vec![ObjectId(1), ObjectId(2)], "one event each, in battlefield order");
}

#[test]
fn untap_all_respects_the_controller_filter() {
    let mut engine = engine_with(
        vec![placed(1, "Mine", CardTypeSet::CREATURE, P0, true), placed(2, "Theirs", CardTypeSet::CREATURE, P1, true)],
        Some((AbilityEffect::UntapAll { filter: creature_filter(Some(FilterController::You)) }, false)),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert!(!tapped(&engine, 1));
    assert!(tapped(&engine, 2), "the opponent's creature stays tapped");
    assert_eq!(count_events(&engine, false), 1);
}

#[test]
fn tap_all_skips_already_tapped_permanents_and_emits_events_only_for_flips() {
    let mut engine = engine_with(
        vec![placed(1, "Tapped", CardTypeSet::CREATURE, P1, true), placed(2, "Ready", CardTypeSet::CREATURE, P1, false)],
        Some((AbilityEffect::TapAll { filter: creature_filter(None) }, false)),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert!(tapped(&engine, 1) && tapped(&engine, 2));
    assert_eq!(tap_events(&engine), vec![ObjectId(2)], "CR 701.26a: the tapped one isn't tapped again");
}

#[test]
fn tap_all_does_not_target_so_protection_does_not_stop_it() {
    let mut shielded = placed(1, "Shielded", CardTypeSet::CREATURE, P1, false);
    shielded.script = Some(PROTECTION_FROM_EVERYTHING);
    let mut engine = engine_with(vec![shielded], Some((AbilityEffect::TapAll { filter: creature_filter(None) }, false)));
    cast_and_resolve(&mut engine, Vec::new());

    assert!(tapped(&engine, 1), "CR 601.2c: an 'all' effect doesn't target");
}

#[test]
fn a_targeted_tap_cannot_aim_at_a_protected_creature() {
    let mut shielded = placed(1, "Shielded", CardTypeSet::CREATURE, P1, false);
    shielded.script = Some(PROTECTION_FROM_EVERYTHING);
    let mut engine = engine_with(vec![shielded], Some((AbilityEffect::Tap, true)));

    assert!(cast(&mut engine, vec![Target::Object(ObjectId(1))]).is_err());
    assert!(!tapped(&engine, 1));
}

#[test]
fn tap_all_with_no_match_resolves_cleanly() {
    let mut engine = engine_with(vec![placed(1, "Forest", CardTypeSet::LAND, P0, false)], Some((AbilityEffect::TapAll { filter: creature_filter(None) }, false)));
    cast_and_resolve(&mut engine, Vec::new());

    assert!(engine.state().stack.is_empty());
    assert!(!tapped(&engine, 1));
    assert_eq!(count_events(&engine, true), 0);
}

#[test]
fn untap_all_only_touches_the_battlefield() {
    let mut in_graveyard = placed(2, "Dead Bear", CardTypeSet::CREATURE, P1, true);
    in_graveyard.zone = ZoneKind::Graveyard;
    let mut engine = engine_with(
        vec![placed(1, "Bear", CardTypeSet::CREATURE, P1, true), in_graveyard],
        Some((AbilityEffect::UntapAll { filter: creature_filter(None) }, false)),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert!(!tapped(&engine, 1));
    assert!(tapped(&engine, 2), "CR 110.5d: a card in a graveyard has no tapped status to change");
    assert_eq!(count_events(&engine, false), 1);
}

const ATTACKER: ObjectId = ObjectId(10);
const BLOCKER: ObjectId = ObjectId(11);

fn combat_engine(effect: AbilityEffect, blocker_tapped: bool) -> Engine {
    let mut state = GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 0)]),
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    for (id, owner, name) in [(ATTACKER, P0, "Attacker"), (BLOCKER, P1, "Blocker")] {
        let mut object = card_object(id, card(id.0, name, CardTypeSet::CREATURE, None), owner, ZoneKind::Battlefield);
        object.controlled_since_turn = 0;
        object.tapped = id == BLOCKER && blocker_tapped;
        state.objects.insert(id, object);
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    }
    state.objects.insert(SPELL, card_object(SPELL, spell(effect, true), P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: P0, kind: PlayerActionKind::DeclareAttackers { attackers: vec![(ATTACKER, Target::Player(P1))] } })
        .expect("p0 may declare");
    drain(&mut engine);
    engine
}

fn pass_until(engine: &mut Engine, step: Step) {
    for _ in 0..20 {
        if engine.state().step == step {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached {step:?}, stuck at {:?}", engine.state().step);
}

fn declare_blocker(engine: &mut Engine) -> Result<(), String> {
    engine
        .submit_action(PlayerAction { player: P1, kind: PlayerActionKind::DeclareBlockers { blocks: vec![(BLOCKER, ATTACKER)] } })
        .map_err(|e| format!("{e:?}"))?;
    match drain(engine) {
        StepStatus::Error(e) => Err(format!("{e:?}")),
        _ => Ok(()),
    }
}

#[test]
fn a_creature_tapped_by_an_effect_cannot_be_declared_as_a_blocker() {
    let mut engine = combat_engine(AbilityEffect::Tap, false);
    cast_and_resolve(&mut engine, vec![Target::Object(BLOCKER)]);
    assert!(engine.state().object(BLOCKER).unwrap().tapped);
    pass_until(&mut engine, Step::DeclareBlockers);

    assert!(declare_blocker(&mut engine).is_err(), "CR 509.1a: chosen blockers must be untapped");
}

#[test]
fn a_creature_untapped_by_an_effect_can_block() {
    let mut engine = combat_engine(AbilityEffect::Untap, true);
    cast_and_resolve(&mut engine, vec![Target::Object(BLOCKER)]);
    assert!(!engine.state().object(BLOCKER).unwrap().tapped);
    pass_until(&mut engine, Step::DeclareBlockers);

    declare_blocker(&mut engine).expect("an untapped creature may block");
    assert_eq!(engine.state().combat.as_ref().unwrap().blocks, vec![(BLOCKER, ATTACKER)]);
}

#[test]
fn untapping_a_declared_attacker_keeps_it_in_combat_and_it_still_deals_damage() {
    let mut engine = combat_engine(AbilityEffect::Untap, false);
    assert!(engine.state().object(ATTACKER).unwrap().tapped, "attacking tapped it");
    cast_and_resolve(&mut engine, vec![Target::Object(ATTACKER)]);

    assert!(!engine.state().object(ATTACKER).unwrap().tapped);
    assert_eq!(engine.state().combat.as_ref().unwrap().attackers, vec![(ATTACKER, Target::Player(P1))], "CR 506.4b: still attacking");
    pass_until(&mut engine, Step::CombatDamage);
    let life = engine.state().players.iter().find(|p| p.player_id == P1).unwrap().life;
    assert_eq!(life, 18, "CR 506.4b: its combat damage isn't prevented");
}

#[test]
fn lua_registers_every_tap_untap_op() {
    let runtime = ScriptRuntime::new();
    let (effect, _) = runtime.build_operation(BARE_TAGS).unwrap().unwrap();
    assert!(matches!(effect, Some(AbilityEffect::Tap)));

    let (effect, _) = runtime.build_operation(TAP_ALL_CREATURES).unwrap().unwrap();
    match effect {
        Some(AbilityEffect::TapAll { filter }) => {
            assert_eq!(filter.card_types, Some(CardTypeSet::CREATURE));
            assert_eq!(filter.controller, Some(FilterController::Opponent));
        }
        other => panic!("{other:?}"),
    }
    let (effect, _) = runtime.build_operation(UNTAP_ALL_CREATURES).unwrap().unwrap();
    assert!(matches!(effect, Some(AbilityEffect::UntapAll { filter }) if filter.controller.is_none()));
}

#[test]
fn lua_tap_all_and_untap_all_reject_bad_arguments() {
    let runtime = ScriptRuntime::new();
    assert!(runtime.build_operation(BAD_FILTER_TYPE).is_err(), "an unknown card type is a script error");
    assert!(runtime.build_operation(NOT_A_TABLE).is_err(), "the filter must be a table, not a zone name");
}
