//! Phase 5 vertical slice: CR 613 continuous-effect layer engine, sub-slice
//! B — layers 4 (`TypeChanging`) and 5 (`ColorChanging`), additive only (CR
//! 613.3: layers 4/5 have no CR-defined lettered sublayers the way layer 7
//! does, so a single `AddCardType`/`AddColor` primitive per layer is
//! faithful to the rule, not a simplification of it). See `layers.rs`'s own
//! doc comment for why the overwrite shape (`SetCardTypes`/`SetColors`) and
//! supertypes/subtypes stay unbuilt.
//!
//! Same "`ContinuousEffect`s built directly via struct literal" bootstrap
//! pattern as `phase5_layer_engine_basics.rs` — no `Game.AddContinuousEffect`
//! Lua binding yet.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::check_state_based_actions;
use stackfish_engine::*;

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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// A land - no power/toughness at all (mirrors a real land's printed
/// values), base type `LAND` only. Stands in for the "target land becomes a
/// creature" real-consumer tests below.
fn land_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_land".to_string()),
        keywords: KeywordSet::empty(),
    }
}

fn land(id: ObjectId, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: land_definition(),
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
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
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

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

    let effective = effective_characteristics(&state, bear);

    assert_eq!(effective.card_types, CardTypeSet::CREATURE);
    assert_eq!(effective.colors, ColorSet::empty());
}

#[test]
fn add_card_type_unions_a_type_in_without_removing_existing_types() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let animator = ObjectId(0);
    let target_land = ObjectId(1);
    put_on_battlefield(&mut state, creature(animator, p0, 0, 0));
    put_on_battlefield(&mut state, land(target_land, p0));
    state.continuous_effects.push(continuous_effect(
        animator,
        Layer::TypeChanging,
        EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, target_land);

    assert!(effective.card_types.contains(CardTypeSet::CREATURE), "should gain Creature");
    assert!(effective.card_types.contains(CardTypeSet::LAND), "should keep Land, not lose it");
}

#[test]
fn add_color_unions_a_color_in_without_removing_existing_colors() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let target = ObjectId(1);
    put_on_battlefield(&mut state, creature(source, p0, 0, 0));
    let mut white_creature = creature(target, p0, 2, 2);
    white_creature.definition.color_indicator = Some(ColorSet::WHITE);
    put_on_battlefield(&mut state, white_creature);
    state.continuous_effects.push(continuous_effect(
        source,
        Layer::ColorChanging,
        EffectPrimitive::AddColor(ColorSet::RED),
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, target);

    assert!(effective.colors.contains(ColorSet::WHITE), "should keep White, not lose it");
    assert!(effective.colors.contains(ColorSet::RED), "should gain Red");
}

#[test]
fn effect_filter_still_scopes_type_and_color_effects() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let own_land = ObjectId(1);
    let opposing_land = ObjectId(2);
    put_on_battlefield(&mut state, creature(source, p0, 0, 0));
    put_on_battlefield(&mut state, land(own_land, p0));
    put_on_battlefield(&mut state, land(opposing_land, p1));
    state.continuous_effects.push(continuous_effect(
        source,
        Layer::TypeChanging,
        EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
        EffectFilter { controller: Some(FilterController::You), card_types: None, self_only: false },
        0,
    ));

    let own = effective_characteristics(&state, own_land);
    let opposing = effective_characteristics(&state, opposing_land);

    assert!(own.card_types.contains(CardTypeSet::CREATURE), "controller's own land should be animated");
    assert!(!opposing.card_types.contains(CardTypeSet::CREATURE), "an opponent's land should be untouched");
}

// ---------------------------------------------------------------------
// Real consumer: sba::check_state_based_actions now reads card_types
// through effective_characteristics, not straight off CardDefinition - a
// land animated into a creature by AddCardType is actually subject to CR
// 704.5g's lethal-damage check.
// ---------------------------------------------------------------------

#[test]
fn a_plain_land_is_not_a_creature_and_is_untouched_by_the_lethal_damage_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let plain_land = ObjectId(0);
    let mut object = land(plain_land, p0);
    // Damage marked on a land shouldn't happen in real play, but this
    // proves the SBA doesn't fire just because damage_marked is nonzero -
    // it still requires an effective Creature type first.
    object.damage_marked = 3;
    put_on_battlefield(&mut state, object);

    let events = check_state_based_actions(&mut state);

    assert!(events.is_empty(), "a non-creature land should never be subject to 704.5g");
    assert!(state.object(plain_land).is_some());
}

#[test]
fn a_land_animated_into_a_creature_by_add_card_type_dies_to_lethal_damage_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let animated_land = ObjectId(0);
    let mut object = land(animated_land, p0);
    object.damage_marked = 3;
    put_on_battlefield(&mut state, object);
    // "Target land becomes a 3/3 Elemental creature": layer 4's
    // AddCardType(CREATURE) plus layer 7b's SetPT for the 3/3, both
    // sourced from the land itself (self-sourced, same as a card's own
    // static ability would be).
    state.continuous_effects.push(continuous_effect(
        animated_land,
        Layer::TypeChanging,
        EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
        no_filter(),
        0,
    ));
    state.continuous_effects.push(continuous_effect(
        animated_land,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 3, toughness: 3 },
        no_filter(),
        0,
    ));

    let events = check_state_based_actions(&mut state);

    assert_eq!(events.len(), 1, "the animated land should die to its 3 marked damage");
    assert!(state.object(animated_land).is_none());
}
