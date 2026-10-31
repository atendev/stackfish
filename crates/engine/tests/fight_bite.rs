//! CR 701.14: `Game.Ops.Fight` makes two creatures each deal damage equal to their power to the other, and `Game.Ops.Bite` is one-sided.
//! Each creature is the source of its own `DamageDealt` event, and both amounts are read before either event goes out, so damage is simultaneous. The pairing comes from the ability's target spec.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

const FIGHT_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" }, controller = "opponent" })
    a:SetOperation(Game.Ops.Fight)
    a:Register()
end
return s
"#;

const BITE_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ card_types = { "Creature" }, controller = "opponent" })
    a:SetOperation(Game.Ops.Bite)
    a:Register()
end
return s
"#;

const DEALS_DAMAGE_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.DealsDamage)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const IS_DEALT_DAMAGE_GAIN_LIFE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.IsDealtDamage)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const PREY_UPON: &str = r#"
local s = {}
function s.target_requirement()
    return {
        { card_types = { "Creature" }, controller = "you", count = 1 },
        { card_types = { "Creature" }, controller = "opponent", count = 1 },
    }
end
function s.operation()
    return Game.Ops.Fight
end
return s
"#;

const BITE_SPELL: &str = r#"
local s = {}
function s.target_requirement()
    return {
        { card_types = { "Creature" }, controller = "you", count = 1 },
        { card_types = { "Creature" }, controller = "opponent", count = 1 },
    }
end
function s.operation()
    return Game.Ops.Bite
end
return s
"#;

struct Creature {
    id: u64,
    owner: PlayerId,
    power: i32,
    toughness: i32,
    keywords: KeywordSet,
    color: Option<ColorSet>,
    script: Option<&'static str>,
}

fn creature(id: u64, owner: PlayerId, power: i32, toughness: i32) -> Creature {
    Creature {
        id,
        owner,
        power,
        toughness,
        keywords: KeywordSet::empty(),
        color: None,
        script: None,
    }
}

impl Creature {
    fn keywords(mut self, keywords: KeywordSet) -> Self {
        self.keywords = keywords;
        self
    }
    fn color(mut self, color: ColorSet) -> Self {
        self.color = Some(color);
        self
    }
    fn script(mut self, script: &'static str) -> Self {
        self.script = Some(script);
        self
    }
}

fn definition_for(c: &Creature) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(c.power)),
        toughness: Some(PTValue::Fixed(c.toughness)),
        keywords: c.keywords,
        color_indicator: c.color,
        script_ref: c
            .script
            .map(|s| ScriptReference::Inline(s.to_string()))
            .unwrap_or(ScriptReference::ByKey(format!("creature_{}", c.id))),
        ..minimal_card_definition(
            c.id,
            format!("Creature {}", c.id),
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn word(controller: Option<FilterController>) -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            card_types: Some(CardTypeSet::CREATURE),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }
}

fn two_words() -> TargetSpec {
    TargetSpec::Multiple(vec![
        word(Some(FilterController::You)),
        word(Some(FilterController::Opponent)),
    ])
}

fn same_filter_two_words() -> TargetSpec {
    TargetSpec::Multiple(vec![word(None), word(None)])
}

fn count_two_word() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        count: 2,
        ..word(None)
    })
}

fn instant(effect: AbilityEffect, spec: TargetSpec) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: Some(spec),
        ..minimal_card_definition(
            99,
            "Test Spell",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

fn engine_with(creatures: Vec<Creature>, spell: Option<CardDefinition>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    let mut runtime = ScriptRuntime::new();
    for c in creatures {
        let mut object = card_object(
            ObjectId(c.id),
            definition_for(&c),
            c.owner,
            ZoneKind::Battlefield,
        );
        object.controlled_since_turn = 0;
        state.objects.insert(ObjectId(c.id), object);
        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(ObjectId(c.id));
        if c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime
                .run_card_script(&mut state, &definition, ObjectId(c.id))
                .expect("script registers");
        }
    }
    if let Some(definition) = spell {
        state
            .objects
            .insert(SPELL, card_object(SPELL, definition, P0, ZoneKind::Hand));
        state
            .zone_mut(ZoneKind::Hand, Some(P0))
            .unwrap()
            .push_bottom(SPELL);
    }
    Engine::new(state)
}

