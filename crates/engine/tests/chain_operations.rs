//! CR 608.2c: several effects of one ability resolve in the order written.
//! `Game.Ops.Chain(a, b, ...)` builds `AbilityEffect::Chain`, run through the modal loop with every entry chosen and the target set shared.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const BEAR: ObjectId = ObjectId(1);

fn decode(body: &str) -> Result<Option<AbilityEffect>, String> {
    let source = format!("local s = {{}}\nfunction s.operation()\n    return {body}\nend\nreturn s");
    let runtime = ScriptRuntime::new();
    runtime.build_operation(&source).map(|decoded| decoded.expect("declares an operation").0).map_err(|e| format!("{e:?}"))
}

#[test]
fn chain_decodes_to_its_entries_in_order() {
    let effect = decode("Game.Ops.Chain(Game.Ops.Destroy, Game.Ops.DrawCards(1), Game.Ops.GainLife(2))").expect("loads");
    assert_eq!(effect, Some(AbilityEffect::Chain { entries: vec![AbilityEffect::Destroy, AbilityEffect::DrawCards(1), AbilityEffect::GainLife(2)], words: None }));
}

#[test]
fn an_empty_chain_is_a_load_error() {
    let error = decode("Game.Ops.Chain()").expect_err("empty chain");
    assert!(error.contains("at least one"), "{error}");
}

#[test]
fn a_nested_chain_is_a_load_error() {
    let error = decode("Game.Ops.Chain(Game.Ops.Chain(Game.Ops.Destroy, Game.Ops.Tap), Game.Ops.DrawCards(1))").expect_err("nested");
    assert!(error.contains("argument 1 can't be chained"), "{error}");
}

#[test]
fn an_effect_the_modal_loop_cannot_run_is_a_load_error() {
    let error = decode("Game.Ops.Chain(Game.Ops.Destroy, Game.Ops.AddMana(\"G\"))").expect_err("AddMana");
    assert!(error.contains("argument 2 can't be chained"), "{error}");
}

#[test]
fn an_uncalled_constructor_is_a_load_error_that_names_it() {
    let error = decode("Game.Ops.Chain(Game.Ops.Destroy, Game.Ops.Mill)").expect_err("uncalled");
    assert!(error.contains("Game.Ops.Mill") && error.contains("must be called"), "{error}");
}

#[test]
fn a_custom_function_is_a_load_error() {
    let error = decode("Game.Ops.Chain(Game.Ops.Destroy, function() return Game.Ops.GainLife(1) end)").expect_err("function");
    assert!(error.contains("custom function"), "{error}");
}

#[test]
fn a_for_each_player_is_a_load_error() {
    let error = decode("Game.Ops.Chain(Game.Ops.ForEachPlayer(Game.Ops.Mill(1)), Game.Ops.DrawCards(1))").expect_err("ForEachPlayer");
    assert!(error.contains("ForEachPlayer"), "{error}");
}

#[test]
fn a_non_operation_argument_is_a_load_error() {
    let error = decode("Game.Ops.Chain(Game.Ops.Destroy, 3)").expect_err("number");
    assert!(error.contains("argument 2 must be a Game.Ops.* value, got"), "{error}");
}

