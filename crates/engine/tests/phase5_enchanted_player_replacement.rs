//! CR 303.4m, 616: Wheel of Sun and Moon, "If a card would be put into enchanted player's graveyard from anywhere, instead it is revealed and put on the bottom of its owner's library."
//! `EventFilter::WouldEnterEnchantedPlayerGraveyard` resolves against the source's `attached_to_player` and the card's owner (CR 400.7), is not gated on `is_destroy`, and reuses `ReplaceZoneDestination(Library)`.
// TODO: revealing the card, since no per-viewer hidden-information boundary exists (`VisibilityPolicy::is_visible_to` has no callers)

use std::collections::HashMap;

use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones};

const WHEEL_OF_SUN_AND_MOON: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldEnterEnchantedPlayerGraveyard)
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Library"))
    e:Register()
end
return s
"#;

fn creature_definition(toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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

fn wheel_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Wheel of Sun and Moon".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant player. If a card would be put into enchanted player's graveyard \
                       from anywhere, instead that card is revealed and put on the bottom of its \
                       owner's library."
            .to_string(),
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
        ..stackfish_testutil::two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Library],
        ))
    }
}

fn wheel_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldEnterEnchantedPlayerGraveyard,
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Library),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

#[test]
fn matches_a_card_headed_to_the_enchanted_player_s_own_graveyard_from_anywhere() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let aura = ObjectId(1);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(2), p1, ZoneKind::Battlefield));
    let mut aura_obj = card_object(aura, wheel_definition(""), p0, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, aura_obj);

    let effect = wheel_effect(aura);
    let script_runtime = ScriptRuntime::new();

    // CR 616 "from anywhere" covers a battlefield death,
    let dies = GameEventKind::ZoneChange { object: creature, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
    assert!(effect.matches(&dies, true, &state, &script_runtime), "should match the enchanted player's own creature dying");
    // not only destroy,
    assert!(effect.matches(&dies, false, &state, &script_runtime), "must match regardless of the is_destroy tag");
    // and a discard from hand.
    let discards = GameEventKind::ZoneChange { object: creature, from: ZoneKind::Hand, to: ZoneKind::Graveyard, last_known_info: None };
    assert!(effect.matches(&discards, false, &state, &script_runtime), "CR 616's own wording is unconditional on the source zone");
}

#[test]
fn does_not_match_a_different_player_s_card_or_a_non_graveyard_destination() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let own_creature = ObjectId(0);
    let foes_creature = ObjectId(1);
    let aura = ObjectId(2);
    put_on_battlefield(&mut state, card_object(own_creature, creature_definition(2), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(foes_creature, creature_definition(2), p1, ZoneKind::Battlefield));
    let mut aura_obj = card_object(aura, wheel_definition(""), p0, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, aura_obj);

    let effect = wheel_effect(aura);
    let script_runtime = ScriptRuntime::new();

    // Owned by p0 and not the enchanted player (p1), so it does not match even though it heads to a graveyard.
    let wrong_owner = GameEventKind::ZoneChange { object: own_creature, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
    assert!(!effect.matches(&wrong_owner, true, &state, &script_runtime));

    // Owned by the enchanted player, but headed to Exile, not Graveyard.
    let wrong_destination = GameEventKind::ZoneChange { object: foes_creature, from: ZoneKind::Battlefield, to: ZoneKind::Exile, last_known_info: None };
    assert!(!effect.matches(&wrong_destination, true, &state, &script_runtime));
}

#[test]
fn does_not_match_when_the_source_is_not_attached_to_a_player() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let aura = ObjectId(1);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(2), p0, ZoneKind::Battlefield));
    // Not attached to anything, a malformed fixture, as WouldDieEnchanted's None arm treats it.
    put_on_battlefield(&mut state, card_object(aura, wheel_definition(""), p0, ZoneKind::Battlefield));

    let effect = wheel_effect(aura);
    let script_runtime = ScriptRuntime::new();
    let dies = GameEventKind::ZoneChange { object: creature, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
    assert!(!effect.matches(&dies, true, &state, &script_runtime));
}

#[test]
fn wheel_of_sun_and_moon_redirects_a_dying_creature_to_the_bottom_of_its_owner_s_library() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let aura = ObjectId(1);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(2), p1, ZoneKind::Battlefield));
    let mut aura_obj = card_object(aura, wheel_definition(""), p0, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, aura_obj);
    state.replacement_effects.push(wheel_effect(aura));
    // A pre-existing library card makes the redirected card's bottom position unambiguous.
    let already_in_library = ObjectId(2);
    state.objects.insert(already_in_library, card_object(already_in_library, creature_definition(2), p1, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p1)).unwrap().push_bottom(already_in_library);

    let script_runtime = ScriptRuntime::new();
    state.object_mut(creature).unwrap().damage_marked = 2;

    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(
                    e.kind,
                    GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Library, .. }
                )),
                "expected the death to be redirected into the library, got {events:?}"
            );
            assert!(
                !events.iter().any(|e| matches!(e.kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. })),
                "the graveyard move must not also have happened, got {events:?}"
            );
        }
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }

    assert!(state.object(creature).is_none(), "the creature's old battlefield id no longer resolves (CR 400.7)");
    assert!(state.zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.is_empty(), "nothing landed in the graveyard");
    let library: Vec<ObjectId> = state.zone(ZoneKind::Library, Some(p1)).unwrap().contents.iter().copied().collect();
    assert_eq!(library.len(), 2, "the redirected card is now in the library alongside the pre-existing one");
    let bottom = *library.last().unwrap();
    let bottom_obj = state.object(bottom).expect("the bottom card resolves");
    assert_eq!(bottom_obj.zone, ZoneKind::Library);
    assert_ne!(bottom, already_in_library, "the redirected card got a fresh id (CR 400.7), landing below the pre-existing one");
}

#[test]
fn without_wheel_of_sun_and_moon_the_creature_dies_normally() {
    let mut state = two_player_state();
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    put_on_battlefield(&mut state, card_object(creature, creature_definition(2), p1, ZoneKind::Battlefield));
    // No Aura, no replacement effect - a plain lethal-damage control case.

    let script_runtime = ScriptRuntime::new();
    state.object_mut(creature).unwrap().damage_marked = 2;

    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(
                    e.kind,
                    GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. }
                )),
                "expected a normal death, got {events:?}"
            );
        }
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
    assert!(!state.zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.is_empty(), "the creature really did land in the graveyard");
}

#[test]
fn run_card_script_self_registers_wheel_of_sun_and_moon() {
    let mut state = two_player_state();
    let aura = ObjectId(0);
    let definition = wheel_definition(WHEEL_OF_SUN_AND_MOON);
    state.objects.insert(aura, card_object(aura, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, aura).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    let effect = &state.replacement_effects[0];
    assert_eq!(effect.source, aura);
    assert_eq!(effect.consumption, ConsumptionPolicy::Repeatable);
    match &effect.spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldEnterEnchantedPlayerGraveyard,
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Library),
        } => {}
        other => panic!("expected a WouldEnterEnchantedPlayerGraveyard/ReplaceZoneDestination(Library) primitive spec, got {other:?}"),
    }
}