fn prey_upon_engine(you: Creature, them: Creature) -> Engine {
    engine_with(
        vec![you, them],
        Some(instant(AbilityEffect::Fight, two_words())),
    )
}

fn bite_engine(you: Creature, them: Creature) -> Engine {
    engine_with(
        vec![you, them],
        Some(instant(AbilityEffect::Bite, two_words())),
    )
}

fn self_effect(id: u64, layer: Layer, primitive: EffectPrimitive) -> ContinuousEffect {
    ContinuousEffect {
        source: ObjectId(id),
        layer,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(primitive),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn prevent_damage_to(source: u64) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt {
                affected: EffectFilter {
                    card_types: None,
                    controller: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::PreventDamage,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn pass_both(engine: &mut Engine) {
    for player in [P0, P1] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        drain(engine);
    }
}

fn pass_until_stack_empty(engine: &mut Engine) {
    for _ in 0..6 {
        if engine.state().stack.is_empty() {
            return;
        }
        pass_both(engine);
    }
    assert!(engine.state().stack.is_empty(), "stack should settle");
}

fn cast(engine: &mut Engine, targets: Vec<Target>) -> Result<(), String> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: SPELL,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .map_err(|e| format!("{e:?}"))?;
    let status = drain(engine);
    if let StepStatus::Error(e) = status {
        return Err(format!("{e:?}"));
    }
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
    Ok(())
}

fn cast_and_resolve(engine: &mut Engine, targets: Vec<Target>) {
    cast(engine, targets).expect("p0 holds priority and the targets are legal");
    pass_both(engine);
}

fn obj(id: u64) -> Target {
    Target::Object(ObjectId(id))
}

fn activate_and_resolve(engine: &mut Engine, source: u64, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: ObjectId(source),
                ability_index: 0,
                targets,
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
    pass_both(engine);
}

fn damage_events(engine: &Engine) -> Vec<(u64, u64, u32, bool)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match &e.kind {
            GameEventKind::DamageDealt {
                source,
                target: Target::Object(target),
                amount,
                is_combat_damage,
            } => Some((source.0, target.0, *amount, *is_combat_damage)),
            _ => None,
        })
        .collect()
}

fn in_graveyard(engine: &Engine, id: u64) -> bool {
    engine
        .state()
        .object(ObjectId(id))
        .is_none_or(|o| o.zone == ZoneKind::Graveyard)
}

fn alive(engine: &Engine, id: u64) -> bool {
    engine
        .state()
        .object(ObjectId(id))
        .is_some_and(|o| o.zone == ZoneKind::Battlefield)
}

fn marked(engine: &Engine, id: u64) -> u32 {
    engine
        .state()
        .object(ObjectId(id))
        .expect("object exists")
        .damage_marked
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn minus_counters(engine: &Engine, id: u64) -> i32 {
    engine
        .state()
        .object(ObjectId(id))
        .unwrap()
        .counters
        .count(&CounterKind::MinusOneMinusOne)
}

fn fizzled(engine: &Engine) -> bool {
    engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }))
}

fn remove_from_battlefield(engine: &mut Engine, id: u64) {
    let state = engine.state_mut();
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .retain(|o| *o != ObjectId(id));
    state
        .zone_mut(
            ZoneKind::Graveyard,
            Some(state.objects[&ObjectId(id)].owner),
        )
        .unwrap()
        .push_bottom(ObjectId(id));
    state.object_mut(ObjectId(id)).unwrap().zone = ZoneKind::Graveyard;
}

#[test]
fn each_creature_deals_damage_equal_to_its_power_to_the_other() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 1), 2, "the 3/3 takes the 2/2's power");
    assert!(alive(&engine, 1));
    assert!(in_graveyard(&engine, 2), "the 2/2 takes 3 and dies");
    assert!(!fizzled(&engine));
}

