//! CR 610.3: `Game.Ops.ExileUntilSourceLeaves` exiles its target and links the card to the ability's source.
//! When the source leaves the battlefield the card returns under its owner's control (CR 610.3c), as a one-shot effect where state-based actions settle, through the normal reanimation pipeline. If the source left before the exile resolves, nothing is exiled (CR 610.3a/b).

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WARD: u64 = 900;

const PRIEST: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" } })
    a:SetOperation(Game.Ops.ExileUntilSourceLeaves)
    a:Register()
    local b = Game.CreateActivatedAbility(c)
    b:SetTarget({ card_types = { "Creature" } })
    b:SetOperation(Game.Ops.ExileUntilSourceLeaves)
    b:Register()
    local c2 = Game.CreateActivatedAbility(c)
    c2:AddCost(Game.Costs.SacrificeSelf)
    c2:SetOperation(Game.Ops.GainLife(1))
    c2:Register()
end
return s
"#;

const KILLER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" } })
    a:SetOperation(Game.Ops.Destroy)
    a:Register()
end
return s
"#;

const BOUNCER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" } })
    a:SetOperation(Game.Ops.ReturnToHand)
    a:Register()
end
return s
"#;

const EXILER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" } })
    a:SetOperation(Game.Ops.Exile)
    a:Register()
end
return s
"#;

const REANIMATOR: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" }, zone = "Exile" })
    a:SetOperation(Game.Ops.ReturnToBattlefield())
    a:Register()
end
return s
"#;

const ENTERS_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local t = Game.CreateTriggeredAbility(c)
    t:SetCondition(Game.Conditions.Enters)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

#[derive(Clone)]
struct Creature {
    id: u64,
    name: String,
    owner: PlayerId,
    controller: PlayerId,
    script: Option<&'static str>,
    token: bool,
    legendary: bool,
}

fn creature(id: u64, owner: PlayerId) -> Creature {
    Creature { id, name: format!("Creature {id}"), owner, controller: owner, script: None, token: false, legendary: false }
}

impl Creature {
    fn script(mut self, script: &'static str) -> Self {
        self.script = Some(script);
        self
    }
    fn controlled_by(mut self, controller: PlayerId) -> Self {
        self.controller = controller;
        self
    }
    fn token(mut self) -> Self {
        self.token = true;
        self
    }
    fn legend(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self.legendary = true;
        self
    }
}

fn definition_for(c: &Creature) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        supertypes: if c.legendary { SupertypeSet::LEGENDARY } else { SupertypeSet::empty() },
        script_ref: c
            .script
            .map(|s| ScriptReference::Inline(s.to_string()))
            .unwrap_or(ScriptReference::ByKey(format!("creature_{}", c.id))),
        ..minimal_card_definition(c.id, c.name.clone(), "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn engine_with(creatures: Vec<Creature>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut runtime = ScriptRuntime::new();
    for c in creatures {
        let mut object = card_object(ObjectId(c.id), definition_for(&c), c.owner, ZoneKind::Battlefield);
        object.controller = c.controller;
        object.is_token = c.token;
        object.controlled_since_turn = 0;
        state.objects.insert(ObjectId(c.id), object);
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(c.id));
        if c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime.run_card_script(&mut state, &definition, ObjectId(c.id)).expect("script registers");
        }
    }
    Engine::new(state)
}

fn with_ward(mut engine: Engine) -> Engine {
    let definition = minimal_card_definition(WARD, "Ward", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::ENCHANTMENT);
    engine.state_mut().objects.insert(ObjectId(WARD), card_object(ObjectId(WARD), definition, P1, ZoneKind::Battlefield));
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(WARD));
    engine
}

fn creature_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

fn redirect(source: u64, from: ZoneKind, to: ZoneKind, destination: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: Some(from), to: Some(to), affected: creature_filter() },
            payload: ReplacementPayload::ReplaceZoneDestination(destination),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    let mut status = StepStatus::Progressed;
    for player in [P0, P1] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        status = drain(engine);
    }
    status
}

fn obj(id: u64) -> Target {
    Target::Object(ObjectId(id))
}

fn activate(engine: &mut Engine, source: u64, index: u32, targets: Vec<Target>, payments: Vec<CostPayment>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility { source: ObjectId(source), ability_index: index, targets, chosen_colors: Vec::new(), x_value: None },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments }).unwrap();
    drain(engine);
}

