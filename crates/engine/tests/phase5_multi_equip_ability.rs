//! Phase 5 vertical slice: CR 702.6d - "If a permanent has multiple equip
//! abilities, any of its equip abilities may be activated." Named as an
//! unattempted TODO by `phase5/equip-ability`'s own doc comment.
//!
//! Turned out not to need any new production code: `commands::find_ability`
//! already looks up `state.abilities` filtered by `source` and indexed by
//! registration order ("any kind, in registration order" - its own doc
//! comment), used by `ActivateAbility`'s `ability_index` field (still
//! positional as of `phase5/ability-stable-id` - it's chosen and consumed in
//! the same synchronous step, never at risk the way `PendingTrigger` was).
//! Nothing about that mechanism is Attach-specific or equip-specific - two
//! `RegisteredAbility` entries with the same `source` and
//! `effect: Some(AbilityEffect::Attach)` already resolve independently by
//! index, each keeping its own distinct `target`/`cost`. This slice is pure
//! proof (a real test exercising two differently-restricted equip abilities
//! on one card, both by hand-built `RegisteredAbility` and by a script
//! registering two `Game.CreateActivatedAbility` builders), not a fix - same
//! "the TODO over-pattern-matched against a different, real wall" shape
//! `phase5/layer-engine-consumers` already found once for a different gap.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

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
        toughness: Some(PTValue::Fixed(2)),
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

/// "Test Dual Equipment: Equip {0}. Equip Dragon {0}." - CR 702.6c's
/// "Equip [quality]" restriction on the second ability only, proving the two
/// indices keep genuinely separate target filters, not a shared one.
fn equipment_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Dual Equipment".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: vec![Subtype("Equipment".to_string())],
        oracle_text: "Test Dual Equipment: Equip {0}. Equip Dragon {0}.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_dual_equipment".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// Index 0: "Equip {0}" - any creature you control.
fn equip_any(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility { id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                controller: Some(FilterController::You),
                zone: None,
            },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Attach),
        intervening_if: None,
    }
}

/// Index 1: "Equip Dragon {0}" - CR 702.6c, only a Dragon you control.
fn equip_dragon(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility { id: AbilityId(1),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: Some(vec![Subtype("Dragon".to_string())]),
                supertypes: None,
                controller: Some(FilterController::You),
                zone: None,
            },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Attach),
        intervening_if: None,
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
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

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, ability_index: u32, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index, targets, chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the ability's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn either_of_two_equip_abilities_may_be_activated_keeping_its_own_restriction() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let dragon = ObjectId(1);
    let equipment = ObjectId(2);
    put_on_battlefield(&mut state, permanent(bear, creature_definition(1, "Test Bear", Vec::new()), p0));
    put_on_battlefield(&mut state, permanent(dragon, creature_definition(2, "Test Dragon", vec![Subtype("Dragon".to_string())]), p0));
    put_on_battlefield(&mut state, permanent(equipment, equipment_definition(), p0));
    // Registration order fixes the index: equip_any is 0, equip_dragon is 1.
    state.abilities.push(equip_any(equipment));
    state.abilities.push(equip_dragon(equipment));

    let mut engine = Engine::new(state);

    // Index 0 ("Equip {0}") can target the non-Dragon bear.
    activate(&mut engine, p0, equipment, 0, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().object(equipment).unwrap().attached_to, Some(bear), "index 0's own \"any creature\" restriction should have accepted the bear");

    // Index 1 ("Equip Dragon {0}") retargets the same Equipment to the
    // Dragon - proves it's a genuinely separate ability, not index 0 reused.
    activate(&mut engine, p0, equipment, 1, vec![Target::Object(dragon)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().object(equipment).unwrap().attached_to, Some(dragon), "index 1 should have reattached to the Dragon it targeted");

    // Index 1 rejects the non-Dragon bear - its own restriction is real,
    // not silently sharing index 0's laxer one.
    activate(&mut engine, p0, equipment, 1, vec![Target::Object(bear)]);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "\"Equip Dragon\" (index 1) must reject a non-Dragon creature, got {status:?}"
    );
}

const TWO_EQUIP_ABILITIES: &str = r#"
local s = {}
function s.initial_effect(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "object", card_types = { "Creature" }, controller = "you" })
    a:SetOperation(Game.Ops.Attach)
    a:Register()

    local b = Game.CreateActivatedAbility(c)
    b:SetTarget({ type = "object", card_types = { "Creature" }, subtypes = { "Dragon" }, controller = "you" })
    b:SetOperation(Game.Ops.Attach)
    b:Register()
end
return s
"#;

#[test]
fn a_script_can_self_register_two_distinct_equip_abilities_on_one_card() {
    let mut state = base_state();
    let equipment = ObjectId(0);
    let mut definition = equipment_definition();
    definition.script_ref = ScriptReference::Inline(TWO_EQUIP_ABILITIES.to_string());
    put_on_battlefield(&mut state, permanent(equipment, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, equipment).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 2, "both Register() calls must produce their own RegisteredAbility entry");
    assert!(matches!(
        &state.abilities[0].target,
        Some(TargetSpec::Primitive(req)) if matches!(&req.filter, TargetFilter::Object { subtypes: None, .. })
    ));
    assert!(matches!(
        &state.abilities[1].target,
        Some(TargetSpec::Primitive(req)) if matches!(
            &req.filter,
            TargetFilter::Object { subtypes: Some(s), .. } if s == &vec![Subtype("Dragon".to_string())]
        )
    ));
}
