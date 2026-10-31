//! CR 110.2a, 400.7, 400.4a, 614.12, 303.4f: `Game.Ops.ReturnToBattlefield` and `ReturnAllToBattlefield` return cards from a graveyard or exile through `ScriptedZoneMoveKind::Reanimate`, so entry is replacement-checked and gets the treatment a resolving permanent spell gets.
//! The graveyard object's `controller` is written temporarily so entry replacement filters see the entering player, and several tests check it returns to the owner on every path that does not enter.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const WARD: u64 = 900;

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

const ENTERS_TAPPED: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

const ENTERS_WITH_TWO_COUNTERS: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCounters("PlusOnePlusOne", 2))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

const BATTLEFIELD_ACTIVATED: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

const GRAVEYARD_ONLY_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Graveyard")
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

struct Placed {
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    subtypes: &'static [&'static str],
    script: Option<&'static str>,
    owner: PlayerId,
    zone: ZoneKind,
}

fn placed(
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    owner: PlayerId,
    zone: ZoneKind,
) -> Placed {
    Placed {
        id,
        name,
        types,
        subtypes: &[],
        script: None,
        owner,
        zone,
    }
}

fn bear(id: u64, owner: PlayerId, zone: ZoneKind) -> Placed {
    placed(id, "Bear", CardTypeSet::CREATURE, owner, zone)
}

fn creature_filter() -> TargetFilter {
    object_filter(Some(CardTypeSet::CREATURE), ZoneKind::Graveyard)
}

fn object_filter(card_types: Option<CardTypeSet>, zone: ZoneKind) -> TargetFilter {
    TargetFilter::Object {
        mana_value: None,
        excluded_card_types: None,
        excluded_subtypes: None,
        card_types,
        subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone: Some(zone),
    }
}

fn definition(p: &Placed) -> CardDefinition {
    let creature = p.types.contains(CardTypeSet::CREATURE);
    let is_aura = p.subtypes.contains(&"Aura");
    CardDefinition {
        script_ref: p
            .script
            .map(|s| ScriptReference::Inline(s.to_string()))
            .unwrap_or(ScriptReference::ByKey(p.name.to_string())),
        power: creature.then_some(PTValue::Fixed(2)),
        toughness: creature.then_some(PTValue::Fixed(2)),
        subtypes: p.subtypes.iter().map(|s| Subtype(s.to_string())).collect(),
        target_requirement: is_aura.then_some(TargetSpec::Primitive(TargetRequirement {
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
        })),
        ..minimal_card_definition(
            p.id,
            p.name,
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            p.types,
        )
    }
}

fn spell(effect: AbilityEffect, target: Option<TargetFilter>) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: target.map(|filter| {
            TargetSpec::Primitive(TargetRequirement {
                filter,
                count: 1,
                up_to: false,
                at_least: false,
                exclude_source: false,
            })
        }),
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

fn engine_with(cards: Vec<Placed>, spell_def: Option<CardDefinition>) -> Engine {
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
    for c in cards {
        let object = card_object(ObjectId(c.id), definition(&c), c.owner, c.zone);
        state.objects.insert(ObjectId(c.id), object);
        let owner = if matches!(c.zone, ZoneKind::Battlefield | ZoneKind::Exile) {
            None
        } else {
            Some(c.owner)
        };
        state
            .zone_mut(c.zone, owner)
            .unwrap()
            .push_bottom(ObjectId(c.id));
        if c.zone == ZoneKind::Battlefield && c.script.is_some() {
            let def = state.objects[&ObjectId(c.id)].definition.clone();
            runtime
                .run_card_script(&mut state, &def, ObjectId(c.id))
                .expect("script registers");
        }
    }
    if let Some(def) = spell_def {
        state
            .objects
            .insert(SPELL, card_object(SPELL, def, P0, ZoneKind::Hand));
        state
            .zone_mut(ZoneKind::Hand, Some(P0))
            .unwrap()
            .push_bottom(SPELL);
    }
    Engine::new(state)
}

fn return_to_battlefield(control: EntryControl, tapped: bool) -> AbilityEffect {
    AbilityEffect::ReturnToBattlefield { control, tapped }
}

fn creature_effect_filter(controller: Option<FilterController>) -> EffectFilter {
    EffectFilter {
        controller,
        card_types: Some(CardTypeSet::CREATURE),
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        self_only: false,
    }
}