fn card(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
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

fn player_target() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Player { controller: None },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn chain_spell(entries: Vec<AbilityEffect>, target_requirement: Option<TargetSpec>) -> CardDefinition {
    CardDefinition {
        effect: Some(AbilityEffect::Chain { entries, words: None }),
        target_requirement,
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn engine_with(spell: CardDefinition, library: u64) -> Engine {
    Engine::new(state_with(spell, library))
}

fn state_with(spell: CardDefinition, library: u64) -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut place = |id: ObjectId, def: CardDefinition, owner: PlayerId, zone: ZoneKind| {
        state.objects.insert(id, card_object(id, def, owner, zone));
        let scope = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(owner) };
        state.zone_mut(zone, scope).unwrap().push_bottom(id);
    };
    place(BEAR, card(1, "Bear", CardTypeSet::CREATURE), P1, ZoneKind::Battlefield);
    for i in 0..library {
        place(ObjectId(200 + i), card(200 + i, &format!("P0 card {i}"), CardTypeSet::LAND), P0, ZoneKind::Library);
        place(ObjectId(300 + i), card(300 + i, &format!("P1 card {i}"), CardTypeSet::LAND), P1, ZoneKind::Library);
    }
    place(SPELL, spell, P0, ZoneKind::Hand);
    state
}

fn cast(engine: &mut Engine, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority and the targets are legal");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn hand_size(engine: &Engine, player: PlayerId) -> usize {
    engine.state().zone(ZoneKind::Hand, Some(player)).unwrap().contents.len()
}

fn zones_of(engine: &Engine, name: &str) -> Vec<ZoneKind> {
    engine.state().objects.values().filter(|o| o.definition.name == name).map(|o| o.zone).collect()
}

fn assert_resolved_cleanly(engine: &Engine) {
    assert!(engine.state().stack.is_empty(), "stack drained");
    let markers = engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count();
    assert_eq!(markers, 1, "the stack object resolves once, after its last entry");
    assert_eq!(zones_of(engine, "Test Spell"), vec![ZoneKind::Graveyard], "the spell ends in the graveyard");
}

fn redirect_battlefield_death(source: u64, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter { controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(to),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn destroy_then_draw_runs_both_entries_against_one_target() {
    let spell = chain_spell(vec![AbilityEffect::Destroy, AbilityEffect::DrawCards(1)], Some(creature_target()));
    let mut engine = engine_with(spell, 3);
    let before = hand_size(&engine, P0);
    cast(&mut engine, vec![Target::Object(BEAR)]);
    let in_hand_after_cast = hand_size(&engine, P0);
    assert_eq!(in_hand_after_cast, before - 1, "the spell left the hand");
    pass_both(&mut engine);

    assert_eq!(zones_of(&engine, "Bear"), vec![ZoneKind::Graveyard]);
    assert_eq!(hand_size(&engine, P0), in_hand_after_cast + 1, "the draw ran after the destroy");
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_untargeted_chain_runs_every_entry_on_the_controller() {
    let mut engine = engine_with(chain_spell(vec![AbilityEffect::GainLife(2), AbilityEffect::LoseLife(5), AbilityEffect::DrawCards(2)], None), 3);
    cast(&mut engine, Vec::new());
    let in_hand = hand_size(&engine, P0);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), 17, "gain 2 then lose 5");
    assert_eq!(life(&engine, P1), 20);
    assert_eq!(hand_size(&engine, P0), in_hand + 2);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_targeted_player_chain_aims_every_entry_at_that_player() {
    let mut engine = engine_with(chain_spell(vec![AbilityEffect::LoseLife(2), AbilityEffect::Mill(2)], Some(player_target())), 5);
    cast(&mut engine, vec![Target::Player(P1)]);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P1), 18);
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(P1)).unwrap().contents.len(), 2, "mill read the same target");
    assert_eq!(life(&engine, P0), 20);
    assert_eq!(engine.state().zone(ZoneKind::Library, Some(P0)).unwrap().contents.len(), 5);
    assert_resolved_cleanly(&engine);
}

#[test]
fn entries_run_in_the_order_written() {
    for (entries, tapped_after) in [
        (vec![AbilityEffect::Untap, AbilityEffect::Tap], true),
        (vec![AbilityEffect::Tap, AbilityEffect::Untap], false),
    ] {
        let mut engine = engine_with(chain_spell(entries.clone(), Some(creature_target())), 0);
        cast(&mut engine, vec![Target::Object(BEAR)]);
        pass_both(&mut engine);
        assert_eq!(engine.state().object(BEAR).unwrap().tapped, tapped_after, "{entries:?}");
        assert_resolved_cleanly(&engine);
    }
}

#[test]
fn damage_from_an_earlier_entry_lands_before_a_later_zone_move() {
    let spell = chain_spell(vec![AbilityEffect::Damage(2), AbilityEffect::Mill(1)], Some(player_target()));
    let mut engine = engine_with(spell, 3);
    cast(&mut engine, vec![Target::Player(P1)]);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P1), 18);
    let position = |matches: &dyn Fn(&GameEventKind) -> bool| engine.state().ledger.iter().position(|e| matches(&e.kind));
    let damage = position(&|k| matches!(k, GameEventKind::DamageDealt { .. })).expect("damage logged");
    let milled = position(&|k| matches!(k, GameEventKind::ZoneChange { from: ZoneKind::Library, .. })).expect("mill logged");
    assert!(damage < milled, "the damage event is committed before the mill starts");
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_ordering_pause_in_an_earlier_entry_resumes_the_rest() {
    let mut engine = engine_with(chain_spell(vec![AbilityEffect::Destroy, AbilityEffect::GainLife(3)], Some(creature_target())), 0);
    for (id, to) in [(901, ZoneKind::Exile), (902, ZoneKind::Hand)] {
        let mut def = card(id, "Replacer", CardTypeSet::ENCHANTMENT);
        def.script_ref = ScriptReference::ByKey("replacer".to_string());
        engine.state_mut().objects.insert(ObjectId(id), card_object(ObjectId(id), def, P0, ZoneKind::Battlefield));
        engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(id));
        engine.state_mut().replacement_effects.push(redirect_battlefield_death(id, to));
    }
    cast(&mut engine, vec![Target::Object(BEAR)]);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "CR 616.1: {status:?}");
    assert_eq!(life(&engine, P0), 20, "the second entry waits while the first is paused");

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901))).unwrap();
    drain(&mut engine);

    assert_eq!(zones_of(&engine, "Bear"), vec![ZoneKind::Exile], "the chosen replacement applied");
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_scry_entry_pauses_and_the_next_entry_runs_after_the_answer() {
    let mut engine = engine_with(chain_spell(vec![AbilityEffect::Scry(1), AbilityEffect::GainLife(3)], None), 5);
    cast(&mut engine, Vec::new());
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingScryChoice { cards, .. } = status else { panic!("expected a scry pause, got {status:?}") };
    assert_eq!(life(&engine, P0), 20, "the next entry waits for the scry");

    engine.submit_scry_choice(cards, Vec::new()).unwrap();
    drain(&mut engine);

    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_entry_that_does_nothing_leaves_the_next_one_to_run() {
    // CR 609.3: an indestructible target survives Destroy, and GainLife still happens.
    let mut engine = engine_with(chain_spell(vec![AbilityEffect::Destroy, AbilityEffect::GainLife(3)], Some(creature_target())), 0);
    engine.state_mut().objects.get_mut(&BEAR).unwrap().definition.keywords = KeywordSet::INDESTRUCTIBLE;
    cast(&mut engine, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);

    assert_eq!(zones_of(&engine, "Bear"), vec![ZoneKind::Battlefield]);
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_chain_fizzles_whole_when_its_target_is_gone() {
    // CR 608.2b: the one recheck covers the whole chain.
    let mut engine = engine_with(chain_spell(vec![AbilityEffect::Tap, AbilityEffect::GainLife(3)], Some(creature_target())), 0);
    cast(&mut engine, vec![Target::Object(BEAR)]);
    engine.state_mut().objects.get_mut(&BEAR).unwrap().zone = ZoneKind::Graveyard;
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), 20, "no entry ran");
    assert!(engine.state().stack.is_empty());
}