#[test]
fn two_creatures_kill_each_other_because_the_damage_is_simultaneous() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 3, 3));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert!(in_graveyard(&engine, 1));
    assert!(in_graveyard(&engine, 2));
}

#[test]
fn both_damage_events_are_committed_before_any_creature_dies() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 3, 3));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    let ledger = &engine.state().ledger;
    let last_damage = ledger
        .iter()
        .rposition(|e| matches!(e.kind, GameEventKind::DamageDealt { .. }))
        .unwrap();
    let first_death = ledger
        .iter()
        .position(|e| {
            matches!(
                e.kind,
                GameEventKind::ZoneChange {
                    from: ZoneKind::Battlefield,
                    to: ZoneKind::Graveyard,
                    ..
                }
            )
        })
        .expect("a creature died");
    assert!(
        last_damage < first_death,
        "SBAs only run after both events are committed"
    );
}

#[test]
fn fight_damage_names_each_creature_as_source_and_is_not_combat_damage() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 5), creature(2, P1, 2, 5));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    let events = damage_events(&engine);
    assert_eq!(events.len(), 2, "one event per direction");
    assert!(
        events.contains(&(1, 2, 3, false)),
        "CR 701.14d: the fighter is the source, not combat damage"
    );
    assert!(events.contains(&(2, 1, 2, false)));
}

#[test]
fn fight_reads_power_through_a_pump_effect() {
    let mut engine = prey_upon_engine(creature(1, P0, 1, 5), creature(2, P1, 1, 9));
    engine.state_mut().continuous_effects.push(self_effect(
        1,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT {
            power: 2,
            toughness: 0,
        },
    ));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 2), 3, "1 base power plus the +2/+0");
}

#[test]
fn fight_reads_power_through_a_plus_one_counter() {
    let mut engine = prey_upon_engine(creature(1, P0, 1, 5), creature(2, P1, 1, 9));
    let timestamp = engine.state_mut().allocate_sequence_no();
    engine
        .state_mut()
        .object_mut(ObjectId(1))
        .unwrap()
        .counters
        .add(CounterKind::PlusOnePlusOne, 2, timestamp);
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 2), 3);
}

#[test]
fn fight_reads_power_through_a_set_power_effect() {
    let mut engine = prey_upon_engine(creature(1, P0, 1, 5), creature(2, P1, 1, 9));
    engine.state_mut().continuous_effects.push(self_effect(
        1,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT {
            power: 4,
            toughness: 5,
        },
    ));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 2), 4);
}

#[test]
fn a_zero_power_creature_deals_nothing_but_still_takes_damage() {
    let mut engine = prey_upon_engine(creature(1, P0, 0, 3), creature(2, P1, 2, 5));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(
        marked(&engine, 2),
        0,
        "CR 701.14a: zero power deals no damage"
    );
    assert_eq!(
        marked(&engine, 1),
        2,
        "the other creature still deals damage"
    );
    assert_eq!(
        damage_events(&engine),
        vec![(2, 1, 2, false)],
        "no event for the zero-power side"
    );
}

#[test]
fn a_negative_power_creature_deals_no_damage_and_emits_no_event() {
    let mut engine = prey_upon_engine(creature(1, P0, 1, 5), creature(2, P1, 2, 5));
    engine.state_mut().continuous_effects.push(self_effect(
        1,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT {
            power: -3,
            toughness: 0,
        },
    ));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 2), 0);
    assert_eq!(damage_events(&engine), vec![(2, 1, 2, false)]);
}

#[test]
fn a_creature_that_fights_itself_takes_one_event_of_twice_its_power() {
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 9)],
        Some(instant(AbilityEffect::Fight, same_filter_two_words())),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(1)]);

    assert_eq!(
        damage_events(&engine),
        vec![(1, 1, 6, false)],
        "CR 701.14c: one event, 2x power"
    );
    assert_eq!(marked(&engine, 1), 6);
}