fn return_all(
    zone: ZoneKind,
    controller: Option<FilterController>,
    control: EntryControl,
) -> AbilityEffect {
    AbilityEffect::ReturnAllToBattlefield {
        filter: creature_effect_filter(controller),
        zone,
        control,
        tapped: false,
    }
}

fn engine_targeted(cards: Vec<Placed>, control: EntryControl, tapped: bool) -> Engine {
    engine_with(
        cards,
        Some(spell(
            return_to_battlefield(control, tapped),
            Some(creature_filter()),
        )),
    )
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    let mut status = StepStatus::Progressed;
    for player in [P0, P1] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        status = drain(engine);
    }
    status
}

fn cast(engine: &mut Engine, targets: Vec<Target>) {
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
        .expect("p0 holds priority");
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
}

fn cast_and_resolve(engine: &mut Engine, targets: Vec<Target>) -> StepStatus {
    cast(engine, targets);
    pass_both(engine)
}

fn reanimate(engine: &mut Engine, target: u64) -> StepStatus {
    cast_and_resolve(engine, vec![Target::Object(ObjectId(target))])
}

fn named(engine: &Engine, name: &str, zone: ZoneKind) -> Option<GameObject> {
    engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == name && o.zone == zone)
        .cloned()
}

fn the(engine: &Engine, name: &str, zone: ZoneKind) -> GameObject {
    named(engine, name, zone).unwrap_or_else(|| panic!("no {name} in {zone:?}"))
}

fn names_in(engine: &Engine, zone: ZoneKind, owner: Option<PlayerId>) -> Vec<String> {
    let mut names: Vec<String> = engine
        .state()
        .zone(zone, owner)
        .unwrap()
        .contents
        .iter()
        .filter_map(|id| engine.state().object(*id))
        .map(|o| o.definition.name.clone())
        .collect();
    names.sort();
    names
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn ward_to(source: u64, destination: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: creature_effect_filter(Some(FilterController::Opponent)),
            },
            payload: ReplacementPayload::ReplaceZoneDestination(destination),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn ward(id: u64) -> Placed {
    placed(
        id,
        "Ward",
        CardTypeSet::ENCHANTMENT,
        P1,
        ZoneKind::Battlefield,
    )
}

fn orb_enters_tapped() -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(WARD),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: false,
                },
            },
            payload: ReplacementPayload::EntersTapped,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn a_creature_card_returns_from_your_graveyard_to_the_battlefield() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    reanimate(&mut engine, 1);

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Bear"]);
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Test Spell"]
    );
    let bear = the(&engine, "Bear", ZoneKind::Battlefield);
    assert_eq!((bear.owner, bear.controller), (P0, P0));
    assert!(!bear.tapped);
}

#[test]
fn the_returned_card_is_a_new_object_with_the_old_id_as_its_lineage() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    reanimate(&mut engine, 1);

    let bear = the(&engine, "Bear", ZoneKind::Battlefield);
    assert_ne!(bear.object_id, ObjectId(1), "CR 400.7: fresh id");
    assert_eq!(bear.lineage.previous, Some(ObjectId(1)));
    assert!(
        engine.state().object(ObjectId(1)).is_none(),
        "the old graveyard id is gone"
    );
}