fn activate_and_resolve(engine: &mut Engine, source: u64, index: u32, targets: Vec<Target>) -> StepStatus {
    activate(engine, source, index, targets, Vec::new());
    pass_both(engine)
}

fn find(engine: &Engine, name: &str, zone: ZoneKind) -> Option<GameObject> {
    engine.state().objects.values().find(|o| o.definition.name == name && o.zone == zone).cloned()
}

fn names_in(engine: &Engine, zone: ZoneKind) -> Vec<String> {
    let mut names: Vec<String> = engine.state().objects.values().filter(|o| o.zone == zone).map(|o| o.definition.name.clone()).collect();
    names.sort();
    names
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn priest_holding_a_bear(extra: Vec<Creature>) -> Engine {
    let mut creatures = vec![creature(1, P0).script(PRIEST), creature(2, P1)];
    creatures.extend(extra);
    let mut engine = engine_with(creatures);
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    engine
}

#[test]
fn the_target_is_exiled_and_stays_there_while_the_source_is_on_the_battlefield() {
    let engine = priest_holding_a_bear(Vec::new());

    assert_eq!(names_in(&engine, ZoneKind::Exile), vec!["Creature 2"]);
    assert_eq!(names_in(&engine, ZoneKind::Battlefield), vec!["Creature 1"]);
    let exiled = find(&engine, "Creature 2", ZoneKind::Exile).unwrap();
    assert_ne!(exiled.object_id, ObjectId(2), "CR 400.7: the exiled card is a new object");
    assert_eq!(engine.state().linked_exiles, vec![LinkedExile { source: ObjectId(1), exiled: exiled.object_id }]);
}

#[test]
fn the_card_returns_when_the_source_dies() {
    let mut engine = priest_holding_a_bear(vec![creature(3, P0).script(KILLER)]);
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert_eq!(names_in(&engine, ZoneKind::Graveyard), vec!["Creature 1"]);
    assert!(names_in(&engine, ZoneKind::Exile).is_empty());
    let returned = find(&engine, "Creature 2", ZoneKind::Battlefield).expect("the exiled card came back");
    assert_eq!(returned.controller, P1);
    assert_eq!(returned.owner, P1);
    assert!(engine.state().linked_exiles.is_empty());
    assert!(engine.state().stack.is_empty(), "CR 610.3: a one-shot effect, no trigger on the stack");
}

#[test]
fn the_returned_card_is_a_new_object_that_is_summoning_sick() {
    let mut engine = priest_holding_a_bear(vec![creature(3, P0).script(KILLER)]);
    let exiled_id = find(&engine, "Creature 2", ZoneKind::Exile).unwrap().object_id;
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    let returned = find(&engine, "Creature 2", ZoneKind::Battlefield).unwrap();
    assert_ne!(returned.object_id, exiled_id, "CR 400.7");
    assert_eq!(returned.controlled_since_turn, engine.state().turn_number, "CR 302.6");
    assert!(!returned.tapped);
}

#[test]
fn the_card_returns_when_the_source_is_bounced() {
    let mut engine = priest_holding_a_bear(vec![creature(3, P0).script(BOUNCER)]);
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert_eq!(names_in(&engine, ZoneKind::Hand), vec!["Creature 1"]);
    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some());
    assert!(names_in(&engine, ZoneKind::Exile).is_empty());
}

#[test]
fn the_card_returns_when_the_source_is_exiled() {
    let mut engine = priest_holding_a_bear(vec![creature(3, P0).script(EXILER)]);
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert_eq!(names_in(&engine, ZoneKind::Exile), vec!["Creature 1"]);
    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some());
}

#[test]
fn the_card_returns_when_the_source_is_sacrificed() {
    let mut engine = priest_holding_a_bear(Vec::new());
    activate(&mut engine, 1, 2, Vec::new(), vec![CostPayment::Sacrifice(ObjectId(1))]);
    pass_both(&mut engine);

    assert_eq!(names_in(&engine, ZoneKind::Graveyard), vec!["Creature 1"]);
    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some());
}

#[test]
fn the_card_comes_back_under_its_owners_control_not_the_exilers() {
    let mut engine = engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1).controlled_by(P0), creature(3, P0).script(KILLER)]);
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    let returned = find(&engine, "Creature 2", ZoneKind::Battlefield).unwrap();
    assert_eq!(returned.controller, P1, "CR 610.3c");
}

