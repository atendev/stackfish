//! `Game.Conditions` holds event and state tags, `Game.Affected` and `Game.Filter` replace raw `:SetAffected` tables, and the removed names fail loudly.
// TODO: unknown and wrong-typed filter keys still pass silently until strict validation lands

use stackfish_engine::script::{ScriptError, ScriptRuntime};
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state, zones};

fn definition(register_body: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Scripted".to_string(),
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
        script_ref: ScriptReference::Inline(format!(
            "local s = {{}}\nfunction s.register_abilities(c)\n{register_body}\nend\nreturn s"
        )),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn run(register_body: &str) -> (Result<(), ScriptError>, GameState) {
    let mut state = two_player_state(zones(
        PlayerId(0),
        PlayerId(1),
        &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard],
    ));
    let source = ObjectId(500);
    let definition = definition(register_body);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));
    let result = ScriptRuntime::new().run_card_script(&mut state, &definition, source);
    (result, state)
}

fn error_of(register_body: &str) -> String {
    match run(register_body).0 {
        Ok(()) => panic!("script should fail to load:\n{register_body}"),
        Err(error) => format!("{error:?}"),
    }
}

const STATIC_PUMP: &str = r#"
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
"#;

#[test]
fn state_conditions_are_read_from_game_conditions() {
    let (result, state) = run(
        r#"
        local t = Game.CreateTriggeredAbility(c)
        t:SetCondition(Game.Conditions.Dies)
        t:SetInterveningIf(Game.Conditions.AtOrBelowLife(5))
        t:SetOperation(Game.Ops.GainLife(1))
        t:Register()
        "#,
    );
    result.expect("event and state tags register through one namespace");
    assert_eq!(state.abilities.len(), 1);
    assert!(state.abilities[0].intervening_if.is_some());
}

#[test]
fn the_intervening_if_namespace_is_gone() {
    let error = error_of("local t = Game.CreateTriggeredAbility(c) t:SetInterveningIf(Game.InterveningIf.Escaped)");
    assert!(error.contains("InterveningIf") || error.contains("nil"), "{error}");
}

#[test]
fn an_event_tag_in_an_intervening_if_slot_names_the_right_method() {
    let error = error_of("local t = Game.CreateTriggeredAbility(c) t:SetInterveningIf(Game.Conditions.Dies)");
    assert!(error.contains("event") && error.contains("SetCondition"), "{error}");
}

#[test]
fn a_state_tag_in_a_trigger_condition_names_the_right_method() {
    let error = error_of("local t = Game.CreateTriggeredAbility(c) t:SetCondition(Game.Conditions.Escaped)");
    assert!(error.contains("SetInterveningIf"), "{error}");
}

#[test]
fn a_static_ability_condition_takes_state_tags_only() {
    let body = format!("{STATIC_PUMP} e:SetCondition(Game.Conditions.ControlsAtLeast({{ card_types = {{ \"Land\" }} }}, 2)) e:Register()");
    run(&body).0.expect("state tag registers on a static ability");
    let error = error_of(&format!("{STATIC_PUMP} e:SetCondition(Game.Conditions.Dies)"));
    assert!(error.contains("event"), "{error}");
}

#[test]
fn always_is_removed_and_an_omitted_condition_is_unconditional() {
    let error = error_of("local t = Game.CreateTriggeredAbility(c) t:SetCondition(Game.Conditions.Always)");
    assert!(error.contains("SetCondition"), "{error}");
    let (result, state) =
        run("local t = Game.CreateTriggeredAbility(c) t:SetOperation(Game.Ops.GainLife(1)) t:Register()");
    result.expect("no condition registers");
    assert!(state.abilities[0].condition.is_none());
}