#[test]
fn an_opponents_graveyard_card_enters_under_your_control_by_default() {
    let mut engine = engine_targeted(
        vec![bear(1, P1, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    reanimate(&mut engine, 1);

    let bear = the(&engine, "Bear", ZoneKind::Battlefield);
    assert_eq!(bear.owner, P1, "CR 110.2: the owner never changes");
    assert_eq!(bear.controller, P0, "CR 110.2a: the effect's controller");
}

#[test]
fn owner_control_puts_the_card_under_its_owners_control() {
    let mut engine = engine_targeted(
        vec![bear(1, P1, ZoneKind::Graveyard)],
        EntryControl::Owner,
        false,
    );
    reanimate(&mut engine, 1);

    let bear = the(&engine, "Bear", ZoneKind::Battlefield);
    assert_eq!((bear.owner, bear.controller), (P1, P1));
}

#[test]
fn the_returned_permanent_is_summoning_sick() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    let turn = engine.state().turn_number;
    reanimate(&mut engine, 1);

    let bear = the(&engine, "Bear", ZoneKind::Battlefield);
    assert_eq!(bear.controlled_since_turn, turn, "CR 302.6");
}

#[test]
fn prior_battlefield_state_does_not_carry_over() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    {
        let old = engine.state_mut().objects.get_mut(&ObjectId(1)).unwrap();
        old.tapped = true;
        old.damage_marked = 3;
        old.counters
            .add(CounterKind::PlusOnePlusOne, 2, SequenceNo(0));
    }
    reanimate(&mut engine, 1);

    let bear = the(&engine, "Bear", ZoneKind::Battlefield);
    assert!(!bear.tapped);
    assert_eq!(bear.damage_marked, 0);
    assert_eq!(bear.counters.count(&CounterKind::PlusOnePlusOne), 0);
    assert!(bear.attached_to.is_none() && bear.attachments.is_empty());
    assert!(!bear.entered_via_escape);
}

#[test]
fn a_target_that_does_not_match_the_filter_is_rejected_at_cast() {
    let mut engine = engine_targeted(
        vec![placed(
            1,
            "Forest",
            CardTypeSet::LAND,
            P0,
            ZoneKind::Graveyard,
        )],
        EntryControl::You,
        false,
    );
    let submitted = engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::CastSpell {
            card: SPELL,
            modes: Vec::new(),
            targets: vec![Target::Object(ObjectId(1))],
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    });
    let status = drain(&mut engine);
    assert!(
        submitted.is_err() || !matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Forest"]
    );
}

#[test]
fn a_card_in_exile_can_be_returned() {
    let mut engine = engine_with(
        vec![bear(1, P0, ZoneKind::Exile)],
        Some(spell(
            return_to_battlefield(EntryControl::You, false),
            Some(object_filter(Some(CardTypeSet::CREATURE), ZoneKind::Exile)),
        )),
    );
    reanimate(&mut engine, 1);

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Bear"]);
    assert!(names_in(&engine, ZoneKind::Exile, None).is_empty());
}

#[test]
fn a_target_that_left_the_graveyard_in_response_is_not_returned() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    cast(&mut engine, vec![Target::Object(ObjectId(1))]);
    // The card is exiled before the spell resolves.
    engine
        .state_mut()
        .zone_mut(ZoneKind::Graveyard, Some(P0))
        .unwrap()
        .remove(ObjectId(1));
    engine
        .state_mut()
        .zone_mut(ZoneKind::Exile, None)
        .unwrap()
        .push_bottom(ObjectId(1));
    engine
        .state_mut()
        .objects
        .get_mut(&ObjectId(1))
        .unwrap()
        .zone = ZoneKind::Exile;
    pass_both(&mut engine);

    assert!(
        names_in(&engine, ZoneKind::Battlefield, None).is_empty(),
        "CR 608.2b: the target is illegal"
    );
    assert_eq!(names_in(&engine, ZoneKind::Exile, None), vec!["Bear"]);
}

#[test]
fn an_instant_card_stays_in_the_graveyard() {
    let mut engine = engine_with(
        vec![placed(
            1,
            "Bolt",
            CardTypeSet::INSTANT,
            P1,
            ZoneKind::Graveyard,
        )],
        Some(spell(
            return_to_battlefield(EntryControl::You, false),
            Some(object_filter(None, ZoneKind::Graveyard)),
        )),
    );
    reanimate(&mut engine, 1);

    assert!(
        names_in(&engine, ZoneKind::Battlefield, None).is_empty(),
        "CR 400.4a"
    );
    let bolt = the(&engine, "Bolt", ZoneKind::Graveyard);
    assert_eq!(
        bolt.object_id,
        ObjectId(1),
        "nothing moved, so no new object"
    );
    assert_eq!(
        bolt.controller, bolt.owner,
        "CR 108.4a: no temporary controller left behind"
    );
}

#[test]
fn a_sorcery_card_stays_in_the_graveyard_too() {
    let mut engine = engine_with(
        vec![placed(
            1,
            "Rite",
            CardTypeSet::SORCERY,
            P0,
            ZoneKind::Graveyard,
        )],
        Some(spell(
            return_to_battlefield(EntryControl::You, false),
            Some(object_filter(None, ZoneKind::Graveyard)),
        )),
    );
    reanimate(&mut engine, 1);

    assert!(names_in(&engine, ZoneKind::Battlefield, None).is_empty());
    assert!(names_in(&engine, ZoneKind::Graveyard, Some(P0)).contains(&"Rite".to_string()));
}

