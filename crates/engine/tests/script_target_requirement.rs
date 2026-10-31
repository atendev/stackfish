//! CR 702.5a, 303.4a: a card declares its base target requirement ("Enchant [quality]") with the optional `s.target_requirement()`, called once per `CardDefinition` build (`ScriptRuntime::build_target_requirement`) because an Aura's Enchant text is needed before it is cast.
//! It returns the flat filter table of `Game.SelectTargets` plus `count`, `up_to`, `at_least` and `exclude_source`, like the scenario crate's `[cards.target_requirement]` table. A `filter` key may hold a scripted predicate (or `{type="player", filter=fn}`) through `decode_target_filter_value`, for shapes the flat table cannot express.

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

#[test]
fn a_card_with_no_target_requirement_function_returns_none() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.register_abilities(c) end
        return s
    "#;
    let requirement = runtime
        .build_target_requirement(source)
        .expect("script should load");
    assert!(
        requirement.is_none(),
        "a card script with no target_requirement() function needs no targets"
    );
}

#[test]
fn target_requirement_function_decodes_a_subtype_filter_with_count() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { subtypes = { "Wall" }, count = 1 }
        end
        return s
    "#;
    let requirement = runtime
        .build_target_requirement(source)
        .expect("script should load")
        .expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else {
        panic!("expected Primitive")
    };
    assert_eq!(req.count, 1);
    assert!(!req.up_to);
    assert!(!req.at_least);
    match req.filter {
        TargetFilter::Object {
            mana_value: None,
            subtypes: Some(subtypes),
            card_types: None,
            ..
        } => {
            assert_eq!(subtypes, vec![Subtype("Wall".to_string())]);
        }
        other => panic!("expected an Object filter with subtypes=[Wall], got {other:?}"),
    }
}

// A `filter` key holding a function is a scripted-predicate legality check. This closes the TODO that the module doc used to name.
#[test]
fn target_requirement_function_decodes_a_scripted_predicate_filter() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { count = 1, filter = function(candidate, source, controller, state) return true end }
        end
        return s
    "#;
    let requirement = runtime
        .build_target_requirement(source)
        .expect("script should load")
        .expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else {
        panic!("expected Primitive")
    };
    assert_eq!(req.count, 1);
    assert!(
        matches!(req.filter, TargetFilter::Script(_)),
        "expected a scripted predicate, got {:?}",
        req.filter
    );
}

#[test]
fn target_requirement_function_decodes_a_player_scripted_predicate_filter() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { count = 1, type = "player", filter = function(player, source, controller, state) return true end }
        end
        return s
    "#;
    let requirement = runtime
        .build_target_requirement(source)
        .expect("script should load")
        .expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else {
        panic!("expected Primitive")
    };
    assert!(
        matches!(req.filter, TargetFilter::PlayerScript(_)),
        "expected a player scripted predicate, got {:?}",
        req.filter
    );
}

// A target_requirement() with no filter key still decodes the flat option-bag shape.
#[test]
fn target_requirement_function_without_a_filter_key_still_decodes_the_flat_shape() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { subtypes = { "Wall" }, count = 1 }
        end
        return s
    "#;
    let requirement = runtime
        .build_target_requirement(source)
        .expect("script should load")
        .expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else {
        panic!("expected Primitive")
    };
    assert!(matches!(
        req.filter,
        TargetFilter::Object {
            mana_value: None,
            subtypes: Some(_),
            ..
        }
    ));
}

#[test]
fn target_requirement_function_missing_count_is_a_load_error() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { subtypes = { "Wall" } }
        end
        return s
    "#;
    assert!(
        runtime.build_target_requirement(source).is_err(),
        "count is required, same as the scenario TOML's own TargetRequirementSpec.count"
    );
}

const ENCHANT_WALL_SCRIPT: &str = r#"
local s = {}

function s.target_requirement()
    return { subtypes = { "Wall" }, count = 1 }
end

function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Enchanted)
    e:Register()
end

return s
"#;

fn wall_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Wall".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: vec![Subtype("Wall".to_string())],
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(0)),
        toughness: Some(PTValue::Fixed(4)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("wall".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn bear_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        name: "Test Bear".to_string(),
        subtypes: Vec::new(),
        ..wall_definition(definition_id)
    }
}