#[test]
fn a_lifelink_creature_that_fights_itself_gains_life_once() {
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 9).keywords(KeywordSet::LIFELINK)],
        Some(instant(AbilityEffect::Fight, same_filter_two_words())),
    );
    let before = life(&engine, P0);
    cast_and_resolve(&mut engine, vec![obj(1), obj(1)]);

    assert_eq!(
        life(&engine, P0),
        before + 6,
        "one damage event of 6, one lifelink gain"
    );
}

#[test]
fn two_target_creatures_fight_each_other_with_a_count_two_word() {
    let mut engine = engine_with(
        vec![creature(1, P1, 3, 5), creature(2, P1, 2, 5)],
        Some(instant(AbilityEffect::Fight, count_two_word())),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 1), 2);
    assert_eq!(marked(&engine, 2), 3);
    assert_eq!(damage_events(&engine).len(), 2);
}

#[test]
fn a_count_two_fight_does_nothing_when_one_target_is_gone() {
    let mut engine = engine_with(
        vec![creature(1, P1, 3, 5), creature(2, P1, 2, 5)],
        Some(instant(AbilityEffect::Fight, count_two_word())),
    );
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    remove_from_battlefield(&mut engine, 2);
    pass_both(&mut engine);

    assert!(
        damage_events(&engine).is_empty(),
        "CR 701.14b: no partner, no fight"
    );
    assert_eq!(marked(&engine, 1), 0);
    assert!(
        !fizzled(&engine),
        "one target is still legal, so the spell resolves"
    );
}

#[test]
fn an_ability_source_fights_its_target() {
    let mut engine = engine_with(
        vec![
            creature(1, P0, 3, 3).script(FIGHT_ABILITY),
            creature(2, P1, 2, 2),
        ],
        None,
    );
    activate_and_resolve(&mut engine, 1, vec![obj(2)]);

    let events = damage_events(&engine);
    assert!(
        events.contains(&(1, 2, 3, false)),
        "the ability's source is the damage source, got {events:?}"
    );
    assert!(events.contains(&(2, 1, 2, false)));
    assert!(in_graveyard(&engine, 2));
    assert_eq!(marked(&engine, 1), 2);
}

#[test]
fn a_deathtouch_fighter_kills_a_larger_creature() {
    let mut engine = prey_upon_engine(
        creature(1, P0, 1, 1).keywords(KeywordSet::DEATHTOUCH),
        creature(2, P1, 5, 5),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert!(in_graveyard(&engine, 2), "any deathtouch damage is lethal");
    assert!(
        in_graveyard(&engine, 1),
        "and the 5/5's damage kills the 1/1 too"
    );
}

#[test]
fn lifelink_gains_life_once_per_damage_event_for_each_side() {
    let mut engine = prey_upon_engine(
        creature(1, P0, 3, 5).keywords(KeywordSet::LIFELINK),
        creature(2, P1, 2, 5).keywords(KeywordSet::LIFELINK),
    );
    let (p0_before, p1_before) = (life(&engine, P0), life(&engine, P1));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(life(&engine, P0), p0_before + 3);
    assert_eq!(life(&engine, P1), p1_before + 2);
}

#[test]
fn wither_fight_damage_becomes_minus_one_counters() {
    let mut engine = prey_upon_engine(
        creature(1, P0, 2, 5).keywords(KeywordSet::WITHER),
        creature(2, P1, 1, 5),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(minus_counters(&engine, 2), 2);
    assert_eq!(
        marked(&engine, 2),
        0,
        "wither damage is counters, not marked damage"
    );
    assert_eq!(
        marked(&engine, 1),
        1,
        "the non-wither side marks damage as usual"
    );
}

#[test]
fn infect_fight_damage_becomes_minus_one_counters() {
    let mut engine = prey_upon_engine(
        creature(1, P0, 2, 5).keywords(KeywordSet::INFECT),
        creature(2, P1, 1, 5),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(minus_counters(&engine, 2), 2);
    assert_eq!(marked(&engine, 2), 0);
}

#[test]
fn protection_from_the_fighters_color_prevents_that_damage_only() {
    let mut engine = prey_upon_engine(
        creature(1, P0, 3, 5),
        creature(2, P1, 2, 5).color(ColorSet::RED),
    );
    engine.state_mut().continuous_effects.push(self_effect(
        1,
        Layer::AbilityAdding,
        EffectPrimitive::AddProtectionColor(ColorSet::RED),
    ));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(
        marked(&engine, 1),
        0,
        "CR 702.16e: the red creature's damage to the protected creature is prevented"
    );
    assert_eq!(
        marked(&engine, 2),
        3,
        "the protected creature still deals its damage"
    );
    assert!(
        !fizzled(&engine),
        "the protection was there when targeting, so the cast was legal"
    );
}

#[test]
fn a_damage_prevention_effect_stops_one_direction_only() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 5), creature(2, P1, 2, 5));
    engine
        .state_mut()
        .replacement_effects
        .push(prevent_damage_to(1));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 1), 0, "damage to creature 1 is prevented");
    assert_eq!(marked(&engine, 2), 3, "damage to creature 2 is not");
}

