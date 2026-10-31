//! CR 701.19b: `Game.CreateReplacementEffect` registers a repeatable regeneration shield (`ConsumptionPolicy::Repeatable`, `WouldDie`, `Regenerate`) from a static ability.
//! CR 701.19c (can't be regenerated) is checked fresh each time the shield is asked to apply (`ReplacementEffect::matches`), since the target's status can change after the shield was registered.

use std::collections::HashMap;

use stackfish_engine::sba::{SbaOutcome, check_state_based_actions};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

const STATIC_REGENERATE_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

fn creature_definition(toughness: i32, keywords: KeywordSet, script: &str) -> CardDefinition {
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
        oracle_text: "This creature regenerates.".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords,
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

#[test]
fn run_card_script_self_registers_a_repeatable_regeneration_shield() {
    let mut state = two_player_state();
    let creature = ObjectId(0);
    let definition = creature_definition(2, KeywordSet::empty(), STATIC_REGENERATE_SELF);
    state.objects.insert(
        creature,
        card_object(
            creature,
            definition.clone(),
            PlayerId(0),
            ZoneKind::Battlefield,
        ),
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, creature)
        .expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    let shield = &state.replacement_effects[0];
    assert_eq!(shield.source, creature);
    assert_eq!(shield.consumption, ConsumptionPolicy::Repeatable);
    match &shield.spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie { affected },
            payload: ReplacementPayload::Regenerate,
        } => {
            assert!(
                affected.self_only,
                "SetAffected(Game.Affected.Self) should carry through"
            );
        }
        other => panic!("expected a WouldDie/Regenerate primitive spec, got {other:?}"),
    }
}

#[test]
fn static_regeneration_shield_survives_being_used_and_saves_target_twice() {
    let mut state = two_player_state();
    let creature = ObjectId(0);
    let object = card_object(
        creature,
        creature_definition(2, KeywordSet::empty(), STATIC_REGENERATE_SELF),
        PlayerId(0),
        ZoneKind::Battlefield,
    );
    state.objects.insert(creature, object);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    // Simulates the shield the creature would have self-registered on an earlier battlefield entry.
    state.replacement_effects.push(ReplacementEffect {
        source: creature,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::Regenerate,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    let script_runtime = ScriptRuntime::new();

    state.object_mut(creature).unwrap().damage_marked = 2;
    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(e.kind, GameEventKind::Regenerated { object } if object == creature)),
                "expected a Regenerated event on the first lethal hit, got {events:?}"
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
    assert_eq!(
        state.replacement_effects.len(),
        1,
        "a Repeatable shield is never consumed"
    );
    let saved = state.object(creature).expect("regenerated, not destroyed");
    assert_eq!(saved.damage_marked, 0, "regeneration clears marked damage");
    assert!(saved.tapped, "regeneration taps the permanent");

    // A second lethal hit the same turn fires again (CR 701.19b "each time"), where a OneShot shield would be gone.
    state.object_mut(creature).unwrap().damage_marked = 2;
    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(e.kind, GameEventKind::Regenerated { object } if object == creature)),
                "expected a second Regenerated event, got {events:?}"
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
    assert_eq!(
        state.replacement_effects.len(),
        1,
        "still not consumed after the second use"
    );
    assert!(
        state.object(creature).is_some(),
        "the creature survived a second lethal hit"
    );
}

#[test]
fn static_regeneration_shield_does_not_save_a_target_that_later_cant_be_regenerated() {
    let mut state = two_player_state();
    let creature = ObjectId(0);
    let object = card_object(
        creature,
        creature_definition(2, KeywordSet::empty(), STATIC_REGENERATE_SELF),
        PlayerId(0),
        ZoneKind::Battlefield,
    );
    state.objects.insert(creature, object);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    state.replacement_effects.push(ReplacementEffect {
        source: creature,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::Regenerate,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    // The target gains CANT_BE_REGENERATED after the shield exists, so the CR 701.19c gate is checked at match time and not at creation.
    state.continuous_effects.push(ContinuousEffect {
        source: creature,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddKeyword(
            KeywordSet::CANT_BE_REGENERATED,
        )),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

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
                "expected the creature to die for real (shield gated off), got {events:?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e.kind, GameEventKind::Regenerated { .. })),
                "no Regenerated event should have fired, got {events:?}"
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
}