#[test]
fn two_cards_held_by_one_source_return_together() {
    let mut engine = engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1), creature(4, P1), creature(3, P0).script(KILLER)]);
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    activate_and_resolve(&mut engine, 1, 1, vec![obj(4)]);
    assert_eq!(names_in(&engine, ZoneKind::Exile), vec!["Creature 2", "Creature 4"]);
    assert_eq!(engine.state().linked_exiles.len(), 2);

    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert!(names_in(&engine, ZoneKind::Exile).is_empty());
    assert_eq!(names_in(&engine, ZoneKind::Battlefield), vec!["Creature 2", "Creature 3", "Creature 4"]);
    assert!(engine.state().linked_exiles.is_empty());
}

#[test]
fn the_source_leaving_does_not_return_cards_it_never_held() {
    let mut engine = priest_holding_a_bear(vec![creature(3, P0).script(KILLER), creature(4, P1)]);
    let held = find(&engine, "Creature 2", ZoneKind::Exile).unwrap().object_id;
    // A card some other effect put in exile has no link.
    let stray = engine.state().objects.get(&ObjectId(4)).unwrap().clone();
    {
        let state = engine.state_mut();
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().contents.retain(|id| *id != ObjectId(4));
        state.zone_mut(ZoneKind::Exile, None).unwrap().push_bottom(ObjectId(4));
        state.object_mut(ObjectId(4)).unwrap().zone = ZoneKind::Exile;
    }
    assert_eq!(stray.zone, ZoneKind::Battlefield);
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert!(engine.state().objects.contains_key(&ObjectId(4)));
    assert_eq!(engine.state().object(ObjectId(4)).unwrap().zone, ZoneKind::Exile);
    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some());
    assert!(engine.state().object(held).is_none(), "the held card moved on");
}

// CR 610.3a/b
#[test]
fn nothing_is_exiled_when_the_source_left_before_the_ability_resolved() {
    let mut engine = engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1)]);
    activate(&mut engine, 1, 0, vec![obj(2)], Vec::new());
    // The source leaves while the ability is on the stack.
    {
        let state = engine.state_mut();
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().contents.retain(|id| *id != ObjectId(1));
        state.zone_mut(ZoneKind::Graveyard, Some(P0)).unwrap().push_bottom(ObjectId(1));
        state.object_mut(ObjectId(1)).unwrap().zone = ZoneKind::Graveyard;
    }
    pass_both(&mut engine);

    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some(), "the target stays put");
    assert!(names_in(&engine, ZoneKind::Exile).is_empty());
    assert!(engine.state().linked_exiles.is_empty());
}

#[test]
fn nothing_is_exiled_when_the_source_is_gone_from_the_state_entirely() {
    let mut engine = engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1)]);
    activate(&mut engine, 1, 0, vec![obj(2)], Vec::new());
    {
        let state = engine.state_mut();
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().contents.retain(|id| *id != ObjectId(1));
        state.objects.remove(&ObjectId(1));
    }
    pass_both(&mut engine);

    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some());
    assert!(engine.state().linked_exiles.is_empty());
}

#[test]
fn an_exile_replaced_with_a_graveyard_move_makes_no_link() {
    let mut engine = with_ward(engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1), creature(3, P0).script(KILLER)]));
    engine.state_mut().replacement_effects.push(redirect(WARD, ZoneKind::Battlefield, ZoneKind::Exile, ZoneKind::Graveyard));
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);

    assert_eq!(names_in(&engine, ZoneKind::Graveyard), vec!["Creature 2"]);
    assert!(engine.state().linked_exiles.is_empty());

    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);
    assert_eq!(names_in(&engine, ZoneKind::Graveyard), vec!["Creature 1", "Creature 2"], "the dead card does not come back");
}

#[test]
fn a_replacement_can_redirect_the_return() {
    let mut engine = with_ward(engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1), creature(3, P0).script(KILLER)]));
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    engine.state_mut().replacement_effects.push(redirect(WARD, ZoneKind::Exile, ZoneKind::Battlefield, ZoneKind::Graveyard));
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_none());
    assert!(find(&engine, "Creature 2", ZoneKind::Graveyard).is_some());
    assert!(engine.state().linked_exiles.is_empty(), "the link is spent either way");
}

