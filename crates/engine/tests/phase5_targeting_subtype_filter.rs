//! Phase 5 vertical slice: CR 205.4b subtype target restrictions ("target
//! Human," "Equip Dragons") - the gap `target.rs`'s own TODO list had named
//! since the targeting-basics slice, and both `phase5/aura-attach` ("Enchant
//! Human"-style) and `phase5/equip-ability` (CR 702.6c "Equip [quality]
//! creature") deferred as a follow-up rather than guessing at.
//!
//! `TargetFilter::Object` gained a `subtypes: Option<Vec<Subtype>>` field,
//! mirroring `cost.rs`'s `FilterSpec::subtypes` shape exactly (an "or" match
//! against the target's *effective* subtypes, via `layers::
//! effective_characteristics`, same treatment `card_types` already gets in
//! `commands::target_is_legal`). Two consumers: a spell/ability's own cast-
//! time target requirement (`TargetSpec::Primitive`, built directly via
//! `CardDefinition.target_requirement`/`RegisteredAbility.target`), and
//! `Game.SelectTargets`/`Game.CreateActivatedAbility:SetTarget`'s shared
//! Lua table decoder (`decode_target_filter_table`) - both exercised below.
//!
//! `TargetFilter`/`TargetRequirement`/`TargetSpec` all lost their `Copy`
//! derive as a mechanical consequence (`Vec<Subtype>` isn't `Copy`) - the
//! two real call sites that relied on it (`ability.target` copied out in
//! `commands.rs`/`engine.rs`) now `.clone()` instead, otherwise invisible
//! from outside the crate.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str, subtypes: Vec<Subtype>) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes,
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn human(id: u64, name: &str) -> CardDefinition {
    creature_definition(id, name, vec![Subtype("Human".to_string())])
}

fn zombie(id: u64, name: &str) -> CardDefinition {
    creature_definition(id, name, vec![Subtype("Zombie".to_string())])
}

/// "Test Human Bolt: deals 2 damage to target Human."
fn human_bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Human Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Human Bolt: deals 2 damage to target Human.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_human_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: Some(vec![Subtype("Human".to_string())]),
                supertypes: None, excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

const STATIC_EQUIP_DRAGONS: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "object", card_types = { "Creature" }, subtypes = { "Dragon" }, controller = "you" })
    a:SetOperation(Game.Ops.Attach)
    a:Register()
end
return s
"#;

fn equipment_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Dragon Equipment".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Equipment".to_string())],
        oracle_text: "Test Dragon Equipment: Equip Dragons {0}.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_dragon_equipment".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    card_object(id, definition, owner, zone)
}

fn put_on_battlefield(state: &mut GameState, obj: GameObject) {
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
}

// ---------------------------------------------------------------------
// CR 205.4b: a spell's own "target Human"
// ---------------------------------------------------------------------

#[test]
fn casting_at_a_matching_subtype_creature_is_accepted() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, human(1, "Human Target"), PlayerId(1), ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, human_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let p1 = PlayerId(1);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "a Human is a legal target for \"target Human\"");
}

#[test]
fn casting_at_a_non_matching_subtype_creature_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, zombie(1, "Zombie Target"), PlayerId(1), ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, human_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "a Zombie isn't a legal target for \"target Human\", got {status:?}"
    );
}

#[test]
fn spell_fizzles_when_targets_subtype_changes_before_resolution() {
    // Re-checked fresh at resolution (CR 608.2b) the same way protection/
    // color/card-type target restrictions already are - RemoveAllAbilities
    // doesn't touch subtypes (only keywords/protection), so this instead
    // simulates the target leaving the battlefield, the one other way an
    // already-legal target can become illegal by resolution time.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, human(1, "Human Target"), p1, ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, human_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);

    // The target leaves the battlefield mid-stack (e.g. bounced by another
    // effect) - no longer a legal "target Human" by resolution time.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target);
    engine.state_mut().objects.remove(&target);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: the spell's only target left the battlefield, so it must fizzle rather than resolve"
    );
}

// ---------------------------------------------------------------------
// CR 702.6c: "Equip Dragons" via the shared Lua table decoder
// ---------------------------------------------------------------------

#[test]
fn game_selecttargets_decoder_accepts_a_subtypes_key_for_equip() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let equipment = ObjectId(0);
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP_DRAGONS.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0, ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert!(matches!(
        &ability.target,
        Some(TargetSpec::Primitive(req)) if matches!(
            &req.filter,
            TargetFilter::Object { subtypes: Some(s), .. } if s == &vec![Subtype("Dragon".to_string())]
        )
    ));
}

#[test]
fn equipping_a_non_matching_subtype_creature_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Plain Bear", Vec::new()), p0, ZoneKind::Battlefield));
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP_DRAGONS.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0, ZoneKind::Battlefield));
    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, equipment, vec![Target::Object(bear)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "\"Equip Dragons\" can't target a non-Dragon creature, got {status:?}"
    );
}

#[test]
fn equipping_a_matching_subtype_creature_attaches_it() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let dragon = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(dragon, creature_definition(1, "Test Dragon", vec![Subtype("Dragon".to_string())]), p0, ZoneKind::Battlefield));
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP_DRAGONS.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0, ZoneKind::Battlefield));
    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, equipment, vec![Target::Object(dragon)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(equipment).unwrap().attached_to, Some(dragon));
}
