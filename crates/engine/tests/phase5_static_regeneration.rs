//! Phase 5 vertical slice: `Game.CreateReplacementEffect` — the CR 701.19b
//! sibling to `phase5/cant-be-regenerated`'s CR 701.19a one-shot shield. A
//! static ability's own effect can now self-register a *repeatable*
//! regeneration shield ("...each time that permanent would be destroyed",
//! not just the next time) - the `ConsumptionPolicy::Repeatable` +
//! `EventFilter::WouldDie`/`ReplacementPayload::Regenerate` shape already
//! existed and was already proven by `replacement.rs`'s own unit tests; the
//! only real gap was a creation path, same "no Lua/Rust creation surface
//! yet" shape `phase5/pump-effect`/`phase5/cant-be-regenerated` themselves
//! closed for their own primitives.
//!
//! Also proves a correctness finding made while implementing this slice: CR
//! 701.19c ("can't be regenerated" gates shield *application*) has to be
//! checked fresh every time a static shield is asked to apply, not just once
//! at shield-creation time the way the one-shot spell/ability path checks it
//! - a static shield is registered once but the target's CANT_BE_REGENERATED
//! status can change afterward. See `ReplacementEffect::matches`'s own doc
//! comment (types/effect.rs).

use std::collections::HashMap;

use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const STATIC_REGENERATE_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn creature_definition(toughness: i32, keywords: KeywordSet, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
    state.objects.insert(creature, card_object(creature, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, creature).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    let shield = &state.replacement_effects[0];
    assert_eq!(shield.source, creature);
    assert_eq!(shield.consumption, ConsumptionPolicy::Repeatable);
    match &shield.spec {
        ReplacementSpec::Primitive { filter: EventFilter::WouldDie { affected }, payload: ReplacementPayload::Regenerate } => {
            assert!(affected.self_only, "SetAffected({{ self_only = true }}) should carry through");
        }
        other => panic!("expected a WouldDie/Regenerate primitive spec, got {other:?}"),
    }
}

#[test]
fn static_regeneration_shield_survives_being_used_and_saves_target_twice() {
    let mut state = two_player_state();
    let creature = ObjectId(0);
    let object = card_object(creature, creature_definition(2, KeywordSet::empty(), STATIC_REGENERATE_SELF), PlayerId(0), ZoneKind::Battlefield);
    state.objects.insert(creature, object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    // Simulates the shield this creature would have self-registered upon an
    // earlier entrance to the battlefield (same convention
    // phase5_static_ability_self_registration.rs's own SBA test uses for a
    // ContinuousEffect) - the Lua wiring itself is proven separately above.
    state.replacement_effects.push(ReplacementEffect {
        source: creature,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
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
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
    assert_eq!(state.replacement_effects.len(), 1, "a Repeatable shield is never consumed");
    let saved = state.object(creature).expect("regenerated, not destroyed");
    assert_eq!(saved.damage_marked, 0, "regeneration clears marked damage");
    assert!(saved.tapped, "regeneration taps the permanent");

    // Second lethal hit, same turn - a OneShot shield would be gone by now
    // (phase5_regenerate_ability.rs's own tests prove that side); this one
    // fires again, proving CR 701.19b's "each time" behavior for real.
    state.object_mut(creature).unwrap().damage_marked = 2;
    match check_state_based_actions(&mut state, &script_runtime) {
        SbaOutcome::Progressed(events, _) => {
            assert!(
                events.iter().any(|e| matches!(e.kind, GameEventKind::Regenerated { object } if object == creature)),
                "expected a second Regenerated event, got {events:?}"
            );
        }
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
    assert_eq!(state.replacement_effects.len(), 1, "still not consumed after the second use");
    assert!(state.object(creature).is_some(), "the creature survived a second lethal hit");
}

#[test]
fn static_regeneration_shield_does_not_save_a_target_that_later_cant_be_regenerated() {
    let mut state = two_player_state();
    let creature = ObjectId(0);
    let object = card_object(creature, creature_definition(2, KeywordSet::empty(), STATIC_REGENERATE_SELF), PlayerId(0), ZoneKind::Battlefield);
    state.objects.insert(creature, object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    state.replacement_effects.push(ReplacementEffect {
        source: creature,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::Regenerate,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    // The target gains CANT_BE_REGENERATED *after* the static shield already
    // exists - proving the CR 701.19c gate is checked fresh at match time
    // (ReplacementEffect::matches), not only once when the shield was
    // created (there is no creation-time moment to check here at all - the
    // shield was registered once, up front, independent of this).
    state.continuous_effects.push(ContinuousEffect {
        source: creature,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddKeyword(KeywordSet::CANT_BE_REGENERATED)),
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
                    GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. }
                )),
                "expected the creature to die for real (shield gated off), got {events:?}"
            );
            assert!(
                !events.iter().any(|e| matches!(e.kind, GameEventKind::Regenerated { .. })),
                "no Regenerated event should have fired, got {events:?}"
            );
        }
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
}
