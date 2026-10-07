//! Phase 5 vertical slice: `Game.Ops.ReturnToHand()` (CR 400.3) and
//! `:SetFunctionsFrom("Hand")` (CR 113.6).
//!
//! A bounce spell moves its battlefield target to the *owner's* hand through
//! the same replacement-checked scripted zone move Exile uses, clears
//! battlefield-only state on the new object, and lets the hand/library sweep
//! register the card's hand abilities or CDA. `SetFunctionsFrom("Hand")`
//! declares a hand-only ability explicitly instead of by cost shape.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const TARGET: ObjectId = ObjectId(0);
const SPELL: ObjectId = ObjectId(1);

const CYCLING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.DiscardSelf)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const CDA_ALL_COLORS: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetCharacteristicDefining()
    e:SetLayer(Game.Layers.ColorChanging)
    e:SetEffect(Game.Effects.SetColors("WUBRG"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn creature(script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(1, "Bear", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn bounce_spell() -> CardDefinition {
    CardDefinition {
        effect: Some(AbilityEffect::ReturnToHand),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        ..minimal_card_definition(2, "Test Bounce", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

/// `owner` owns the target; `P0` controls it and casts the bounce spell.
fn setup(script: &str, owner: PlayerId, token: bool) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            P0,
            P1,
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile],
        ))
    };
    let mut target = card_object(TARGET, creature(script), owner, ZoneKind::Battlefield);
    target.controller = P0;
    target.is_token = token;
    target.tapped = true;
    target.damage_marked = 1;
    target.counters.add(CounterKind::PlusOnePlusOne, 2, SequenceNo(0));
    state.objects.insert(TARGET, target);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(TARGET);
    state.objects.insert(SPELL, card_object(SPELL, bounce_spell(), P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    Engine::new(state)
}

fn bounce(engine: &mut Engine) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: SPELL,
                modes: Vec::new(),
                targets: vec![Target::Object(TARGET)],
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority");
    assert!(matches!(drain(engine), StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
    for player in [P0, P1] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

fn hand_card(engine: &Engine, player: PlayerId) -> Option<GameObject> {
    let zone = engine.state().zone(ZoneKind::Hand, Some(player))?;
    zone.contents.iter().filter_map(|id| engine.state().object(*id)).find(|o| o.definition.name == "Bear").cloned()
}

#[test]
fn bounce_returns_the_permanent_to_its_owners_hand() {
    let mut engine = setup("return {}", P1, false);
    bounce(&mut engine);
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty());
    assert!(hand_card(&engine, P0).is_none(), "not the controller's hand");
    let card = hand_card(&engine, P1).expect("in the owner's hand");
    assert_eq!(card.controller, P1);
    assert_ne!(card.object_id, TARGET, "CR 400.7: new object");
}

#[test]
fn bounce_clears_battlefield_only_state() {
    let mut engine = setup("return {}", P0, false);
    bounce(&mut engine);
    let card = hand_card(&engine, P0).unwrap();
    assert!(!card.tapped);
    assert_eq!(card.damage_marked, 0);
    assert_eq!(card.counters.count(&CounterKind::PlusOnePlusOne), 0);
}

#[test]
fn a_bounced_token_ceases_to_exist() {
    let mut engine = setup("return {}", P0, true);
    bounce(&mut engine);
    assert!(hand_card(&engine, P0).is_none());
    assert!(engine.state().objects.values().all(|o| !o.is_token), "token ceased (CR 704.5d)");
}

#[test]
fn bounce_fires_a_leaves_the_battlefield_trigger() {
    let mut engine = setup("return {}", P0, false);
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: TARGET,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::LeavesBattlefield)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
    let before = engine.state().player(P0).unwrap().life;
    bounce(&mut engine);
    for player in [P0, P1] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(&mut engine);
    }
    assert_eq!(engine.state().player(P0).unwrap().life, before + 1);
}

#[test]
fn a_bounced_cycling_card_gets_its_hand_ability() {
    let mut engine = setup(CYCLING, P0, false);
    bounce(&mut engine);
    let card = hand_card(&engine, P0).unwrap();
    let count = engine.state().abilities.iter().filter(|a| a.source == card.object_id).count();
    assert_eq!(count, 1, "Cycling registered once for the new hand object");
    assert!(engine.state().abilities.iter().all(|a| a.source != TARGET), "old id's abilities purged");
}

#[test]
fn a_bounced_cda_card_has_its_cda_live_in_hand() {
    let mut engine = setup(CDA_ALL_COLORS, P0, false);
    bounce(&mut engine);
    let card = hand_card(&engine, P0).unwrap();
    assert_eq!(effective_characteristics(engine.state(), &ScriptRuntime::new(), card.object_id).colors, ColorSet::all());
}

// ---- :SetFunctionsFrom("Hand") ----

fn hand_declared(body: &str) -> String {
    format!("local s = {{}}\nfunction s.register_abilities(c)\n{body}\nend\nreturn s")
}

fn run_in(zone: ZoneKind, script: &str) -> Result<GameState, stackfish_engine::script::ScriptError> {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let def = creature(script);
    state.objects.insert(TARGET, card_object(TARGET, def.clone(), P0, zone));
    ScriptRuntime::new().run_card_script(&mut state, &def, TARGET)?;
    Ok(state)
}

#[test]
fn a_hand_declared_activated_ability_registers_only_in_hand() {
    let script = hand_declared("local a = Game.CreateActivatedAbility(c) a:SetFunctionsFrom(\"Hand\") a:SetOperation(Game.Ops.GainLife(1)) a:Register()");
    assert_eq!(run_in(ZoneKind::Hand, &script).unwrap().abilities.len(), 1);
    assert!(run_in(ZoneKind::Battlefield, &script).unwrap().abilities.is_empty());
    assert!(run_in(ZoneKind::Graveyard, &script).unwrap().abilities.is_empty());
}

#[test]
fn a_hand_declared_trigger_registers_only_in_hand() {
    let script = hand_declared("local t = Game.CreateTriggeredAbility(c) t:SetFunctionsFrom(\"Hand\") t:SetCondition(Game.Conditions.Always) t:SetOperation(Game.Ops.GainLife(1)) t:Register()");
    assert_eq!(run_in(ZoneKind::Hand, &script).unwrap().abilities.len(), 1);
    assert!(run_in(ZoneKind::Battlefield, &script).unwrap().abilities.is_empty());
}

#[test]
fn a_hand_declared_static_registers_only_in_hand() {
    let script = hand_declared("local e = Game.CreateStaticAbility(c) e:SetFunctionsFrom(\"Hand\") e:SetLayer(Game.Layers.ColorChanging) e:SetEffect(Game.Effects.SetColors(\"WUBRG\")) e:SetAffected({ self_only = true }) e:Register()");
    assert_eq!(run_in(ZoneKind::Hand, &script).unwrap().continuous_effects.len(), 1);
    assert!(run_in(ZoneKind::Battlefield, &script).unwrap().continuous_effects.is_empty());
}

#[test]
fn stack_is_still_rejected_by_set_functions_from() {
    let script = hand_declared("local a = Game.CreateActivatedAbility(c) a:SetFunctionsFrom(\"Stack\") a:SetOperation(Game.Ops.GainLife(1)) a:Register()");
    assert!(run_in(ZoneKind::Hand, &script).is_err());
}
