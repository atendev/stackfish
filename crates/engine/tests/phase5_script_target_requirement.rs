//! Phase 5 vertical slice: a card's own base target requirement (CR
//! 702.5a/303.4a's "Enchant [quality]", or any other targeted spell's
//! targeting) declared by its own script, via the optional
//! `s.target_requirement()` module convention - a sibling to
//! `s.register_abilities(c)`, not a replacement.
//!
//! `s.target_requirement()` takes no arguments and is called once per
//! `CardDefinition` build (`ScriptRuntime::build_target_requirement`), not
//! per object per zone-entry the way `run_card_script`/`register_abilities(c)`
//! is - an Aura's own Enchant restriction is fixed text needed *before* it's
//! ever cast (while still in hand), so it can't wait for
//! `run_card_script`'s battlefield-entry timing. Returns the same flat
//! filter table `Game.SelectTargets`/`ActivatedAbilityBuilder::SetTarget`
//! already accept, plus `count`/`up_to`/`at_least`/`exclude_source` as
//! sibling keys - identical shape to the scenario crate's own TOML
//! `[cards.target_requirement]` table, just Lua-authored.
//!
//! `phase5/script-target-requirement-predicate`: a `filter` key can hold a
//! scripted-predicate function instead (or `{type="player", filter=fn}`) -
//! the same table-or-function decoder `ActivatedAbilityBuilder::SetTarget`
//! already uses (`decode_target_filter_value`), for a legality shape the
//! flat option-bag table can't express (e.g. "Enchant creature with power 4
//! or greater," `phase5_scripted_target_filter.rs`'s own worked example).
//! `TargetFilter::Script`/`PlayerScript` were already read generically off
//! any `CardDefinition.target_requirement` before this slice - only
//! `target_requirement()`'s own decode entry point needed the change.

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state};

// ---------------------------------------------------------------------
// Decode-level: ScriptRuntime::build_target_requirement
// ---------------------------------------------------------------------

#[test]
fn a_card_with_no_target_requirement_function_returns_none() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.register_abilities(c) end
        return s
    "#;
    let requirement = runtime.build_target_requirement(source).expect("script should load");
    assert!(requirement.is_none(), "a card script with no target_requirement() function needs no targets");
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
    let requirement =
        runtime.build_target_requirement(source).expect("script should load").expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else { panic!("expected Primitive") };
    assert_eq!(req.count, 1);
    assert!(!req.up_to);
    assert!(!req.at_least);
    match req.filter {
        TargetFilter::Object { mana_value: None, subtypes: Some(subtypes), card_types: None, .. } => {
            assert_eq!(subtypes, vec![Subtype("Wall".to_string())]);
        }
        other => panic!("expected an Object filter with subtypes=[Wall], got {other:?}"),
    }
}

/// `phase5/script-target-requirement-predicate`: a `filter` key holding a
/// function is a scripted-predicate legality check (same shape `:SetTarget`
/// already accepts), not the flat option-bag table - closes the TODO this
/// slice's own doc comment above used to name.
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
    let requirement =
        runtime.build_target_requirement(source).expect("script should load").expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else { panic!("expected Primitive") };
    assert_eq!(req.count, 1);
    assert!(matches!(req.filter, TargetFilter::Script(_)), "expected a scripted predicate, got {:?}", req.filter);
}

/// The player-target scripted-predicate form (`{type="player", filter=fn}`)
/// works from `target_requirement()` too - same decoder `:SetTarget` uses.
#[test]
fn target_requirement_function_decodes_a_player_scripted_predicate_filter() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { count = 1, filter = { type = "player", filter = function(player, source, controller, state) return true end } }
        end
        return s
    "#;
    let requirement =
        runtime.build_target_requirement(source).expect("script should load").expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else { panic!("expected Primitive") };
    assert!(
        matches!(req.filter, TargetFilter::PlayerScript(_)),
        "expected a player scripted predicate, got {:?}",
        req.filter
    );
}

/// A `target_requirement()` with no `filter` key at all still decodes the
/// original flat option-bag shape unchanged - the new `filter`-key check
/// doesn't disturb the pre-existing common case.
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
    let requirement =
        runtime.build_target_requirement(source).expect("script should load").expect("declares a requirement");
    let TargetSpec::Primitive(req) = requirement else { panic!("expected Primitive") };
    assert!(matches!(req.filter, TargetFilter::Object { mana_value: None, subtypes: Some(_), .. }));
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

