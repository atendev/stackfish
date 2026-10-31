//! CR 303.4m: an ability that refers to the "enchanted [object or player]" refers to what the permanent is attached to, here for a continuous effect over other objects (Curse of Vitality: "Creatures enchanted player controls have lifelink").
//! `AffectedFilter::EnchantedPlayerControls` compares `attached_to_player` with the candidate's controller, and cannot combine with `controller` or `card_types`.

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
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
        name: "Curse of Vitality".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
    card_object(
        id,
        creature_definition(power, toughness),
        owner,
        ZoneKind::Battlefield,
    )
}

fn aura_on_player(
    id: ObjectId,
    owner: PlayerId,
    player: PlayerId,
    script_ref: ScriptReference,
) -> GameObject {
    GameObject {
        attached_to_player: Some(player),
        ..card_object(
            id,
            aura_definition(script_ref),
            owner,
            ZoneKind::Battlefield,
        )
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
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

fn lifelink_to_enchanted_players_creatures(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::EnchantedPlayerControls,
        effect: EffectPayload::Primitive(EffectPrimitive::AddKeyword(KeywordSet::LIFELINK)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

#[test]
fn enchanted_player_controls_matches_only_that_players_objects() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature_p0 = creature(ObjectId(1), p0, 2, 2);
    let creature_p1 = creature(ObjectId(2), p1, 2, 2);
    let aura = aura_on_player(
        ObjectId(3),
        p0,
        p0,
        ScriptReference::ByKey("vanilla".to_string()),
    );
    let effect = lifelink_to_enchanted_players_creatures(ObjectId(3));

    assert!(
        effect.applies_to(&creature_p0, &aura),
        "p0 is the enchanted player and controls this creature"
    );
    assert!(
        !effect.applies_to(&creature_p1, &aura),
        "p1 doesn't control it, even though the aura's own controller is p0"
    );
}

#[test]
fn enchanted_player_controls_matches_nothing_when_source_is_unattached() {
    let p0 = PlayerId(0);
    let creature_p0 = creature(ObjectId(1), p0, 2, 2);
    // Attached to an object instead of a player, or not attached at all.
    let unattached = card_object(
        ObjectId(3),
        aura_definition(ScriptReference::ByKey("vanilla".to_string())),
        p0,
        ZoneKind::Battlefield,
    );
    let effect = lifelink_to_enchanted_players_creatures(ObjectId(3));

    assert!(
        !effect.applies_to(&creature_p0, &unattached),
        "no enchanted player, so nothing can match"
    );
}

#[test]
fn enchanted_player_controls_does_not_match_the_enchanted_player_themselves_as_an_object() {
    // applies_to receives only objects, so the filter cannot match the player itself.
    let p0 = PlayerId(0);
    let aura = aura_on_player(
        ObjectId(3),
        p0,
        p0,
        ScriptReference::ByKey("vanilla".to_string()),
    );
    let effect = lifelink_to_enchanted_players_creatures(ObjectId(3));
    let unrelated_object = creature(ObjectId(9), PlayerId(1), 1, 1);

    assert!(!effect.applies_to(&unrelated_object, &aura));
}

#[test]
fn enchanted_player_controls_applies_through_effective_characteristics() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature_p0 = ObjectId(1);
    let creature_p1 = ObjectId(2);
    let aura = ObjectId(3);
    put_on_battlefield(&mut state, creature(creature_p0, p0, 2, 2));
    put_on_battlefield(&mut state, creature(creature_p1, p1, 2, 2));
    put_on_battlefield(
        &mut state,
        aura_on_player(aura, p1, p0, ScriptReference::ByKey("vanilla".to_string())),
    );
    state
        .continuous_effects
        .push(lifelink_to_enchanted_players_creatures(aura));

    let script_runtime = ScriptRuntime::new();
    assert!(
        effective_characteristics(&state, &script_runtime, creature_p0)
            .keywords
            .contains(KeywordSet::LIFELINK),
        "p0 is the enchanted player and controls this creature"
    );
    assert!(
        !effective_characteristics(&state, &script_runtime, creature_p1)
            .keywords
            .contains(KeywordSet::LIFELINK),
        "p1's own creature is untouched, even though p1 controls the Aura itself"
    );
}

#[test]
fn enchanted_player_controls_tracks_a_change_of_control() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature_id = ObjectId(1);
    let aura = ObjectId(3);
    put_on_battlefield(&mut state, creature(creature_id, p0, 2, 2));
    put_on_battlefield(
        &mut state,
        aura_on_player(aura, p0, p0, ScriptReference::ByKey("vanilla".to_string())),
    );
    state
        .continuous_effects
        .push(lifelink_to_enchanted_players_creatures(aura));

    let script_runtime = ScriptRuntime::new();
    assert!(
        effective_characteristics(&state, &script_runtime, creature_id)
            .keywords
            .contains(KeywordSet::LIFELINK)
    );

    // Control changes hands, and with no caching the creature immediately stops qualifying.
    state.object_mut(creature_id).unwrap().controller = p1;
    assert!(
        !effective_characteristics(&state, &script_runtime, creature_id)
            .keywords
            .contains(KeywordSet::LIFELINK)
    );
}

const SCRIPT_ENCHANTED_PLAYER_CONTROLS: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddKeyword("Lifelink"))
    e:SetAffected(Game.Affected.EnchantedPlayerControls)
    e:Register()
end
return s
"#;

#[test]
fn run_card_script_self_registers_an_enchanted_player_controls_affects_filter() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let aura = ObjectId(3);
    let definition = aura_definition(ScriptReference::Inline(
        SCRIPT_ENCHANTED_PLAYER_CONTROLS.to_string(),
    ));
    put_on_battlefield(
        &mut state,
        GameObject {
            attached_to_player: Some(p0),
            ..card_object(aura, definition.clone(), p0, ZoneKind::Battlefield)
        },
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, aura)
        .expect("Register() succeeds with { enchanted_player_controls = true }");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(
        state.continuous_effects[0].affected,
        AffectedFilter::EnchantedPlayerControls
    ));
}

#[test]
fn scripted_enchanted_player_controls_registration_applies_through_effective_characteristics() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature_p0 = ObjectId(1);
    let creature_p1 = ObjectId(2);
    let aura = ObjectId(3);
    put_on_battlefield(&mut state, creature(creature_p0, p0, 2, 2));
    put_on_battlefield(&mut state, creature(creature_p1, p1, 2, 2));
    let definition = aura_definition(ScriptReference::Inline(
        SCRIPT_ENCHANTED_PLAYER_CONTROLS.to_string(),
    ));
    put_on_battlefield(
        &mut state,
        GameObject {
            attached_to_player: Some(p0),
            ..card_object(aura, definition.clone(), p1, ZoneKind::Battlefield)
        },
    );

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, aura)
        .expect("Register() succeeds");

    assert!(
        effective_characteristics(&state, &runtime, creature_p0)
            .keywords
            .contains(KeywordSet::LIFELINK),
        "the enchanted player's own creature gains lifelink"
    );
    assert!(
        !effective_characteristics(&state, &runtime, creature_p1)
            .keywords
            .contains(KeywordSet::LIFELINK),
        "the Aura's controller's own creature is untouched"
    );
}