#[test]
fn the_tapped_option_enters_the_permanent_tapped() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        true,
    );
    reanimate(&mut engine, 1);

    assert!(the(&engine, "Bear", ZoneKind::Battlefield).tapped);
}

#[test]
fn an_external_enters_tapped_effect_applies() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard), ward(WARD)],
        EntryControl::You,
        false,
    );
    engine
        .state_mut()
        .replacement_effects
        .push(orb_enters_tapped());
    reanimate(&mut engine, 1);

    assert!(
        the(&engine, "Bear", ZoneKind::Battlefield).tapped,
        "CR 614.12: Orb of Dreams style"
    );
}

#[test]
fn a_cards_own_enters_tapped_ability_applies() {
    let mut card = bear(1, P0, ZoneKind::Graveyard);
    card.script = Some(ENTERS_TAPPED);
    let mut engine = engine_targeted(vec![card], EntryControl::You, false);
    reanimate(&mut engine, 1);

    assert!(
        the(&engine, "Bear", ZoneKind::Battlefield).tapped,
        "CR 614.12: Scarwood Treefolk from a graveyard"
    );
}

#[test]
fn a_cards_own_enters_with_counters_ability_applies_exactly_once() {
    let mut card = bear(1, P0, ZoneKind::Graveyard);
    card.script = Some(ENTERS_WITH_TWO_COUNTERS);
    let mut engine = engine_targeted(vec![card], EntryControl::You, false);
    reanimate(&mut engine, 1);

    assert_eq!(
        the(&engine, "Bear", ZoneKind::Battlefield)
            .counters
            .count(&CounterKind::PlusOnePlusOne),
        2
    );
}

#[test]
fn an_entry_redirect_sends_the_card_to_exile_instead() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard), ward(WARD)],
        EntryControl::You,
        false,
    );
    engine
        .state_mut()
        .replacement_effects
        .push(ward_to(WARD, ZoneKind::Exile));
    reanimate(&mut engine, 1);

    assert_eq!(
        names_in(&engine, ZoneKind::Battlefield, None),
        vec!["Ward"],
        "the Bear never entered"
    );
    let exiled = the(&engine, "Bear", ZoneKind::Exile);
    assert_eq!(
        exiled.controller, exiled.owner,
        "CR 108.4a: the temporary controller is undone after a redirect"
    );
    assert!(
        engine
            .state()
            .abilities
            .iter()
            .all(|a| a.source != exiled.object_id)
    );
}

#[test]
fn a_redirect_back_to_the_graveyard_leaves_the_controller_as_the_owner() {
    // The card is P1's and reanimated under P0's control, so the ward sees an opponent's creature entering.
    let mut engine = engine_targeted(
        vec![bear(1, P1, ZoneKind::Graveyard), ward(WARD)],
        EntryControl::You,
        false,
    );
    engine
        .state_mut()
        .replacement_effects
        .push(ward_to(WARD, ZoneKind::Graveyard));
    reanimate(&mut engine, 1);

    assert!(names_in(&engine, ZoneKind::Battlefield, None) == vec!["Ward"]);
    let back = the(&engine, "Bear", ZoneKind::Graveyard);
    assert_eq!((back.owner, back.controller), (P1, P1));
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(P1))
            .unwrap()
            .contents
            .contains(&back.object_id)
    );
}

#[test]
fn the_entry_filter_sees_the_player_the_card_enters_under() {
    // The ward is P1's and hits creatures entering under an opponent of P1: P1's Bear enters under P0 with `You` (redirected) and under P1 with `Owner` (untouched).
    let mut you = engine_targeted(
        vec![bear(1, P1, ZoneKind::Graveyard), ward(WARD)],
        EntryControl::You,
        false,
    );
    you.state_mut()
        .replacement_effects
        .push(ward_to(WARD, ZoneKind::Exile));
    reanimate(&mut you, 1);
    assert!(
        named(&you, "Bear", ZoneKind::Exile).is_some(),
        "entering under P0 is redirected"
    );

    let mut owner = engine_targeted(
        vec![bear(1, P1, ZoneKind::Graveyard), ward(WARD)],
        EntryControl::Owner,
        false,
    );
    owner
        .state_mut()
        .replacement_effects
        .push(ward_to(WARD, ZoneKind::Exile));
    reanimate(&mut owner, 1);
    assert!(
        named(&owner, "Bear", ZoneKind::Battlefield).is_some(),
        "entering under P1 is not"
    );
}

