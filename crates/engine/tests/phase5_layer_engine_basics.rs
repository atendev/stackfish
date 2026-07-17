//! Phase 5 vertical slice: CR 613 continuous-effect layer engine, narrowed to
//! layer 6 (ability adding) and layers 7b/7c (power/toughness set/modify) —
//! see `layers.rs`'s own doc comment for why the rest of CR 613's layers
//! (copy/control/text/type/color, plus 7a/7d/7e) aren't built yet, and for
//! why ordering within a layer is timestamp-only (no CR 613.6-8 dependency
//! detection).
//!
//! `ContinuousEffect`s here are built directly via struct literal, the same
//! "bootstrap before the scripting slice exists" pattern every prior
//! subsystem used (mana abilities before `Game.Costs.Tap`, triggered
//! abilities before `Game.CreateTriggeredAbility`) — `Game.
//! AddContinuousEffect`/static-ability self-registration are their own
//! future slice (docs/implementation.md's backlog, item 3).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

/// This slice's fixtures never register a `ReplacementEffect` on a dying
/// creature - a real CR 616.1 pause here would mean the fixture itself is
/// wrong, not something to handle gracefully.
fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
        SbaOutcome::NeedsLegendRuleChoice { .. } => {
            panic!("fixture didn't expect a legend-rule choice pause")
        }
        SbaOutcome::NeedsCommanderReturnChoice { .. } => {
            panic!("fixture didn't expect a commander-return choice pause")
        }
    }
}

fn creature_definition(power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
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
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(power, toughness, KeywordSet::empty()),
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::CombatDamage,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// A permanent-effect-shaped `ContinuousEffect` sourced from `source`,
/// affecting anything matching `filter`, at `timestamp`.
fn continuous_effect(
    source: ObjectId,
    layer: Layer,
    effect: EffectPrimitive,
    filter: EffectFilter,
    timestamp: u64,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, self_only: false }
}

#[test]
fn base_case_no_continuous_effects_mirrors_card_definition() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(2));
    assert_eq!(effective.toughness, Some(2));
    assert_eq!(effective.keywords, KeywordSet::empty());
}

#[test]
fn add_keyword_unions_into_effective_keywords() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::FLYING),
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(effective.keywords.contains(KeywordSet::FLYING));
}

#[test]
fn modify_pt_adds_on_top_of_base() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 1, toughness: 1 },
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3));
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn set_pt_overrides_base_before_modify_pt_applies() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let setter = ObjectId(0);
    let modifier = ObjectId(1);
    let bear = ObjectId(2);
    put_on_battlefield(&mut state, creature(setter, p0, 0, 0));
    put_on_battlefield(&mut state, creature(modifier, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    // Layer 7b: base 2/2 becomes a flat 0/1 ...
    state.continuous_effects.push(continuous_effect(
        setter,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 0, toughness: 1 },
        no_filter(),
        0,
    ));
    // ... then layer 7c adds +2/+2 on top of the *set* value, not the
    // original base - 7b must run before 7c regardless of timestamp order.
    state.continuous_effects.push(continuous_effect(
        modifier,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 2, toughness: 2 },
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(2));
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn later_timestamp_set_pt_wins_within_the_same_layer() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let first = ObjectId(0);
    let second = ObjectId(1);
    let bear = ObjectId(2);
    put_on_battlefield(&mut state, creature(first, p0, 0, 0));
    put_on_battlefield(&mut state, creature(second, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        first,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 4, toughness: 4 },
        no_filter(),
        5,
    ));
    state.continuous_effects.push(continuous_effect(
        second,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 1, toughness: 1 },
        no_filter(),
        10,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    // Timestamp 10 (second) landed after timestamp 5 (first) - it wins.
    assert_eq!(effective.power, Some(1));
    assert_eq!(effective.toughness, Some(1));
}

#[test]
fn effect_filter_only_affects_matching_objects() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let anthem = ObjectId(0);
    let own_bear = ObjectId(1);
    let opposing_bear = ObjectId(2);
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    put_on_battlefield(&mut state, creature(own_bear, p0, 2, 2));
    put_on_battlefield(&mut state, creature(opposing_bear, p1, 2, 2));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 1, toughness: 1 },
        EffectFilter { controller: Some(FilterController::You), card_types: None, self_only: false },
        0,
    ));

    let own = effective_characteristics(&state, &ScriptRuntime::new(), own_bear);
    let opposing = effective_characteristics(&state, &ScriptRuntime::new(), opposing_bear);

    assert_eq!(own.power, Some(3), "creatures you control should be boosted");
    assert_eq!(opposing.power, Some(2), "an opponent's creature should be untouched");
}