// ---------------------------------------------------------------------
// End-to-end: casting a script-declared Aura ("Enchant Wall")
// ---------------------------------------------------------------------

const ENCHANT_WALL_SCRIPT: &str = r#"
local s = {}

function s.target_requirement()
    return { subtypes = { "Wall" }, count = 1 }
end

function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected({ enchanted_only = true })
    e:Register()
end

return s
"#;

fn wall_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Wall".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
    CardDefinition { name: "Test Bear".to_string(), subtypes: Vec::new(), ..wall_definition(definition_id) }
}

/// The Aura's own `CardDefinition`, with `target_requirement` filled in by
/// its own script rather than hand-written inline - exactly the shape a
/// real caller (the scenario crate, or any other future `CardDefinition`
/// producer) builds it.
fn enchant_wall_definition() -> CardDefinition {
    let runtime = ScriptRuntime::new();
    let target_requirement = runtime
        .build_target_requirement(ENCHANT_WALL_SCRIPT)
        .expect("script should load")
        .expect("script declares a target_requirement");
    CardDefinition {
        definition_id: 30,
        name: "Test Enchant Wall".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None , pay_buyback: false},
        })
        
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

#[test]
fn casting_the_script_declared_aura_at_a_wall_is_legal_and_attaches() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(0);
    state.objects.insert(wall, card_object(wall, wall_definition(1), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(wall);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, enchant_wall_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(wall)]);
    let status = pay_free_and_drain(&mut engine);
    assert!(!matches!(status, StepStatus::Error(_)), "casting at a Wall should be legal, got {status:?}");

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    let aura = engine
        .state()
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Battlefield && o.definition.name == "Test Enchant Wall")
        .expect("the Aura should have resolved onto the battlefield");
    assert_eq!(aura.attached_to, Some(wall), "the Aura should be attached to the Wall it targeted");
}

#[test]
fn casting_the_script_declared_aura_at_a_non_wall_creature_is_rejected() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    state.objects.insert(bear, card_object(bear, bear_definition(1), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, enchant_wall_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a plain Bear (no Wall subtype) should fail the script-declared Enchant Wall restriction, got {err:?}");
}

// ---------------------------------------------------------------------
// End-to-end: casting a script-declared Aura whose Enchant restriction is
// a scripted predicate, not a flat filter table ("Enchant creature with
// power 4 or greater" - phase5_scripted_target_filter.rs's own worked
// example, reused here for target_requirement() instead of :SetTarget).
// ---------------------------------------------------------------------

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

fn creature_definition_pt(definition_id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id,
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

/// The Aura's own `CardDefinition`, with `target_requirement` left `None` -
/// a scripted predicate carries a real `ScriptHandle`, valid only against
/// whichever `ScriptRuntime` decoded it (the exact caller trap
/// `docs/implementation/05-script-bridge.md` already names for a
/// custom-function handle), so it can't be pre-built with a throwaway
/// `ScriptRuntime` the way `enchant_wall_definition`'s flat-table shape
/// safely is above. Each test builds it from the real `Engine`'s own
/// runtime instead, via `Engine::build_target_requirement`, then patches it
/// onto the already-inserted object - the same placeholder-then-patch
/// timing the scenario crate's own `register_target_script` established.
fn enchant_power_four_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 31,
        name: "Test Enchant Power Four".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
    engine.state_mut().object_mut(card).unwrap().definition.target_requirement = Some(target_requirement);
}

#[test]
fn casting_the_scripted_predicate_aura_at_a_power_four_creature_is_legal() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let big_creature = ObjectId(0);
    state.objects.insert(big_creature, card_object(big_creature, creature_definition_pt(1, "Big Creature", 4, 4), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(big_creature);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, enchant_power_four_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    patch_in_power_four_target_requirement(&mut engine, card);
    cast_spell(&mut engine, p0, card, vec![Target::Object(big_creature)]);
    let status = pay_free_and_drain(&mut engine);
    assert!(!matches!(status, StepStatus::Error(_)), "casting at a power-4 creature should be legal, got {status:?}");
}

#[test]
fn casting_the_scripted_predicate_aura_at_a_small_creature_is_rejected() {
    let mut state = two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)));
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let small_creature = ObjectId(0);
    state.objects.insert(small_creature, card_object(small_creature, creature_definition_pt(1, "Small Creature", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(small_creature);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, enchant_power_four_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    patch_in_power_four_target_requirement(&mut engine, card);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(small_creature)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a power-2 creature should fail the scripted power-4-or-greater predicate, got {err:?}");
}