#[test]
fn two_competing_redirects_pause_for_an_ordering_choice_and_resume() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard), ward(WARD), ward(WARD + 1)],
        EntryControl::You,
        false,
    );
    engine
        .state_mut()
        .replacement_effects
        .push(ward_to(WARD, ZoneKind::Exile));
    engine
        .state_mut()
        .replacement_effects
        .push(ward_to(WARD + 1, ZoneKind::Graveyard));
    let status = reanimate(&mut engine, 1);

    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("expected a CR 616.1 ordering choice, got {status:?}");
    };
    assert_eq!(player, P0, "the entering player chooses");
    assert_eq!(candidates.len(), 2);

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(WARD)))
        .unwrap();
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == P0),
        "{status:?}"
    );

    let exiled = the(&engine, "Bear", ZoneKind::Exile);
    assert_eq!(
        exiled.controller, exiled.owner,
        "the temporary controller is undone after the answered pause"
    );
    assert_eq!(
        names_in(&engine, ZoneKind::Battlefield, None),
        vec!["Ward", "Ward"]
    );
}

#[test]
fn the_cards_own_enters_trigger_fires() {
    let mut card = bear(1, P0, ZoneKind::Graveyard);
    card.script = Some(ENTERS_GAIN_LIFE);
    let mut engine = engine_targeted(vec![card], EntryControl::You, false);
    let before = life(&engine, P0);
    reanimate(&mut engine, 1);
    assert_eq!(
        engine.state().stack.len(),
        1,
        "the enters trigger is on the stack"
    );
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), before + 1);
}

#[test]
fn the_ledger_records_the_graveyard_to_battlefield_zone_change() {
    let mut engine = engine_targeted(
        vec![bear(1, P0, ZoneKind::Graveyard)],
        EntryControl::You,
        false,
    );
    reanimate(&mut engine, 1);

    let new_id = the(&engine, "Bear", ZoneKind::Battlefield).object_id;
    let found = engine.state().ledger.iter().any(|e| {
        matches!(&e.kind, GameEventKind::ZoneChange { object, from: ZoneKind::Graveyard, to: ZoneKind::Battlefield, last_known_info: Some(lki) }
            if *object == new_id && lki.object_id == ObjectId(1))
    });
    assert!(found);
}

#[test]
fn the_cards_battlefield_abilities_register_on_entry() {
    let mut card = bear(1, P0, ZoneKind::Graveyard);
    card.script = Some(BATTLEFIELD_ACTIVATED);
    let mut engine = engine_targeted(vec![card], EntryControl::You, false);
    reanimate(&mut engine, 1);

    let new_id = the(&engine, "Bear", ZoneKind::Battlefield).object_id;
    assert_eq!(
        engine
            .state()
            .abilities
            .iter()
            .filter(|a| a.source == new_id)
            .count(),
        1
    );
}

#[test]
fn a_graveyard_only_ability_does_not_follow_the_card_onto_the_battlefield() {
    let mut card = bear(1, P0, ZoneKind::Graveyard);
    card.script = Some(GRAVEYARD_ONLY_ABILITY);
    let mut engine = engine_targeted(vec![card], EntryControl::You, false);
    reanimate(&mut engine, 1);

    assert!(
        engine.state().abilities.is_empty(),
        "the graveyard ability left with the old object"
    );
}

fn aura(id: u64, owner: PlayerId) -> Placed {
    Placed {
        subtypes: &["Aura"],
        ..placed(
            id,
            "Aura",
            CardTypeSet::ENCHANTMENT,
            owner,
            ZoneKind::Graveyard,
        )
    }
}

fn engine_for_aura(cards: Vec<Placed>) -> Engine {
    engine_with(
        cards,
        Some(spell(
            return_to_battlefield(EntryControl::You, false),
            Some(object_filter(
                Some(CardTypeSet::ENCHANTMENT),
                ZoneKind::Graveyard,
            )),
        )),
    )
}