#[test]
fn self_only_filter_excludes_other_objects() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let equipment = ObjectId(0);
    let equipped = ObjectId(1);
    let bystander = ObjectId(2);
    put_on_battlefield(&mut state, creature(equipment, p0, 0, 0));
    put_on_battlefield(&mut state, creature(equipped, p0, 2, 2));
    put_on_battlefield(&mut state, creature(bystander, p0, 2, 2));
    // self_only models "equipped creature" as "the effect's own source" -
    // so the effect's source here stands in for the equipped creature
    // itself (see EffectFilter::self_only's doc comment).
    state.continuous_effects.push(continuous_effect(
        equipment,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 3, toughness: 0 },
        EffectFilter { controller: None, card_types: None, self_only: true },
        0,
    ));

    let source_itself = effective_characteristics(&state, &ScriptRuntime::new(), equipment);
    let other = effective_characteristics(&state, &ScriptRuntime::new(), bystander);

    assert_eq!(source_itself.power, Some(3));
    assert_eq!(other.power, Some(2));
}

#[test]
fn cda_power_and_toughness_are_still_skipped_not_guessed() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let mut object = creature(star, p0, 0, 0);
    object.definition.power =
        Some(PTValue::CharacteristicDefining { display: "*".to_string(), script: None });
    object.definition.toughness =
        Some(PTValue::CharacteristicDefining { display: "*".to_string(), script: None });
    put_on_battlefield(&mut state, object);

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), star);

    assert_eq!(effective.power, None);
    assert_eq!(effective.toughness, None);
}

// ---------------------------------------------------------------------
// Real consumer: sba::check_state_based_actions now reads toughness and
// Indestructible through effective_characteristics, not straight off
// CardDefinition.
// ---------------------------------------------------------------------

#[test]
fn a_modify_pt_boost_saves_a_creature_from_the_lethal_toughness_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, anthem_source(anthem, p0));
    let mut daged_bear = creature(bear, p0, 2, 2);
    daged_bear.damage_marked = 2;
    put_on_battlefield(&mut state, daged_bear);

    // Without the boost, 2 damage on a 2-toughness creature is lethal.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1, "should have died without the boost");
    assert!(state.object(bear).is_none());
}

#[test]
fn with_the_boost_registered_the_same_creature_survives_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, anthem_source(anthem, p0));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 1, toughness: 1 },
        no_filter(),
        0,
    ));
    let mut damaged_bear = creature(bear, p0, 2, 2);
    damaged_bear.damage_marked = 2;
    put_on_battlefield(&mut state, damaged_bear);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(events.is_empty(), "the +1/+1 anthem should make 2 damage sub-lethal on an effective 3 toughness");
    assert!(state.object(bear).is_some());
}

#[test]
fn a_granted_indestructible_survives_lethal_damage_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, anthem_source(source, p0));
    state.continuous_effects.push(continuous_effect(
        source,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::INDESTRUCTIBLE),
        no_filter(),
        0,
    ));
    let mut damaged_bear = creature(bear, p0, 2, 2);
    damaged_bear.damage_marked = 5;
    put_on_battlefield(&mut state, damaged_bear);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(events.is_empty(), "granted Indestructible should ignore 704.5g lethal damage");
    assert!(state.object(bear).is_some());
}

/// Non-creature permanent standing in for an anthem/equipment source - not
/// the object under test, just something for a `ContinuousEffect::source`
/// to point at. Deliberately NOT a creature (unlike `creature()`'s 0/0
/// shape would be) so `apply_creature_death`'s own 704.5f/g checks never
/// apply to the source itself and confuse a test about the *bear's*
/// survival.
fn anthem_source(id: ObjectId, owner: PlayerId) -> GameObject {
    let mut object = creature(id, owner, 0, 0);
    object.definition.card_types = CardTypeSet::ENCHANTMENT;
    object.definition.power = None;
    object.definition.toughness = None;
    object
}