#[test]
fn set_affected_takes_tags_filters_and_functions_but_not_raw_tables() {
    for argument in [
        "Game.Affected.Self",
        "Game.Affected.Enchanted",
        "Game.Affected.EnchantedPlayerControls",
        "Game.Filter({ controller = \"you\", card_types = { \"Creature\" } })",
        "function(object, source, controller, state) return true end",
    ] {
        let (result, state) = run(&format!("{STATIC_PUMP} e:SetAffected({argument}) e:Register()"));
        result.unwrap_or_else(|error| panic!("{argument} should register: {error:?}"));
        assert_eq!(state.continuous_effects.len(), 1, "{argument}");
    }
    let error = error_of(&format!("{STATIC_PUMP} e:SetAffected({{ controller = \"you\" }})"));
    assert!(error.contains("Game.Filter"), "{error}");
}

#[test]
fn game_filter_rejects_the_object_flags_that_moved_to_game_affected() {
    for flag in ["self_only", "enchanted_only", "enchanted_player_controls"] {
        let error = error_of(&format!("local f = Game.Filter({{ {flag} = true }})"));
        assert!(error.contains("Game.Affected"), "{flag}: {error}");
    }
}

#[test]
fn game_filter_reads_the_shared_object_keys() {
    let (result, state) = run(&format!(
        "{STATIC_PUMP} e:SetAffected(Game.Filter({{ controller = \"opponent\", card_types = {{ \"Land\" }}, \
         excluded_supertypes = {{ \"Basic\" }}, subtypes = {{ \"Swamp\" }} }})) e:Register()"
    ));
    result.expect("shared keys decode");
    let AffectedFilter::Primitive(filter) = &state.continuous_effects[0].affected else {
        panic!("a Game.Filter is a primitive filter");
    };
    assert_eq!(filter.controller, Some(FilterController::Opponent));
    assert_eq!(filter.card_types, Some(CardTypeSet::LAND));
    assert_eq!(filter.excluded_supertypes, Some(SupertypeSet::BASIC));
    assert_eq!(filter.subtypes, Some(vec![Subtype("Swamp".to_string())]));
    assert!(!filter.self_only);
}

#[test]
fn a_static_ability_rejects_player_affected_tags() {
    let error = error_of(&format!("{STATIC_PUMP} e:SetAffected(Game.Affected.You)"));
    assert!(error.contains("Game.Affected.You") && error.contains("player"), "{error}");
}

#[test]
fn a_replacement_effect_takes_self_and_filter_only() {
    let body = |argument: &str| {
        format!(
            "local e = Game.CreateReplacementEffect(c) e:SetFilter(Game.EventFilters.WouldDie) \
             e:SetAffected({argument}) e:SetPayload(Game.ReplacementPayloads.Regenerate) e:Register()"
        )
    };
    run(&body("Game.Affected.Self")).0.expect("Self registers");
    run(&body("Game.Filter({ card_types = { \"Creature\" } })")).0.expect("a filter registers");
    assert!(error_of(&body("Game.Affected.Enchanted")).contains("replacement"));
    assert!(error_of(&body("{ self_only = true }")).contains("Game.Filter"));
}

#[test]
fn player_modifiers_share_game_affected() {
    run("local m = Game.CreateHandSizeModifier(c) m:SetAffected(Game.Affected.EnchantedPlayer) m:SetEffect(Game.HandSizeEffects.NoMaximum) m:Register()")
        .0
        .expect("hand size takes the enchanted player");
    run("local m = Game.CreateHandRevealModifier(c) m:SetAffected(Game.Affected.Opponent) m:Register()")
        .0
        .expect("hand reveal takes a controller tag");
    let (result, state) = run("local m = Game.CreateLandPlayModifier(c) m:SetAffected(Game.Affected.You) m:SetAdditional(1) m:Register()");
    if let Err(error) = result {
        assert!(!format!("{error:?}").contains("Game.Affected"), "{error:?}");
    } else {
        assert_eq!(state.land_play_modifiers.len(), 1);
    }
}

#[test]
fn draw_limit_and_land_play_modifiers_reject_the_enchanted_player() {
    for constructor in ["CreateDrawLimitModifier", "CreateLandPlayModifier"] {
        let error = error_of(&format!("local m = Game.{constructor}(c) m:SetAffected(Game.Affected.EnchantedPlayer)"));
        assert!(error.contains("EnchantedPlayer"), "{constructor}: {error}");
    }
}