#[test]
fn competing_replacements_on_the_return_pause_and_the_return_still_finishes() {
    let mut engine = with_ward(engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1), creature(3, P0).script(KILLER)]));
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    engine.state_mut().replacement_effects.push(redirect(WARD, ZoneKind::Exile, ZoneKind::Battlefield, ZoneKind::Graveyard));
    engine.state_mut().replacement_effects.push(redirect(WARD, ZoneKind::Exile, ZoneKind::Battlefield, ZoneKind::Exile));
    let status = activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    let StepStatus::AwaitingReplacementChoice { candidates, .. } = status else {
        panic!("expected a CR 616.1 ordering choice, got {status:?}");
    };
    assert_eq!(candidates.len(), 2);
    engine.submit_replacement_choice(candidates[0]).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == P0), "{status:?}");
    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_none());
    assert!(engine.state().linked_exiles.is_empty());
}

#[test]
fn the_returned_cards_enters_trigger_fires() {
    let mut engine = engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1).script(ENTERS_GAIN_LIFE), creature(3, P0).script(KILLER)]);
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    let before = life(&engine, P1);
    // Kill the priest and stop with the enters trigger on the stack.
    activate(&mut engine, 3, 0, vec![obj(1)], Vec::new());
    pass_both(&mut engine);
    // pass_both resolved the kill; the trigger now waits on the stack or has resolved.
    if !engine.state().stack.is_empty() {
        pass_both(&mut engine);
    }

    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some());
    assert_eq!(life(&engine, P1), before + 1, "the enters trigger resolved for the owner");
}

#[test]
fn a_returned_token_is_gone_for_good() {
    let mut engine = engine_with(vec![creature(1, P0).script(PRIEST), creature(2, P1).token(), creature(3, P0).script(KILLER)]);
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_none(), "CR 111.7: a token that left the battlefield never returns");
    assert!(names_in(&engine, ZoneKind::Exile).is_empty());
    assert!(engine.state().linked_exiles.is_empty());
}

#[test]
fn a_returned_legend_triggers_the_legend_rule() {
    let mut engine = engine_with(vec![
        creature(1, P0).script(PRIEST),
        creature(2, P1).legend("Hero"),
        creature(3, P0).script(KILLER),
    ]);
    activate_and_resolve(&mut engine, 1, 0, vec![obj(2)]);
    // P1 plays a second Hero while the first is exiled.
    let second = creature(5, P1).legend("Hero");
    {
        let state = engine.state_mut();
        let object = card_object(ObjectId(5), definition_for(&second), P1, ZoneKind::Battlefield);
        state.objects.insert(ObjectId(5), object);
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ObjectId(5));
    }
    let status = activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    assert!(
        matches!(status, StepStatus::AwaitingLegendRuleChoice { player, .. } if player == P1),
        "the returned permanent gets its own state-based action pass, got {status:?}"
    );
}

#[test]
fn a_card_returned_early_by_another_effect_is_not_pulled_back_or_duplicated() {
    let mut engine = priest_holding_a_bear(vec![creature(3, P0).script(KILLER), creature(4, P0).script(REANIMATOR)]);
    let exiled = find(&engine, "Creature 2", ZoneKind::Exile).unwrap().object_id;
    activate_and_resolve(&mut engine, 4, 0, vec![Target::Object(exiled)]);
    assert!(find(&engine, "Creature 2", ZoneKind::Battlefield).is_some(), "reanimated early");
    assert!(engine.state().linked_exiles.is_empty(), "the old exile id is retired, so its link goes");

    activate_and_resolve(&mut engine, 3, 0, vec![obj(1)]);

    let copies = engine.state().objects.values().filter(|o| o.definition.name == "Creature 2").count();
    assert_eq!(copies, 1, "the priest dying returns nothing a second time");
    assert_eq!(find(&engine, "Creature 2", ZoneKind::Battlefield).unwrap().controller, P0, "still under the reanimator's control");
}

#[test]
fn the_lua_tag_builds_the_effect() {
    let runtime = ScriptRuntime::new();
    let script = r#"
local s = {}
function s.operation()
    return Game.Ops.ExileUntilSourceLeaves
end
return s
"#;
    let result = runtime.build_operation(script);
    assert!(matches!(result, Ok(Some((Some(AbilityEffect::ExileUntilSourceLeaves), _)))), "got {result:?}");
}