#[test]
fn deals_damage_and_is_dealt_damage_triggers_fire_for_fight_damage() {
    let mut engine = prey_upon_engine(
        creature(1, P0, 2, 5).script(DEALS_DAMAGE_GAIN_LIFE),
        creature(2, P1, 2, 5).script(IS_DEALT_DAMAGE_GAIN_LIFE),
    );
    let (p0_before, p1_before) = (life(&engine, P0), life(&engine, P1));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);
    pass_until_stack_empty(&mut engine);

    assert_eq!(life(&engine, P0), p0_before + 1, "creature 1 dealt damage");
    assert_eq!(
        life(&engine, P1),
        p1_before + 1,
        "creature 2 was dealt damage"
    );
}

#[test]
fn targets_in_the_wrong_word_order_are_rejected_at_cast_time() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));

    assert!(
        cast(&mut engine, vec![obj(2), obj(1)]).is_err(),
        "the first word wants a creature you control"
    );
}

#[test]
fn a_single_word_count_two_fight_rejects_the_same_creature_twice() {
    let mut engine = engine_with(
        vec![creature(1, P1, 3, 5)],
        Some(instant(AbilityEffect::Fight, count_two_word())),
    );

    assert!(
        cast(&mut engine, vec![obj(1), obj(1)]).is_err(),
        "CR 115.3: one word cannot pick the same creature twice"
    );
}

#[test]
fn nothing_happens_when_your_creature_leaves_before_resolution() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    remove_from_battlefield(&mut engine, 1);
    pass_both(&mut engine);

    assert!(
        damage_events(&engine).is_empty(),
        "CR 701.14b: neither fights"
    );
    assert_eq!(marked(&engine, 2), 0);
    assert!(!fizzled(&engine), "the other target is still legal");
}

#[test]
fn nothing_happens_when_the_opposing_creature_leaves_before_resolution() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    remove_from_battlefield(&mut engine, 2);
    pass_both(&mut engine);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(
        marked(&engine, 1),
        0,
        "the surviving creature takes nothing"
    );
}

#[test]
fn a_target_that_gains_hexproof_after_targeting_stops_the_whole_fight() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    engine.state_mut().continuous_effects.push(self_effect(
        2,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::HEXPROOF),
    ));
    pass_both(&mut engine);

    assert!(
        damage_events(&engine).is_empty(),
        "an illegal target means neither creature fights"
    );
    assert_eq!(marked(&engine, 1), 0);
}

#[test]
fn a_target_that_gains_protection_from_the_spell_after_targeting_stops_the_whole_fight() {
    let spell = CardDefinition {
        color_indicator: Some(ColorSet::GREEN),
        ..instant(AbilityEffect::Fight, two_words())
    };
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 3), creature(2, P1, 2, 2)],
        Some(spell),
    );
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    engine.state_mut().continuous_effects.push(self_effect(
        2,
        Layer::AbilityAdding,
        EffectPrimitive::AddProtectionColor(ColorSet::GREEN),
    ));
    pass_both(&mut engine);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(marked(&engine, 1), 0);
}

