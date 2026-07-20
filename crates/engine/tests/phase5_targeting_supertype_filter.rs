//! Phase 5 vertical slice: CR 205.4a supertype target restrictions ("target
//! legendary creature") - closes the gap `target.rs`'s own doc comment
//! named since `phase5/targeting-subtype-filter`: `FilterSpec` (cost.rs)
//! already had a `supertypes` field, `TargetFilter` didn't.
//!
//! `TargetFilter::Object` gained a `supertypes: Option<SupertypeSet>` field,
//! mirroring `cost.rs`'s `FilterSpec::supertypes` shape exactly (an
//! intersection test against the target's *effective* supertypes, via
//! `layers::effective_characteristics`, same treatment `card_types`/
//! `subtypes` already get in `commands::target_is_legal`). Two consumers,
//! both exercised below: a spell/ability's own cast-time target requirement
//! (`TargetSpec::Primitive`, built directly via `CardDefinition.
//! target_requirement`/`RegisteredAbility.target`), and `Game.
//! SelectTargets`/`Game.CreateActivatedAbility:SetTarget`'s shared Lua table
//! decoder (`decode_target_filter_table`). `SupertypeSet` is `Copy`
//! (unlike `subtypes`' `Vec<Subtype>`), so this field addition needed no
//! `Copy`-derive fallout - purely additive.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn creature_definition(id: u64, name: &str, supertypes: SupertypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes,
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
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
    }
}

fn legend(id: u64, name: &str) -> CardDefinition {
    creature_definition(id, name, SupertypeSet::LEGENDARY)
}

fn vanilla(id: u64, name: &str) -> CardDefinition {
    creature_definition(id, name, SupertypeSet::empty())
}

/// "Test Legend Bolt: deals 2 damage to target legendary creature."
fn legend_bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Legend Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Legend Bolt: deals 2 damage to target legendary creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_legend_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: Some(SupertypeSet::LEGENDARY),
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
    }
}

const STATIC_EQUIP_LEGENDARY: &str = r#"
local s = {}
function s.initial_effect(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "object", card_types = { "Creature" }, supertypes = { "Legendary" }, controller = "you" })
    a:SetOperation(Game.Ops.Attach)
    a:Register()
end
return s
"#;

fn equipment_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Legend Equipment".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Equipment".to_string())],
        oracle_text: "Test Legend Equipment: Equip legendary creature {0}.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_legend_equipment".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn put_on_battlefield(state: &mut GameState, obj: GameObject) {
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

fn player_state(id: PlayerId) -> PlayerState {
    PlayerState {
        player_id: id,
        life: 20,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
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
// CR 205.4a: a spell's own "target legendary creature"
// ---------------------------------------------------------------------

#[test]
fn casting_at_a_legendary_creature_is_accepted() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, legend(1, "Legend Target"), PlayerId(1), ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, legend_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let p1 = PlayerId(1);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "a legendary creature is a legal target for \"target legendary creature\"");
}

#[test]
fn casting_at_a_non_legendary_creature_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, vanilla(1, "Plain Target"), PlayerId(1), ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, legend_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "a non-legendary creature isn't a legal target for \"target legendary creature\", got {status:?}"
    );
}

#[test]
fn spell_fizzles_when_target_leaves_before_resolution() {
    // Re-checked fresh at resolution (CR 608.2b), same "target leaves the
    // battlefield" recheck phase5_targeting_subtype_filter.rs's own version
    // of this test already exercises for subtypes.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, permanent(target, legend(1, "Legend Target"), p1, ZoneKind::Battlefield));
    let card = ObjectId(2);
    state.objects.insert(card, permanent(card, legend_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);

    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target);
    engine.state_mut().objects.remove(&target);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: the spell's only target left the battlefield, so it must fizzle rather than resolve"
    );
}

// ---------------------------------------------------------------------
// Shared Lua table decoder: "Equip legendary creature"
// ---------------------------------------------------------------------

#[test]
fn game_selecttargets_decoder_accepts_a_supertypes_key_for_equip() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let equipment = ObjectId(0);
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP_LEGENDARY.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0, ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert!(matches!(
        &ability.target,
        Some(TargetSpec::Primitive(req)) if matches!(
            &req.filter,
            TargetFilter::Object { supertypes: Some(s), .. } if *s == SupertypeSet::LEGENDARY
        )
    ));
}

#[test]
fn equipping_a_non_legendary_creature_is_rejected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(bear, vanilla(1, "Plain Bear"), p0, ZoneKind::Battlefield));
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP_LEGENDARY.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0, ZoneKind::Battlefield));
    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, equipment, vec![Target::Object(bear)]);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "\"Equip legendary creature\" can't target a non-legendary creature, got {status:?}"
    );
}

#[test]
fn equipping_a_legendary_creature_attaches_it() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let legendary = ObjectId(0);
    let equipment = ObjectId(1);
    put_on_battlefield(&mut state, permanent(legendary, legend(1, "Test Legend"), p0, ZoneKind::Battlefield));
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(STATIC_EQUIP_LEGENDARY.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), p0, ZoneKind::Battlefield));
    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, equipment, vec![Target::Object(legendary)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(equipment).unwrap().attached_to, Some(legendary));
}