fn enchant_wall_definition() -> CardDefinition {
    let runtime = ScriptRuntime::new();
    let target_requirement = runtime
        .build_target_requirement(ENCHANT_WALL_SCRIPT)
        .expect("script should load")
        .expect("script declares a target_requirement");
    CardDefinition {
        definition_id: 30,
        name: "Test Enchant Wall".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant Wall".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(ENCHANT_WALL_SCRIPT.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(target_requirement),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets,
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    })
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

#[test]
fn casting_the_script_declared_aura_at_a_wall_is_legal_and_attaches() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(0);
    state.objects.insert(
        wall,
        card_object(wall, wall_definition(1), p1, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(wall);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, enchant_wall_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(wall)]);
    let status = pay_free_and_drain(&mut engine);
    assert!(
        !matches!(status, StepStatus::Error(_)),
        "casting at a Wall should be legal, got {status:?}"
    );

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);

    let aura = engine
        .state()
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Battlefield && o.definition.name == "Test Enchant Wall")
        .expect("the Aura should have resolved onto the battlefield");
    assert_eq!(
        aura.attached_to,
        Some(wall),
        "the Aura should be attached to the Wall it targeted"
    );
}

#[test]
fn casting_the_script_declared_aura_at_a_non_wall_creature_is_rejected() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    state.objects.insert(
        bear,
        card_object(bear, bear_definition(1), p1, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(bear);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, enchant_wall_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]).unwrap_err();

    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a plain Bear (no Wall subtype) should fail the script-declared Enchant Wall restriction, got {err:?}"
    );
}

const ENCHANT_POWER_FOUR_SCRIPT: &str = r#"
local s = {}

function s.target_requirement()
    return { count = 1, filter = function(object, source, controller, state)
        for _, card in ipairs(state.battlefield) do
            if card.id == object then
                return card.power ~= nil and card.power >= 4
            end
        end
        return false
    end }
end

return s
"#;

fn creature_definition_pt(
    definition_id: u64,
    name: &str,
    power: i32,
    toughness: i32,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey("creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

// target_requirement is left None because a scripted predicate's ScriptHandle is valid only against the Engine's own runtime, so each test builds it with Engine::build_target_requirement and patches it onto the inserted object.
fn enchant_power_four_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 31,
        name: "Test Enchant Power Four".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant creature with power 4 or greater".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(ENCHANT_POWER_FOUR_SCRIPT.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn patch_in_power_four_target_requirement(engine: &mut Engine, card: ObjectId) {
    let target_requirement = engine
        .build_target_requirement(ENCHANT_POWER_FOUR_SCRIPT)
        .expect("script should load")
        .expect("script declares a target_requirement");
    engine
        .state_mut()
        .object_mut(card)
        .unwrap()
        .definition
        .target_requirement = Some(target_requirement);
}

#[test]
fn casting_the_scripted_predicate_aura_at_a_power_four_creature_is_legal() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let big_creature = ObjectId(0);
    state.objects.insert(
        big_creature,
        card_object(
            big_creature,
            creature_definition_pt(1, "Big Creature", 4, 4),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(big_creature);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, enchant_power_four_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    patch_in_power_four_target_requirement(&mut engine, card);
    cast_spell(&mut engine, p0, card, vec![Target::Object(big_creature)]);
    let status = pay_free_and_drain(&mut engine);
    assert!(
        !matches!(status, StepStatus::Error(_)),
        "casting at a power-4 creature should be legal, got {status:?}"
    );
}

#[test]
fn casting_the_scripted_predicate_aura_at_a_small_creature_is_rejected() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let small_creature = ObjectId(0);
    state.objects.insert(
        small_creature,
        card_object(
            small_creature,
            creature_definition_pt(1, "Small Creature", 2, 2),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(small_creature);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, enchant_power_four_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    patch_in_power_four_target_requirement(&mut engine, card);
    let err =
        try_cast_spell(&mut engine, p0, card, vec![Target::Object(small_creature)]).unwrap_err();

    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a power-2 creature should fail the scripted power-4-or-greater predicate, got {err:?}"
    );
}