#[test]
fn an_aura_with_one_legal_host_enters_attached_to_it() {
    let mut engine = engine_for_aura(vec![aura(1, P0), bear(2, P1, ZoneKind::Battlefield)]);
    reanimate(&mut engine, 1);

    let aura = the(&engine, "Aura", ZoneKind::Battlefield);
    assert_eq!(aura.attached_to, Some(ObjectId(2)), "CR 303.4f");
    assert!(
        engine
            .state()
            .object(ObjectId(2))
            .unwrap()
            .attachments
            .contains(&aura.object_id)
    );
}

#[test]
fn an_aura_with_no_legal_host_stays_in_the_graveyard() {
    let mut engine = engine_for_aura(vec![aura(1, P1)]);
    reanimate(&mut engine, 1);

    assert!(
        names_in(&engine, ZoneKind::Battlefield, None).is_empty(),
        "CR 303.4g"
    );
    let aura = the(&engine, "Aura", ZoneKind::Graveyard);
    assert_eq!(
        (aura.owner, aura.controller),
        (P1, P1),
        "the temporary controller is undone"
    );
}

#[test]
fn an_aura_with_several_legal_hosts_stays_in_the_graveyard_until_the_choice_exists() {
    // TODO: the controller should choose, but no entry-attach pause exists yet (CR 303.4f)
    let mut engine = engine_for_aura(vec![
        aura(1, P0),
        bear(2, P0, ZoneKind::Battlefield),
        bear(3, P1, ZoneKind::Battlefield),
    ]);
    reanimate(&mut engine, 1);

    assert!(named(&engine, "Aura", ZoneKind::Battlefield).is_none());
    let aura = the(&engine, "Aura", ZoneKind::Graveyard);
    assert_eq!(aura.controller, aura.owner);
}

#[test]
fn equipment_enters_unattached() {
    let equipment = Placed {
        subtypes: &["Equipment"],
        ..placed(1, "Sword", CardTypeSet::ARTIFACT, P0, ZoneKind::Graveyard)
    };
    let mut engine = engine_with(
        vec![equipment, bear(2, P0, ZoneKind::Battlefield)],
        Some(spell(
            return_to_battlefield(EntryControl::You, false),
            Some(object_filter(
                Some(CardTypeSet::ARTIFACT),
                ZoneKind::Graveyard,
            )),
        )),
    );
    reanimate(&mut engine, 1);

    let sword = the(&engine, "Sword", ZoneKind::Battlefield);
    assert!(
        sword.attached_to.is_none(),
        "CR 303.4h-style: nothing attaches it"
    );
}

#[test]
fn return_all_returns_every_matching_card_from_every_graveyard() {
    let mut engine = engine_with(
        vec![
            bear(1, P0, ZoneKind::Graveyard),
            bear(2, P1, ZoneKind::Graveyard),
            placed(3, "Forest", CardTypeSet::LAND, P0, ZoneKind::Graveyard),
        ],
        Some(spell(
            return_all(ZoneKind::Graveyard, None, EntryControl::You),
            None,
        )),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(
        names_in(&engine, ZoneKind::Battlefield, None),
        vec!["Bear", "Bear"]
    );
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Forest", "Test Spell"]
    );
    assert!(names_in(&engine, ZoneKind::Graveyard, Some(P1)).is_empty());
}

#[test]
fn return_all_with_the_you_filter_takes_only_your_graveyard() {
    let mut engine = engine_with(
        vec![
            bear(1, P0, ZoneKind::Graveyard),
            bear(2, P1, ZoneKind::Graveyard),
        ],
        Some(spell(
            return_all(
                ZoneKind::Graveyard,
                Some(FilterController::You),
                EntryControl::You,
            ),
            None,
        )),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Bear"]);
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P1)),
        vec!["Bear"]
    );
}

#[test]
fn return_all_defaults_to_the_casters_control_and_owner_control_is_an_option() {
    let cards = || {
        vec![
            bear(1, P0, ZoneKind::Graveyard),
            bear(2, P1, ZoneKind::Graveyard),
        ]
    };
    let mut you = engine_with(
        cards(),
        Some(spell(
            return_all(ZoneKind::Graveyard, None, EntryControl::You),
            None,
        )),
    );
    cast_and_resolve(&mut you, Vec::new());
    assert!(
        you.state()
            .objects
            .values()
            .filter(|o| o.zone == ZoneKind::Battlefield)
            .all(|o| o.controller == P0)
    );

    let mut owner = engine_with(
        cards(),
        Some(spell(
            return_all(ZoneKind::Graveyard, None, EntryControl::Owner),
            None,
        )),
    );
    cast_and_resolve(&mut owner, Vec::new());
    assert!(
        owner
            .state()
            .objects
            .values()
            .filter(|o| o.zone == ZoneKind::Battlefield)
            .all(|o| o.controller == o.owner)
    );
}

