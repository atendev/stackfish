//! CR 613.1f: `EffectPrimitive::RemoveAllAbilities`, the ability-removing half of layer 6 ("~ loses all abilities"). It strips `KeywordSet` and `protection_colors`.
// TODO: custom triggered and activated `RegisteredAbility` entries are not filtered by continuous effects, since `collect_triggers` and activation checks read `state.abilities` directly

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }
}

fn self_only_effect(source: ObjectId, layer: Layer, effect: EffectPrimitive, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(EffectFilter { self_only: true, ..no_filter() }),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

#[test]
fn remove_all_abilities_strips_a_printed_keyword() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Drake", 2, 2, KeywordSet::FLYING), p0));
    state
        .continuous_effects
        .push(self_only_effect(bear, Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities, 0));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.keywords, KeywordSet::empty(), "printed Flying should be stripped");
}

#[test]
fn a_later_add_keyword_still_applies_after_an_earlier_remove_all_abilities() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Drake", 2, 2, KeywordSet::FLYING), p0));
    state
        .continuous_effects
        .push(self_only_effect(bear, Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities, 0));
    // A later timestamp, such as a second effect resolving after the strip, still sticks on top.
    state.continuous_effects.push(self_only_effect(
        bear,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::TRAMPLE),
        1,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(!effective.keywords.contains(KeywordSet::FLYING), "printed Flying was stripped");
    assert!(effective.keywords.contains(KeywordSet::TRAMPLE), "a later-timestamp grant still applies");
}

#[test]
fn remove_all_abilities_also_strips_an_earlier_granted_keyword() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Bear", 2, 2, KeywordSet::empty()), p0));
    // An earlier-timestamp grant (e.g. an aura attached first) ...
    state.continuous_effects.push(self_only_effect(
        bear,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::FLYING),
        0,
    ));
    // ... stripped by a later RemoveAllAbilities, not just the printed set.
    state
        .continuous_effects
        .push(self_only_effect(bear, Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities, 1));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.keywords, KeywordSet::empty(), "the earlier-granted Flying should be stripped too");
}

// CR 613.1f
// RemoveAllAbilities must reset `protection_colors` as well as `keywords`, which are separate fields in the layer 6 fold, or protection would survive "loses all abilities".
#[test]
fn remove_all_abilities_also_strips_a_granted_protection_color() {
    let mut state = combat_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, creature_definition("Bear", 2, 2, KeywordSet::empty()), p0));
    state.continuous_effects.push(self_only_effect(
        bear,
        Layer::AbilityAdding,
        EffectPrimitive::AddProtectionColor(ColorSet::RED),
        0,
    ));
    state
        .continuous_effects
        .push(self_only_effect(bear, Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities, 1));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.protection_colors, ColorSet::empty(), "the earlier-granted protection should be stripped too");
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

fn try_declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        ?;
    Ok(drain(engine))
}

fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached DeclareBlockers - state.step = {:?}", engine.state().step);
}

#[test]
fn printed_flying_stripped_by_remove_all_abilities_becomes_blockable_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Drake", 2, 2, KeywordSet::FLYING), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 2, KeywordSet::empty()), p1));
    state
        .continuous_effects
        .push(self_only_effect(attacker, Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "printed Flying was stripped - a ground creature should block legally, got {status:?}"
    );
}

#[test]
fn without_the_strip_the_same_printed_flying_attacker_still_enforces_legality() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Drake", 2, 2, KeywordSet::FLYING), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Bear", 2, 2, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let err = try_declare_blockers(&mut engine, p1, vec![(blocker, attacker)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "no strip - printed Flying should still enforce blocking legality, got {err:?}");
}

const STRIPS_ITSELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.RemoveAllAbilities)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn bear_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Lose-all-abilities test fixture.".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::FLYING,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

#[test]
fn game_effects_remove_all_abilities_self_registers_and_strips_printed_flying() {
    let mut state = combat_state();
    let bear = ObjectId(0);
    let definition = bear_definition(STRIPS_ITSELF);
    put_on_battlefield(&mut state, permanent(bear, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(state.continuous_effects[0].effect, EffectPayload::Primitive(EffectPrimitive::RemoveAllAbilities)));

    let effective = effective_characteristics(&state, &runtime, bear);
    assert_eq!(effective.keywords, KeywordSet::empty(), "printed Flying should be stripped by the self-registered effect");
}

#[test]
fn effect_decodes_remove_all_abilities_from_a_script_backed_function() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return Game.Effects.RemoveAllAbilities end")
        .unwrap();

    let state = combat_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::RemoveAllAbilities));
}