const TAP_AND_DRAW_ACTIVATED: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "object", card_types = { "Creature" } })
    a:SetOperation(Game.Ops.Chain(Game.Ops.Tap, Game.Ops.DrawCards(1)))
    a:Register()
end
return s
"#;

const DRAW_AND_LOSE_ETB: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.Chain(Game.Ops.DrawCards(1), Game.Ops.LoseLife(2)))
    e:Register()
end
return s
"#;

const GAIN_AND_DRAW_SPELL: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.Chain(Game.Ops.GainLife(2), Game.Ops.DrawCards(1))
end
return s
"#;

#[test]
fn an_activated_ability_runs_its_chain() {
    let mut state = state_with(chain_spell(Vec::new(), None), 3);
    let mut source = card(2, "Tapper", CardTypeSet::ARTIFACT);
    source.script_ref = ScriptReference::Inline(TAP_AND_DRAW_ACTIVATED.to_string());
    let id = ObjectId(2);
    let mut object = card_object(id, source.clone(), P0, ZoneKind::Battlefield);
    object.controlled_since_turn = 0;
    state.objects.insert(id, object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    ScriptRuntime::new().run_card_script(&mut state, &source, id).expect("script registers");
    let mut engine = Engine::new(state);
    pass_both(&mut engine);
    let hand = hand_size(&engine, P0);

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility { source: id, ability_index: 0, targets: vec![Target::Object(BEAR)], chosen_colors: Vec::new(), x_value: None },
        })
        .expect("activation is legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(&mut engine);
    pass_both(&mut engine);

    assert!(engine.state().object(BEAR).unwrap().tapped, "the first entry tapped the target");
    assert_eq!(hand_size(&engine, P0), hand + 1, "the second entry drew");
}

#[test]
fn a_scripted_spell_operation_can_return_a_chain() {
    let runtime = ScriptRuntime::new();
    let (effect, _) = runtime.build_operation(GAIN_AND_DRAW_SPELL).unwrap().unwrap();
    let Some(AbilityEffect::Chain { entries, .. }) = effect else { panic!("expected a chain, got {effect:?}") };

    let mut engine = engine_with(chain_spell(entries, None), 3);
    cast(&mut engine, Vec::new());
    let hand = hand_size(&engine, P0);
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), 22);
    assert_eq!(hand_size(&engine, P0), hand + 1);
    assert_resolved_cleanly(&engine);
}

#[test]
fn an_etb_trigger_resolves_its_chain_after_the_creature_enters() {
    let looter = ObjectId(5);
    let mut def = card(5, "Looter", CardTypeSet::CREATURE);
    def.script_ref = ScriptReference::Inline(DRAW_AND_LOSE_ETB.to_string());
    let mut state = state_with(chain_spell(Vec::new(), None), 3);
    state.objects.insert(looter, card_object(looter, def, P0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(looter);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: looter, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(&mut engine);
    let hand_after_cast = hand_size(&engine, P0);
    assert!(engine.state().abilities.is_empty(), "the trigger registers only once the creature enters");

    // First pass pair resolves the creature spell, which puts the trigger on the stack.
    pass_both(&mut engine);
    assert_eq!(engine.state().stack.len(), 1, "only the ETB trigger is on the stack");
    assert_eq!(hand_size(&engine, P0), hand_after_cast, "the chain hasn't run yet");
    assert_eq!(life(&engine, P0), 20);

    // Second pass pair resolves the trigger and its whole chain.
    pass_both(&mut engine);
    assert!(engine.state().stack.is_empty());
    assert_eq!(hand_size(&engine, P0), hand_after_cast + 1, "the first entry drew");
    assert_eq!(life(&engine, P0), 18, "the second entry lost 2 life");
    assert_eq!(life(&engine, P1), 20);
}
