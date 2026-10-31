//! CR 613.3: layers 4 (`TypeChanging`) and 5 (`ColorChanging`), additive only, with one `AddCardType` or `AddColor` primitive per layer because those layers have no lettered sublayers.
//! The overwrite shape and supertype/subtype grants are in layer_engine_type_color_overwrite.rs. Effects are built directly by struct literal.

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::{SbaOutcome, check_state_based_actions};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

// No fixture registers a ReplacementEffect on a dying creature, so a CR 616.1 pause here would mean the fixture is wrong.
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
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    card_object(
        id,
        creature_definition(power, toughness, KeywordSet::empty()),
        owner,
        ZoneKind::Battlefield,
    )
}

fn land_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Land".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land(id: ObjectId, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
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
        attached_to_player: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy {
            public: true,
            visible_to: Vec::new(),
        },
        copy_generation: 0,
        entered_via_escape: false,
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
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
        condition: None,
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        self_only: false,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
    }
}

#[test]
fn base_case_no_continuous_effects_mirrors_card_definition() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.card_types, CardTypeSet::CREATURE);
    assert_eq!(effective.colors, ColorSet::empty());
}

#[test]
fn printed_mana_cost_symbols_derive_base_color_per_cr_202_2() {
    // effective_characteristics must derive color from the mana cost through CardDefinition::colors() (CR 105.2a), since colored cards rarely set color_indicator.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut definition = creature_definition(2, 2, KeywordSet::empty());
    definition.mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Blue)),
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
        ],
    };
    let mut object = creature(bear, p0, 2, 2);
    object.definition = definition;
    put_on_battlefield(&mut state, object);

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.colors, ColorSet::WHITE | ColorSet::BLUE);
}

#[test]
fn color_indicator_still_overrides_mana_cost_derived_color() {
    // CR 105.2a: an explicit color_indicator wins outright and does not union with the mana cost's colors.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut definition = creature_definition(2, 2, KeywordSet::empty());
    definition.mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White))],
    };
    definition.color_indicator = Some(ColorSet::BLACK);
    let mut object = creature(bear, p0, 2, 2);
    object.definition = definition;
    put_on_battlefield(&mut state, object);

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.colors, ColorSet::BLACK);
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

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target_land);

    assert!(
        effective.card_types.contains(CardTypeSet::CREATURE),
        "should gain Creature"
    );
    assert!(
        effective.card_types.contains(CardTypeSet::LAND),
        "should keep Land, not lose it"
    );
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

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target);

    assert!(
        effective.colors.contains(ColorSet::WHITE),
        "should keep White, not lose it"
    );
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
        EffectFilter {
            controller: Some(FilterController::You),
            card_types: None,
            self_only: false,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        },
        0,
    ));

    let own = effective_characteristics(&state, &ScriptRuntime::new(), own_land);
    let opposing = effective_characteristics(&state, &ScriptRuntime::new(), opposing_land);

    assert!(
        own.card_types.contains(CardTypeSet::CREATURE),
        "controller's own land should be animated"
    );
    assert!(
        !opposing.card_types.contains(CardTypeSet::CREATURE),
        "an opponent's land should be untouched"
    );
}

#[test]
fn a_plain_land_is_not_a_creature_and_is_untouched_by_the_lethal_damage_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let plain_land = ObjectId(0);
    let mut object = land(plain_land, p0);
    // Damage marked on a land should not happen in play, and the SBA must not fire on damage_marked alone without an effective Creature type.
    object.damage_marked = 3;
    put_on_battlefield(&mut state, object);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(
        events.is_empty(),
        "a non-creature land should never be subject to 704.5g"
    );
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
    // "Target land becomes a 3/3 Elemental creature": layer 4 AddCardType(CREATURE) plus layer 7b SetPT, both sourced from the land itself.
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
        EffectPrimitive::SetPT {
            power: 3,
            toughness: 3,
        },
        no_filter(),
        0,
    ));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert_eq!(
        events.len(),
        1,
        "the animated land should die to its 3 marked damage"
    );
    assert!(state.object(animated_land).is_none());
}