#[test]
fn return_all_can_take_cards_from_exile() {
    let mut engine = engine_with(
        vec![
            bear(1, P0, ZoneKind::Exile),
            bear(2, P0, ZoneKind::Graveyard),
        ],
        Some(spell(
            return_all(ZoneKind::Exile, None, EntryControl::You),
            None,
        )),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Bear"]);
    assert!(names_in(&engine, ZoneKind::Exile, None).is_empty());
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Bear", "Test Spell"]
    );
}

#[test]
fn return_all_with_no_match_resolves_cleanly() {
    let mut engine = engine_with(
        vec![placed(
            1,
            "Forest",
            CardTypeSet::LAND,
            P0,
            ZoneKind::Graveyard,
        )],
        Some(spell(
            return_all(ZoneKind::Graveyard, None, EntryControl::You),
            None,
        )),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert!(engine.state().stack.is_empty());
    assert!(names_in(&engine, ZoneKind::Battlefield, None).is_empty());
}

#[test]
fn return_all_skips_instants_when_the_filter_allows_them() {
    let all_cards = EffectFilter {
        card_types: None,
        ..creature_effect_filter(None)
    };
    let mut engine = engine_with(
        vec![
            placed(1, "Bolt", CardTypeSet::INSTANT, P0, ZoneKind::Graveyard),
            bear(2, P0, ZoneKind::Graveyard),
        ],
        Some(spell(
            AbilityEffect::ReturnAllToBattlefield {
                filter: all_cards,
                zone: ZoneKind::Graveyard,
                control: EntryControl::You,
                tapped: false,
            },
            None,
        )),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Bear"]);
    let bolt = the(&engine, "Bolt", ZoneKind::Graveyard);
    assert_eq!(bolt.controller, bolt.owner);
}

#[test]
fn return_all_enters_each_card_tapped_with_the_option() {
    let mut engine = engine_with(
        vec![
            bear(1, P0, ZoneKind::Graveyard),
            bear(2, P1, ZoneKind::Graveyard),
        ],
        Some(spell(
            AbilityEffect::ReturnAllToBattlefield {
                filter: creature_effect_filter(None),
                zone: ZoneKind::Graveyard,
                control: EntryControl::You,
                tapped: true,
            },
            None,
        )),
    );
    cast_and_resolve(&mut engine, Vec::new());

    assert!(
        engine
            .state()
            .objects
            .values()
            .filter(|o| o.zone == ZoneKind::Battlefield)
            .all(|o| o.tapped)
    );
}

#[test]
fn return_all_applies_an_entry_redirect_to_each_card() {
    let mut engine = engine_with(
        vec![
            bear(1, P0, ZoneKind::Graveyard),
            bear(2, P0, ZoneKind::Graveyard),
            ward(WARD),
        ],
        Some(spell(
            return_all(ZoneKind::Graveyard, None, EntryControl::You),
            None,
        )),
    );
    engine
        .state_mut()
        .replacement_effects
        .push(ward_to(WARD, ZoneKind::Exile));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Ward"]);
    assert_eq!(
        names_in(&engine, ZoneKind::Exile, None),
        vec!["Bear", "Bear"]
    );
    assert!(
        engine
            .state()
            .objects
            .values()
            .filter(|o| o.zone == ZoneKind::Exile)
            .all(|o| o.controller == o.owner)
    );
}

#[test]
fn return_all_runs_each_cards_enters_trigger() {
    let mut a = bear(1, P0, ZoneKind::Graveyard);
    a.script = Some(ENTERS_GAIN_LIFE);
    let mut b = bear(2, P1, ZoneKind::Graveyard);
    b.script = Some(ENTERS_GAIN_LIFE);
    let mut engine = engine_with(
        vec![a, b],
        Some(spell(
            return_all(ZoneKind::Graveyard, None, EntryControl::You),
            None,
        )),
    );
    let before = life(&engine, P0);
    let status = cast_and_resolve(&mut engine, Vec::new());
    // Both triggers belong to P0, so P0 orders them (CR 603.3b).
    let StepStatus::AwaitingTriggerOrderChoice { group, .. } = status else {
        panic!("{status:?}")
    };
    assert_eq!(group.len(), 2);
    engine.submit_trigger_order(group).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);
    pass_both(&mut engine);
    pass_both(&mut engine);

    assert_eq!(
        life(&engine, P0),
        before + 2,
        "both trigger under P0's control"
    );
}

fn build(source: &str) -> Result<Option<AbilityEffect>, String> {
    let runtime = ScriptRuntime::new();
    runtime
        .build_operation(source)
        .map(|built| built.and_then(|(effect, _)| effect))
        .map_err(|e| format!("{e:?}"))
}

fn op(call: &str) -> String {
    format!("local s = {{}}\nfunction s.operation()\n    return {call}\nend\nreturn s\n")
}

#[test]
fn lua_return_to_battlefield_defaults_to_your_control_untapped() {
    match build(&op("Game.Ops.ReturnToBattlefield()")) {
        Ok(Some(AbilityEffect::ReturnToBattlefield {
            control: EntryControl::You,
            tapped: false,
        })) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_return_to_battlefield_reads_its_options() {
    match build(&op(
        r#"Game.Ops.ReturnToBattlefield({ control = "owner", tapped = true })"#,
    )) {
        Ok(Some(AbilityEffect::ReturnToBattlefield {
            control: EntryControl::Owner,
            tapped: true,
        })) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_rejects_a_bad_control_value_or_unknown_option() {
    assert!(
        build(&op(
            r#"Game.Ops.ReturnToBattlefield({ control = "Opponent" })"#
        ))
        .is_err()
    );
    assert!(build(&op(r#"Game.Ops.ReturnToBattlefield({ tapped = "yes" })"#)).is_err());
    assert!(build(&op(r#"Game.Ops.ReturnToBattlefield({ counters = 1 })"#)).is_err());
}

#[test]
fn lua_return_all_defaults_to_the_graveyard() {
    match build(&op(
        r#"Game.Ops.ReturnAllToBattlefield({ card_types = { "Creature" } })"#,
    )) {
        Ok(Some(AbilityEffect::ReturnAllToBattlefield {
            filter,
            zone: ZoneKind::Graveyard,
            control: EntryControl::You,
            tapped: false,
        })) => {
            assert_eq!(filter.card_types, Some(CardTypeSet::CREATURE));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_return_all_reads_zone_and_options() {
    let call =
        r#"Game.Ops.ReturnAllToBattlefield({}, "Exile", { control = "owner", tapped = true })"#;
    match build(&op(call)) {
        Ok(Some(AbilityEffect::ReturnAllToBattlefield {
            zone: ZoneKind::Exile,
            control: EntryControl::Owner,
            tapped: true,
            ..
        })) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_return_all_rejects_a_zone_it_cannot_source_from() {
    assert!(build(&op(r#"Game.Ops.ReturnAllToBattlefield({}, "Battlefield")"#)).is_err());
    assert!(build(&op(r#"Game.Ops.ReturnAllToBattlefield({}, "Hand")"#)).is_err());
}

#[test]
fn lua_gives_a_clear_error_for_an_uncalled_op() {
    let err = build("local s = {}\nfunction s.operation()\n    return Game.Ops.ReturnToBattlefield\nend\nreturn s\n").unwrap_err();
    assert!(err.contains("must be called"), "{err}");
    assert!(err.contains("ReturnToBattlefield"), "{err}");

    let err =
        build("local s = {}\nfunction s.operation()\n    return Game.Ops.Mill\nend\nreturn s\n")
            .unwrap_err();
    assert!(
        err.contains("Mill"),
        "the check covers every Game.Ops constructor: {err}"
    );
}

#[test]
fn lua_uncalled_op_in_set_operation_fails_card_registration() {
    let source = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.ReturnAllToBattlefield)
    a:Register()
end
return s
"#;
    let mut card = bear(1, P0, ZoneKind::Battlefield);
    card.script = Some(source);
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
    state.objects.insert(
        ObjectId(1),
        card_object(ObjectId(1), definition(&card), P0, ZoneKind::Battlefield),
    );
    let def = state.objects[&ObjectId(1)].definition.clone();
    let err = ScriptRuntime::new()
        .run_card_script(&mut state, &def, ObjectId(1))
        .unwrap_err();
    assert!(format!("{err:?}").contains("must be called"), "{err:?}");
}