#[test]
fn the_spell_fizzles_when_every_target_is_illegal() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    remove_from_battlefield(&mut engine, 1);
    remove_from_battlefield(&mut engine, 2);
    pass_both(&mut engine);

    assert!(fizzled(&engine), "CR 608.2b");
    assert!(damage_events(&engine).is_empty());
}

#[test]
fn a_target_that_stops_being_a_creature_stops_the_fight() {
    let mut engine = prey_upon_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    engine.state_mut().continuous_effects.push(self_effect(
        2,
        Layer::TypeChanging,
        EffectPrimitive::SetCardTypes(CardTypeSet::LAND),
    ));
    pass_both(&mut engine);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(marked(&engine, 1), 0);
}

#[test]
fn a_self_source_fight_does_nothing_when_the_source_stops_being_a_creature() {
    let mut engine = engine_with(
        vec![
            creature(1, P0, 3, 3).script(FIGHT_ABILITY),
            creature(2, P1, 2, 2),
        ],
        None,
    );
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: ObjectId(1),
                ability_index: 0,
                targets: vec![obj(2)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(&mut engine);
    engine.state_mut().continuous_effects.push(self_effect(
        1,
        Layer::TypeChanging,
        EffectPrimitive::SetCardTypes(CardTypeSet::LAND),
    ));
    pass_both(&mut engine);

    assert!(
        damage_events(&engine).is_empty(),
        "CR 701.14b: the source is no longer a creature"
    );
    assert_eq!(marked(&engine, 2), 0);
}

#[test]
fn a_self_source_fight_does_nothing_when_the_source_has_left() {
    let mut engine = engine_with(
        vec![
            creature(1, P0, 3, 3).script(FIGHT_ABILITY),
            creature(2, P1, 2, 2),
        ],
        None,
    );
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: ObjectId(1),
                ability_index: 0,
                targets: vec![obj(2)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(&mut engine);
    remove_from_battlefield(&mut engine, 1);
    pass_both(&mut engine);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(marked(&engine, 2), 0);
}

#[test]
fn bite_deals_damage_to_the_target_and_the_biter_takes_none() {
    let mut engine = bite_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert!(in_graveyard(&engine, 2));
    assert_eq!(marked(&engine, 1), 0, "a bite is not a fight");
    assert_eq!(
        damage_events(&engine),
        vec![(1, 2, 3, false)],
        "one event, the biter as source"
    );
}

#[test]
fn bite_with_deathtouch_and_lifelink_works_like_any_damage_from_the_creature() {
    let mut engine = bite_engine(
        creature(1, P0, 1, 1).keywords(KeywordSet::DEATHTOUCH | KeywordSet::LIFELINK),
        creature(2, P1, 2, 9),
    );
    let before = life(&engine, P0);
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert!(in_graveyard(&engine, 2));
    assert_eq!(life(&engine, P0), before + 1);
    assert!(alive(&engine, 1));
}

#[test]
fn bite_from_a_creature_that_left_does_nothing() {
    let mut engine = bite_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    remove_from_battlefield(&mut engine, 1);
    pass_both(&mut engine);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(marked(&engine, 2), 0);
    assert!(!fizzled(&engine));
}

#[test]
fn bite_at_a_target_that_left_does_nothing() {
    let mut engine = bite_engine(creature(1, P0, 3, 3), creature(2, P1, 2, 2));
    cast(&mut engine, vec![obj(1), obj(2)]).unwrap();
    remove_from_battlefield(&mut engine, 2);
    pass_both(&mut engine);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(marked(&engine, 1), 0);
}

#[test]
fn a_zero_power_biter_deals_no_damage_and_emits_no_event() {
    let mut engine = bite_engine(creature(1, P0, 0, 3), creature(2, P1, 2, 2));
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert!(damage_events(&engine).is_empty());
    assert_eq!(marked(&engine, 2), 0);
}

#[test]
fn an_ability_source_bites_its_target() {
    let mut engine = engine_with(
        vec![
            creature(1, P0, 3, 3).script(BITE_ABILITY),
            creature(2, P1, 2, 2),
        ],
        None,
    );
    activate_and_resolve(&mut engine, 1, vec![obj(2)]);

    assert_eq!(damage_events(&engine), vec![(1, 2, 3, false)]);
    assert_eq!(marked(&engine, 1), 0);
    assert!(in_graveyard(&engine, 2));
}

#[test]
fn a_count_two_word_bite_does_nothing() {
    let mut engine = engine_with(
        vec![creature(1, P1, 3, 5), creature(2, P1, 2, 5)],
        Some(instant(AbilityEffect::Bite, count_two_word())),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert!(
        damage_events(&engine).is_empty(),
        "a count-2 word has no dealer"
    );
    assert!(!fizzled(&engine));
}

#[test]
fn a_creature_that_bites_itself_takes_its_power_once() {
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 9)],
        Some(instant(AbilityEffect::Bite, same_filter_two_words())),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(1)]);

    assert_eq!(
        damage_events(&engine),
        vec![(1, 1, 3, false)],
        "bite is one damage instance, never doubled"
    );
}

