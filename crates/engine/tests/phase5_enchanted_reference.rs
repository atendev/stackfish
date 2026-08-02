//! Phase 5 vertical slice: CR 303.4m — "an ability of a permanent that
//! refers to the 'enchanted [object or player]' refers to whatever object
//! or player that permanent is attached to." `GameObject.attached_to` already
//! existed (maintained by the attach machinery, CR 303.4d-h) but nothing read
//! it for continuous-effect matching - only SBA (CR 704.5m/n, "falls off if
//! illegal") ever consulted it.
//!
//! `AffectedFilter::EnchantedOnly` is the Rust-native, common-case shape
//! ("Enchanted creature gets +1/+1" needs no script at all): a new fieldless
//! variant, not a field on `EffectFilter` (129 struct-literal construction
//! sites with no `Default` impl - the same ripple class the modal-spell
//! slices already avoided each time). `ContinuousEffect::applies_to` resolves
//! it directly from the two full `&GameObject`s it's already given, no new
//! state lookup. For a more complex scripted Aura, `ScriptRuntime::
//! invoke_continuous_affects`/`invoke_continuous_effect` also gained a
//! trailing `enchanted: Option<ObjectId>` argument (nil in Lua when `None`),
//! and `card_summary_to_lua` now exposes `attached_to`/`attachments` on every
//! card summary for general board-state visibility.
//!
//! Narrowed on purpose: CR 303.4m for a permanent enchanting a *player*
//! stays unmodeled (`attached_to` is `ObjectId`-only); a replacement effect's
//! own "enchanted permanent" reference (e.g. Umbra armor, CR 702.89a) stays
//! unmodeled too (`ReplacementEffect.affected: EffectFilter`, a different,
//! non-`AffectedFilter` type - out of scope here).

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
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
        script_ref: ScriptReference::ByKey("vanilla".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn aura_definition(script_ref: ScriptReference) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Aura".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref,
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    card_object(id, creature_definition(power, toughness), owner, ZoneKind::Battlefield)
}

/// An Aura attached to `enchanted`, sitting on the battlefield.
fn attached_aura(id: ObjectId, owner: PlayerId, enchanted: ObjectId, script_ref: ScriptReference) -> GameObject {
    GameObject { attached_to: Some(enchanted), ..card_object(id, aura_definition(script_ref), owner, ZoneKind::Battlefield) }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState { step: Step::PreCombatMain, ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard])) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn enchanted_effect(source: ObjectId, effect: EffectPayload) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::EnchantedOnly,
        effect,
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

// ---------------------------------------------------------------------
// Unit-level: ContinuousEffect::applies_to / EnchantedOnly directly.
// ---------------------------------------------------------------------

#[test]
fn enchanted_only_matches_only_the_attached_object() {
    let p0 = PlayerId(0);
    let creature_a = creature(ObjectId(1), p0, 2, 2);
    let creature_b = creature(ObjectId(2), p0, 2, 2);
    let aura = attached_aura(ObjectId(3), p0, ObjectId(1), ScriptReference::ByKey("vanilla".to_string()));
    let effect = enchanted_effect(ObjectId(3), EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }));

    assert!(effect.applies_to(&creature_a, &aura), "creature A is what the aura is attached to");
    assert!(!effect.applies_to(&creature_b, &aura), "creature B is a different object entirely");
}

#[test]
fn enchanted_only_matches_nothing_when_the_source_is_unattached() {
    let p0 = PlayerId(0);
    let creature_a = creature(ObjectId(1), p0, 2, 2);
    // A card with no attachment at all (attached_to: None) - e.g. an Aura
    // that somehow lost its attachment target without yet being cleaned up
    // by SBA, or simply any non-Aura source using this filter by mistake.
    let unattached = card_object(ObjectId(3), aura_definition(ScriptReference::ByKey("vanilla".to_string())), p0, ZoneKind::Battlefield);
    let effect = enchanted_effect(ObjectId(3), EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }));

    assert!(!effect.applies_to(&creature_a, &unattached), "nothing is enchanted, so nothing can match");
}

// ---------------------------------------------------------------------
// Real consumer: effective_characteristics actually applies EnchantedOnly.
// ---------------------------------------------------------------------

#[test]
fn enchanted_only_applies_through_effective_characteristics() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let creature_a = ObjectId(1);
    let creature_b = ObjectId(2);
    let aura = ObjectId(3);
    put_on_battlefield(&mut state, creature(creature_a, p0, 2, 2));
    put_on_battlefield(&mut state, creature(creature_b, p0, 2, 2));
    put_on_battlefield(&mut state, attached_aura(aura, p0, creature_a, ScriptReference::ByKey("vanilla".to_string())));
    state.continuous_effects.push(enchanted_effect(
        aura,
        EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
    ));

    let script_runtime = ScriptRuntime::new();
    assert_eq!(effective_characteristics(&state, &script_runtime, creature_a).power, Some(3), "enchanted creature gets +1/+1");
    assert_eq!(effective_characteristics(&state, &script_runtime, creature_b).power, Some(2), "a different creature is untouched");
}