#[test]
fn a_player_modifier_rejects_an_object_tag() {
    let error = error_of("local m = Game.CreateHandSizeModifier(c) m:SetAffected(Game.Affected.Self)");
    assert!(error.contains("objects"), "{error}");
}

#[test]
fn the_old_player_affected_namespaces_are_gone() {
    for namespace in ["HandSizeAffected", "HandRevealAffected", "DrawLimitAffected", "LandPlayAffected"] {
        let error = error_of(&format!("local m = Game.CreateHandSizeModifier(c) m:SetAffected(Game.{namespace}.You)"));
        assert!(error.contains("nil") || error.contains(namespace), "{namespace}: {error}");
    }
}

#[test]
fn a_cost_modifier_filter_must_be_game_filter() {
    run("local m = Game.CreateCostModifier(c) m:SetFilter(Game.Filter({ card_types = { \"Creature\" } })) m:SetAmount(-1) m:Register()")
        .0
        .expect("Game.Filter registers");
    let error = error_of("local m = Game.CreateCostModifier(c) m:SetFilter({ card_types = { \"Creature\" } })");
    assert!(error.contains("Game.Filter"), "{error}");
}

#[test]
fn artifact_creature_tokens_use_additional_types() {
    let error = error_of("local t = Game.Tokens.ArtifactCreature(\"Construct\", \"\", 1, 1)");
    assert!(error.contains("nil") || error.contains("ArtifactCreature"), "{error}");
    let (result, _) = run("local t = Game.Tokens.Creature(\"Construct\", \"\", 1, 1, { additional_types = { \"Artifact\" } })");
    result.expect("additional_types builds an artifact creature token");
}

#[test]
fn a_nested_target_filter_table_is_an_error_and_flat_keys_work() {
    let runtime = ScriptRuntime::new();
    let nested = r#"
        local s = {}
        function s.target_requirement() return { count = 1, filter = { card_types = { "Creature" } } } end
        return s
    "#;
    let error = runtime.build_target_requirement(nested).expect_err("nested filter tables are removed");
    assert!(format!("{error:?}").contains("nested"), "{error:?}");

    let flat = r#"
        local s = {}
        function s.target_requirement()
            return { count = 2, up_to = true, card_types = { "Creature" }, excluded_supertypes = { "Legendary" }, controller = "opponent" }
        end
        return s
    "#;
    let spec = runtime.build_target_requirement(flat).expect("flat keys load").expect("declares a requirement");
    let TargetSpec::Primitive(requirement) = spec else { panic!("one word") };
    assert_eq!(requirement.count, 2);
    assert!(requirement.up_to);
    let TargetFilter::Object { card_types, excluded_supertypes, controller, .. } = requirement.filter else {
        panic!("object filter");
    };
    assert_eq!(card_types, Some(CardTypeSet::CREATURE));
    assert_eq!(excluded_supertypes, Some(SupertypeSet::LEGENDARY));
    assert_eq!(controller, Some(FilterController::Opponent));
}

#[test]
fn a_target_filter_function_pairs_with_object_or_player() {
    let runtime = ScriptRuntime::new();
    let source = |extra: &str| {
        format!(
            "local s = {{}} function s.target_requirement() return {{ count = 1, {extra} filter = function() return true end }} end return s"
        )
    };
    let spec = runtime.build_target_requirement(&source("")).unwrap().unwrap();
    assert!(matches!(spec, TargetSpec::Primitive(TargetRequirement { filter: TargetFilter::Script(_), .. })));
    let spec = runtime.build_target_requirement(&source("type = \"player\",")).unwrap().unwrap();
    assert!(matches!(spec, TargetSpec::Primitive(TargetRequirement { filter: TargetFilter::PlayerScript(_), .. })));
    assert!(runtime.build_target_requirement(&source("type = \"any\",")).is_err());
}