#[test]
fn fight_and_bite_decode_as_bare_tags() {
    let mut runtime = ScriptRuntime::new();
    let (fight, _) = runtime
        .load_operation("return Game.Ops.Fight")
        .expect("Fight loads");
    let (bite, _) = runtime
        .load_operation("return Game.Ops.Bite")
        .expect("Bite loads");
    assert!(matches!(fight, Some(AbilityEffect::Fight)), "got {fight:?}");
    assert!(matches!(bite, Some(AbilityEffect::Bite)), "got {bite:?}");
}

#[test]
fn calling_fight_or_bite_like_a_function_is_an_error() {
    let mut runtime = ScriptRuntime::new();
    assert!(runtime.load_operation("return Game.Ops.Fight()").is_err());
    assert!(runtime.load_operation("return Game.Ops.Bite()").is_err());
}

#[test]
fn an_array_target_requirement_builds_a_two_word_multiple_spec() {
    let runtime = ScriptRuntime::new();
    let spec = runtime
        .build_target_requirement(PREY_UPON)
        .expect("script loads")
        .expect("declares targets");
    let TargetSpec::Multiple(reqs) = spec else {
        panic!("expected Multiple, got {spec:?}")
    };
    assert_eq!(reqs.len(), 2);
    let controllers: Vec<_> = reqs
        .iter()
        .map(|r| match &r.filter {
            TargetFilter::Object {
                controller,
                card_types,
                ..
            } => {
                assert_eq!(*card_types, Some(CardTypeSet::CREATURE));
                *controller
            }
            other => panic!("expected an object filter, got {other:?}"),
        })
        .collect();
    assert_eq!(
        controllers,
        vec![
            Some(FilterController::You),
            Some(FilterController::Opponent)
        ]
    );
    assert!(reqs.iter().all(|r| r.count == 1 && !r.up_to && !r.at_least));
}

#[test]
fn a_table_with_count_still_builds_a_single_requirement() {
    let runtime = ScriptRuntime::new();
    let source = r#"
local s = {}
function s.target_requirement()
    return { card_types = { "Creature" }, count = 2 }
end
return s
"#;
    let spec = runtime.build_target_requirement(source).unwrap().unwrap();
    assert!(
        matches!(spec, TargetSpec::Primitive(ref r) if r.count == 2),
        "got {spec:?}"
    );
}

#[test]
fn a_one_entry_array_is_the_same_single_word_as_a_bare_table() {
    let runtime = ScriptRuntime::new();
    let source = r#"
local s = {}
function s.target_requirement()
    return { { card_types = { "Creature" }, count = 1 } }
end
return s
"#;
    let spec = runtime.build_target_requirement(source).unwrap().unwrap();
    assert!(matches!(spec, TargetSpec::Primitive(_)), "got {spec:?}");
}