// ---------------------------------------------------------------------
// Script bridge: the new trailing `enchanted` argument.
// ---------------------------------------------------------------------

#[test]
fn invoke_continuous_affects_receives_the_enchanted_argument() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(object, source, controller, state, enchanted) return object == enchanted end")
        .expect("loads cleanly");

    let state = base_state();
    let source = ObjectId(1);
    let creature_a = ObjectId(2);
    assert!(
        runtime.invoke_continuous_affects(handle, &state, creature_a, source, PlayerId(0), Some(creature_a)).unwrap(),
        "object matches the enchanted id"
    );
    assert!(
        !runtime.invoke_continuous_affects(handle, &state, creature_a, source, PlayerId(0), None).unwrap(),
        "nil enchanted (nothing attached) never matches a real object id"
    );
}

#[test]
fn invoke_continuous_effect_receives_the_enchanted_argument() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function(
            "return function(source, controller, state, enchanted) \
                if enchanted ~= nil then return { modify_pt = { power = 9, toughness = 9 } } end \
                return { modify_pt = { power = 0, toughness = 0 } } \
            end",
        )
        .unwrap();

    let state = base_state();
    let decoded_attached = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), Some(ObjectId(5))).unwrap();
    assert_eq!(decoded_attached, Some(EffectPrimitive::ModifyPT { power: 9, toughness: 9 }));
    let decoded_unattached = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded_unattached, Some(EffectPrimitive::ModifyPT { power: 0, toughness: 0 }));
}

// ---------------------------------------------------------------------
// Game.CreateStaticAbility builder: :SetAffected({ enchanted_only = true }).
// ---------------------------------------------------------------------

const SCRIPT_ENCHANTED_ONLY: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected({ enchanted_only = true })
    e:Register()
end
return s
"#;

#[test]
fn run_card_script_self_registers_an_enchanted_only_affects_filter() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(3);
    let definition = aura_definition(ScriptReference::Inline(SCRIPT_ENCHANTED_ONLY.to_string()));
    put_on_battlefield(&mut state, GameObject { attached_to: Some(ObjectId(1)), ..card_object(aura, definition.clone(), p0, ZoneKind::Battlefield) });

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, aura).expect("Register() succeeds with { enchanted_only = true }");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(state.continuous_effects[0].affected, AffectedFilter::EnchantedOnly));
}

#[test]
fn scripted_enchanted_only_registration_applies_through_effective_characteristics() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let creature_a = ObjectId(1);
    let creature_b = ObjectId(2);
    let aura = ObjectId(3);
    put_on_battlefield(&mut state, creature(creature_a, p0, 2, 2));
    put_on_battlefield(&mut state, creature(creature_b, p0, 2, 2));
    let definition = aura_definition(ScriptReference::Inline(SCRIPT_ENCHANTED_ONLY.to_string()));
    put_on_battlefield(&mut state, GameObject { attached_to: Some(creature_a), ..card_object(aura, definition.clone(), p0, ZoneKind::Battlefield) });

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, aura).expect("Register() succeeds");

    assert_eq!(effective_characteristics(&state, &runtime, creature_a).power, Some(3), "the enchanted creature is buffed");
    assert_eq!(effective_characteristics(&state, &runtime, creature_b).power, Some(2), "an unenchanted creature is untouched");
}

// ---------------------------------------------------------------------
// card_summary_to_lua: attached_to/attachments on a state snapshot.
// ---------------------------------------------------------------------

#[test]
fn state_snapshot_exposes_attached_to_and_attachments() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let creature_a = ObjectId(1);
    let aura = ObjectId(3);
    put_on_battlefield(&mut state, creature(creature_a, p0, 2, 2));
    put_on_battlefield(&mut state, attached_aura(aura, p0, creature_a, ScriptReference::ByKey("vanilla".to_string())));

    let mut runtime = ScriptRuntime::new();
    let source = "return function(object, source, controller, state) \
        local creature_attached_to, aura_attached_to, aura_attachment_count \
        for _, c in ipairs(state.battlefield) do \
            if c.id == 1 then creature_attached_to = c.attached_to end \
            if c.id == 3 then aura_attached_to = c.attached_to; aura_attachment_count = #c.attachments end \
        end \
        return creature_attached_to == nil and aura_attached_to == 1 and aura_attachment_count == 0 \
        end";
    let handle = runtime.load_function(source).unwrap();

    assert!(
        runtime.invoke_continuous_affects(handle, &state, creature_a, aura, p0, Some(creature_a)).unwrap(),
        "creature has no attached_to, the aura's own attached_to points at the creature's id"
    );
}
