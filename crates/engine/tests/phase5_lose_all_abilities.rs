//! Phase 5 vertical slice: `EffectPrimitive::RemoveAllAbilities` (CR 613.1f,
//! layer 6's ability-*removing* half — "~ loses all abilities"). Before this
//! slice, layer 6 only had the additive `AddKeyword` shape; a card saying
//! "loses all abilities" had no construction path (`Layer::TextChanging`'s
//! own TODO flagged it, even though it's mechanically layer 6).
//!
//! Deliberately narrow: only strips `KeywordSet`/`protection_colors` (the
//! latter added in parallel by `phase5/protection-from-color`, reconciled
//! when the two branches merged - see this file's own
//! `remove_all_abilities_also_strips_a_granted_protection_color` test), the
//! layer-engine half of "abilities." A permanent's own custom triggered/
//! activated `RegisteredAbility` entries are never filtered by continuous
//! effects at all in this engine (`collect_triggers`/activation-legality
//! checks read `state.abilities` directly) - not covered here, same shape
//! the layer-2-control-consumers gap already named for a different layer.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

// ---------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------

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
    EffectFilter { controller: None, card_types: None, self_only: false }
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

// ---------------------------------------------------------------------
// Unit-level: effective_characteristics's layer-6 fold
// ---------------------------------------------------------------------

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
    // Later timestamp - e.g. a second effect resolving after the strip -
    // should still stick on top, same as a real strip-then-regrant.
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

/// CR 613.1f names "ability-adding... and ability-removing effects"
/// together, no carve-out for protection specifically - `protection_colors`
/// is a separate field from `keywords` in the same layer-6 fold (added by
/// `phase5/protection-from-color`, developed in parallel and merged
/// together with this slice), so `RemoveAllAbilities` has to reset both or
/// "loses all abilities" would leave protection behind. Same real
/// interaction the two slices' own UNRESOLVED notes each flagged before
/// either saw the other's code.
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

// ---------------------------------------------------------------------
// Combat integration: printed Flying stripped by RemoveAllAbilities no
// longer enforces blocking legality (mirrors phase5_layer_engine_consumers.
// rs's own granted-Flying test, in reverse).
// ---------------------------------------------------------------------

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
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);

    assert!(
        matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))),
        "no strip - printed Flying should still enforce blocking legality, got {status:?}"
    );
}

// ---------------------------------------------------------------------
// Lua wiring: Game.Effects.RemoveAllAbilities / self-registration
// ---------------------------------------------------------------------

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
        .load_function("return function(source, controller) return { remove_all_abilities = true } end")
        .unwrap();

    let state = combat_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::RemoveAllAbilities));
}