#[test]
fn malformed_target_requirement_returns_are_clear_errors() {
    let runtime = ScriptRuntime::new();
    let build = |body: &str| {
        let source = format!(
            "local s = {{}}\nfunction s.target_requirement()\n    return {body}\nend\nreturn s\n"
        );
        runtime.build_target_requirement(&source)
    };

    let empty = build("{}").unwrap_err();
    assert!(format!("{empty:?}").contains("empty"), "got {empty:?}");

    let mixed = build(r#"{ { count = 1 }, count = 1 }"#).unwrap_err();
    assert!(format!("{mixed:?}").contains("mixes"), "got {mixed:?}");

    let non_table = build(r#"{ { count = 1 }, "creature" }"#).unwrap_err();
    assert!(
        format!("{non_table:?}").contains("entry 2"),
        "got {non_table:?}"
    );

    let gap = build(r#"{ [1] = { count = 1 }, [3] = { count = 1 } }"#).unwrap_err();
    assert!(
        format!("{gap:?}").contains("mixes") || format!("{gap:?}").contains("gaps"),
        "got {gap:?}"
    );
}

fn scripted_spell(source: &str) -> CardDefinition {
    let runtime = ScriptRuntime::new();
    let target_requirement = runtime
        .build_target_requirement(source)
        .expect("target_requirement loads");
    let (effect, _) = runtime
        .build_operation(source)
        .expect("operation loads")
        .expect("declares an operation");
    CardDefinition {
        target_requirement,
        effect,
        ..minimal_card_definition(
            98,
            "Scripted Spell",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

#[test]
fn a_script_declared_prey_upon_casts_and_resolves_end_to_end() {
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 3), creature(2, P1, 2, 2)],
        Some(scripted_spell(PREY_UPON)),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 1), 2);
    assert!(in_graveyard(&engine, 2));
}

#[test]
fn a_script_declared_bite_casts_and_resolves_end_to_end() {
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 3), creature(2, P1, 2, 2)],
        Some(scripted_spell(BITE_SPELL)),
    );
    cast_and_resolve(&mut engine, vec![obj(1), obj(2)]);

    assert_eq!(marked(&engine, 1), 0);
    assert!(in_graveyard(&engine, 2));
}

#[test]
fn a_script_declared_prey_upon_rejects_two_creatures_you_control() {
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 3), creature(2, P0, 2, 2)],
        Some(scripted_spell(PREY_UPON)),
    );

    assert!(
        cast(&mut engine, vec![obj(1), obj(2)]).is_err(),
        "the second word wants a creature you don't control"
    );
}

#[test]
fn a_two_word_fight_can_be_one_mode_of_a_multi_mode_spell() {
    let definition = CardDefinition {
        modes: vec![
            SpellMode {
                target_requirement: Some(two_words()),
                effect: Some(AbilityEffect::Fight),
                additional_cost: None,
                pawprint_weight: 0,
            },
            SpellMode {
                target_requirement: None,
                effect: Some(AbilityEffect::GainLife(3)),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        mode_choice: ModeChoice::Exactly(2),
        ..minimal_card_definition(
            97,
            "Test Modal",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    };
    let mut engine = engine_with(
        vec![creature(1, P0, 3, 3), creature(2, P1, 2, 2)],
        Some(definition),
    );
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: SPELL,
                modes: vec![0, 1],
                targets: vec![obj(1), obj(2)],
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "a mode with its own two target words combines now: {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(&mut engine);
    pass_both(&mut engine);

    assert_eq!(marked(&engine, 1), 2, "the 3/3 takes the 2/2's power");
    assert!(in_graveyard(&engine, 2), "the 2/2 takes 3 and dies");
    assert_eq!(life(&engine, P0), 23, "the other mode ran too");
    assert!(!fizzled(&engine));
}
