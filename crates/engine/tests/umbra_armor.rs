//! CR 702.89a: Umbra armor, "if enchanted permanent would be destroyed, instead remove all damage marked on it and destroy this Aura". `EventFilter::WouldDieEnchanted` resolves against the replacement source's `attached_to`.
//! `ReplacementPayload::UmbraArmor` has two effects on two objects (clear damage on the enchanted permanent, destroy the Aura with a CR 400.7 new id), so `sba::commit_death_or_regeneration` returns a `Vec` of committed events.

use std::collections::HashMap;

use stackfish_engine::sba::{SbaOutcome, check_state_based_actions};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

const UMBRA_ARMOR_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldDieEnchanted)
    e:SetPayload(Game.ReplacementPayloads.UmbraArmor)
    e:Register()
end
return s
"#;

fn creature_definition(toughness: i32) -> CardDefinition {
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
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn umbra_armor_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Umbra Armor Aura".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant creature. Enchanted creature has umbra armor.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn umbra_armor_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDieEnchanted,
            payload: ReplacementPayload::UmbraArmor,
        },
        consumption: ConsumptionPolicy::Repeatable,
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

#[test]
fn would_die_enchanted_matches_a_destroy_on_the_enchanted_creature_only() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let other_creature = ObjectId(1);
    let aura = ObjectId(2);
    put_on_battlefield(
        &mut state,
        card_object(creature, creature_definition(2), p0, ZoneKind::Battlefield),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            other_creature,
            creature_definition(2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    let mut aura_obj = card_object(aura, umbra_armor_definition(""), p0, ZoneKind::Battlefield);
    aura_obj.attached_to = Some(creature);
    put_on_battlefield(&mut state, aura_obj);

    let effect = umbra_armor_effect(aura);
    let script_runtime = ScriptRuntime::new();

    let would_die_enchanted = GameEventKind::ZoneChange {
        object: creature,
        from: ZoneKind::Battlefield,
        to: ZoneKind::Graveyard,
        last_known_info: None,
    };
    assert!(
        effect.matches(&would_die_enchanted, true, &state, &script_runtime),
        "should match a genuine destroy of the enchanted creature"
    );

    let not_enchanted = GameEventKind::ZoneChange {
        object: other_creature,
        from: ZoneKind::Battlefield,
        to: ZoneKind::Graveyard,
        last_known_info: None,
    };
    assert!(
        !effect.matches(&not_enchanted, true, &state, &script_runtime),
        "should not match a different creature's destruction"
    );

    assert!(
        !effect.matches(&would_die_enchanted, false, &state, &script_runtime),
        "CR 702.89a only fires on genuine destruction, not any death"
    );
}

#[test]
fn umbra_armor_saves_the_enchanted_creature_and_destroys_the_aura_instead() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let aura = ObjectId(1);
    put_on_battlefield(
        &mut state,
        card_object(creature, creature_definition(2), p0, ZoneKind::Battlefield),
    );
    let mut aura_obj = card_object(aura, umbra_armor_definition(""), p0, ZoneKind::Battlefield);
    aura_obj.attached_to = Some(creature);
    put_on_battlefield(&mut state, aura_obj);
    state.replacement_effects.push(umbra_armor_effect(aura));

    let script_runtime = ScriptRuntime::new();
    state.object_mut(creature).unwrap().damage_marked = 2;

    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(e.kind, GameEventKind::UmbraArmorConsumed { enchanted, aura: a } if enchanted == creature && a == aura)),
                "expected an UmbraArmorConsumed event, got {events:?}"
            );
            assert!(
                events.iter().any(|e| matches!(
                    e.kind,
                    GameEventKind::ZoneChange { object, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. } if object != creature
                )),
                "expected the Aura's own real ZoneChange to the graveyard, got {events:?}"
            );
        }
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

    let creature_obj = state.object(creature).expect("the creature survives");
    assert_eq!(
        creature_obj.damage_marked, 0,
        "CR 702.89a: all damage marked is removed"
    );
    assert_eq!(creature_obj.zone, ZoneKind::Battlefield);

    assert!(
        state.object(aura).is_none(),
        "the Aura's own old id no longer resolves"
    );
    let graveyard_aura = state
        .objects
        .values()
        .find(|o| o.definition.name == "Test Umbra Armor Aura")
        .expect("the Aura moved to the graveyard under a new id");
    assert_eq!(graveyard_aura.zone, ZoneKind::Graveyard);
}

#[test]
fn without_umbra_armor_the_creature_dies_normally() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    put_on_battlefield(
        &mut state,
        card_object(creature, creature_definition(2), p0, ZoneKind::Battlefield),
    );
    // No Aura, no replacement effect registered - a plain lethal-damage control case.

    let script_runtime = ScriptRuntime::new();
    state.object_mut(creature).unwrap().damage_marked = 2;

    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(
                    e.kind,
                    GameEventKind::ZoneChange {
                        from: ZoneKind::Battlefield,
                        to: ZoneKind::Graveyard,
                        ..
                    }
                )),
                "expected the creature to die for real, got {events:?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e.kind, GameEventKind::UmbraArmorConsumed { .. })),
                "no UmbraArmorConsumed event should fire without the Aura"
            );
        }
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
    assert!(
        state.object(creature).is_none(),
        "the creature's own old id no longer resolves"
    );
}

#[test]
fn run_card_script_self_registers_umbra_armor() {
    let mut state = two_player_state();
    let aura = ObjectId(0);
    let definition = umbra_armor_definition(UMBRA_ARMOR_SELF);
    state.objects.insert(
        aura,
        card_object(aura, definition.clone(), PlayerId(0), ZoneKind::Battlefield),
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, aura)
        .expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    let effect = &state.replacement_effects[0];
    assert_eq!(effect.source, aura);
    assert_eq!(effect.consumption, ConsumptionPolicy::Repeatable);
    match &effect.spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldDieEnchanted,
            payload: ReplacementPayload::UmbraArmor,
        } => {}
        other => panic!("expected a WouldDieEnchanted/UmbraArmor primitive spec, got {other:?}"),
    }
}
