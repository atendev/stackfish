//! CR 613.1a, 707: layer 1 copy effects. A copy freezes its values when applied (CR 707.2b/c), so it is a `GameObject.copiable_override` snapshot that `effective_characteristics` reads ahead of `definition`, and not a `ContinuousEffect`.
//! Two entry points write it, `AbilityEffect::BecomeCopy` and `TokenSpec::CopyOf`. Both purge the copier's pre-copy abilities, apply `CopyExceptions` (CR 707.9), and re-run `run_card_script` against the snapshot (CR 707.2a).
// TODO: stack copies (CR 707.10, Fork) and double-faced-card copying (CR 707.8)

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

const SELF_REGISTER_UNCONDITIONAL: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

fn bear_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Bear".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(SELF_REGISTER_UNCONDITIONAL.to_string()),
        keywords: KeywordSet::FLYING,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn salamander_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Salamander".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(5)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn doppelganger_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 3,
        name: "Doppelganger".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        // CR 704.5f: nonzero toughness so the doppelganger survives to activate BecomeCopy.
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn instant_definition(id: u64, name: &str, effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(effect),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn become_copy_ability(source: ObjectId) -> RegisteredAbility {
    become_copy_ability_with_exceptions(source, CopyExceptions::default())
}

fn become_copy_ability_with_exceptions(
    source: ObjectId,
    exceptions: CopyExceptions,
) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::BecomeCopy(exceptions)),
        intervening_if: None,
    }
}

fn one_player_state(objects: Vec<GameObject>, abilities: Vec<RegisteredAbility>) -> GameState {
    let p0 = PlayerId(0);
    let mut objects_map = HashMap::new();
    let mut bf_contents = VecDeque::new();
    for obj in objects {
        bf_contents.push_back(obj.object_id);
        objects_map.insert(obj.object_id, obj);
    }
    GameState {
        players: vec![PlayerState {
            player_id: p0,
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
        }],
        turn_order: vec![p0],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: objects_map,
        zones: vec![Zone {
            kind: ZoneKind::Battlefield,
            owner: None,
            contents: bf_contents,
        }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        hand_reveal_modifiers: Vec::new(),
        draw_limit_modifiers: Vec::new(),
        land_play_modifiers: Vec::new(),
        abilities,
        ledger: Vec::new(),
        rng: RngState {
            seed: 1,
            draws_consumed: 0,
        },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
        linked_ability_generation: std::collections::HashMap::new(),
        script_ability_ids: std::collections::HashSet::new(),
        ability_function_zones: std::collections::HashMap::new(),
        off_battlefield_scripts_run: std::collections::HashSet::new(),
        linked_exiles: Vec::new(),
        pending_timing_bypass: None,
        pending_free_cast: None,
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets,
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected payment pause, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("free cost");
    drain(engine)
}

fn activate_pay_and_resolve(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) {
    activate(engine, player, source, targets);
    pay_free_and_drain(engine);
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority");
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

// CR 707.2, 707.3
// "Become a copy of target creature" changes name, P/T and keywords through effective_characteristics.
#[test]
fn become_copy_changes_effective_characteristics() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(chars.name, "Bear");
    assert_eq!(chars.power, Some(2));
    assert_eq!(chars.toughness, Some(2));
    assert!(chars.keywords.contains(KeywordSet::FLYING));
    assert!(
        engine.state().object(doppel).is_some(),
        "CR 707.4: becoming a copy while remaining on the battlefield doesn't mint a new ObjectId"
    );
}

// CR 707.9e, 707.9g
// copy_generation counts distinct copy applications. The copy targets are scriptless so a second activation leaves no unrelated trigger on the stack.
#[test]
fn become_copy_bumps_copy_generation() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let target_a = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(target_a, salamander_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel)],
    );
    let mut engine = Engine::new(state);
    assert_eq!(
        engine.state().object(doppel).unwrap().copy_generation,
        0,
        "never copied yet"
    );

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(target_a)]);
    assert_eq!(
        engine.state().object(doppel).unwrap().copy_generation,
        1,
        "one real copy application"
    );
    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name,
        "Salamander"
    );

    // become_copy_ability was purged with the pre-copy abilities, so it is re-granted to drive a second BecomeCopy.
    let target_b = ObjectId(2);
    engine.state_mut().objects.insert(
        target_b,
        card_object(
            target_b,
            doppelganger_definition(),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    engine
        .state_mut()
        .abilities
        .push(become_copy_ability(doppel));
    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(target_b)]);
    assert_eq!(
        engine.state().object(doppel).unwrap().copy_generation,
        2,
        "a second, distinct copy application"
    );
    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name,
        "Doppelganger"
    );
}

// CR 707.9a
// A recurring-copy exception (Vesuvan Doppelganger) is purged and re-granted on each copy with the new copy_generation, so linked_ability_generation's supersession check is never reached through this producer.
#[test]
fn become_copy_with_grant_recurring_copy_trigger_regenerates_the_linked_ability_each_time() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let target_a = ObjectId(1);
    let exceptions = CopyExceptions {
        grant_recurring_copy_trigger: true,
        ..CopyExceptions::default()
    };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(target_a, salamander_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability_with_exceptions(
            doppel,
            exceptions.clone(),
        )],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(target_a)]);
    let linked_after_first: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == doppel && a.kind == AbilityKind::Triggered)
        .collect();
    assert_eq!(
        linked_after_first.len(),
        1,
        "the linked ability was granted"
    );
    let first_id = linked_after_first[0].id;
    assert_eq!(
        engine
            .state()
            .linked_ability_generation
            .get(&first_id)
            .copied(),
        Some(1)
    );

    let target_b = ObjectId(2);
    engine.state_mut().objects.insert(
        target_b,
        card_object(
            target_b,
            doppelganger_definition(),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    engine
        .state_mut()
        .abilities
        .push(become_copy_ability_with_exceptions(doppel, exceptions));
    // ability_index 1, since the linked ability from the first copy sits at index 0 until this activation purges it.
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: doppel,
                ability_index: 1,
                targets: vec![Target::Object(target_b)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
    pay_free_and_drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);

    let linked_after_second: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == doppel && a.kind == AbilityKind::Triggered)
        .collect();
    assert_eq!(
        linked_after_second.len(),
        1,
        "still exactly one linked ability, not two stacked"
    );
    assert_ne!(
        linked_after_second[0].id, first_id,
        "the first application's own ability was purged outright, not left in place"
    );
    assert_eq!(
        engine
            .state()
            .linked_ability_generation
            .get(&linked_after_second[0].id)
            .copied(),
        Some(2)
    );
    assert_eq!(
        engine
            .state()
            .linked_ability_generation
            .get(&first_id)
            .copied(),
        Some(1),
        "the old tag entry is never removed (linked_ability_generation only ever grows), but its own ability is long gone"
    );
}

// CR 707.2a
// The copy's abilities come from the copied text, so doppel ends with exactly Bear's ability and not both.
#[test]
fn become_copy_purges_old_abilities_and_registers_the_copys_own() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let doppel_abilities: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == doppel)
        .collect();
    assert_eq!(
        doppel_abilities.len(),
        1,
        "exactly Bear's granted ability, not both old and new"
    );
    assert!(matches!(
        doppel_abilities[0].effect,
        Some(AbilityEffect::GainLife(1))
    ));

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(events.iter().any(|e| matches!(e, GameEventKind::BecameCopy { object, copied } if *object == doppel && *copied == bear)));
}

// CR 707.9b
// A P/T exception overrides the copy's base power and toughness, and name and keywords still come from the copied object.
#[test]
fn become_copy_with_pt_exception_overrides_base_power_and_toughness() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let exceptions = CopyExceptions {
        set_power_toughness: Some((1, 1)),
        add_keywords: KeywordSet::empty(),
        retain_name: false,
        retain_color: false,
        grant_recurring_copy_trigger: false,
        enters_with_additional_counters: None,
    };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability_with_exceptions(doppel, exceptions)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(
        chars.name, "Bear",
        "name still comes from the copied object"
    );
    assert_eq!(chars.power, Some(1), "overridden by the P/T exception");
    assert_eq!(chars.toughness, Some(1), "overridden by the P/T exception");
    assert!(
        chars.keywords.contains(KeywordSet::FLYING),
        "keywords still come from the copied object"
    );
}

// CR 707.9a
// A keyword exception adds to what the copied object grants and does not replace it.
#[test]
fn become_copy_with_keyword_exception_adds_the_keyword() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let salamander = ObjectId(1);
    let exceptions = CopyExceptions {
        set_power_toughness: None,
        add_keywords: KeywordSet::TRAMPLE,
        retain_name: false,
        retain_color: false,
        grant_recurring_copy_trigger: false,
        enters_with_additional_counters: None,
    };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(
                salamander,
                salamander_definition(),
                p0,
                ZoneKind::Battlefield,
            ),
        ],
        vec![become_copy_ability_with_exceptions(doppel, exceptions)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(salamander)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(chars.name, "Salamander");
    assert_eq!(
        chars.power,
        Some(5),
        "P/T still comes from the copied object - no P/T exception this time"
    );
    assert!(
        chars.keywords.contains(KeywordSet::TRAMPLE),
        "granted by the keyword exception"
    );
}

// CR 707.9e
// Spark Double's additional-effect exception adds a +1/+1 counter as a GameObject mutation, not a CardDefinition one.
#[test]
fn become_copy_with_additional_counter_exception_adds_a_real_counter() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let exceptions = CopyExceptions {
        enters_with_additional_counters: Some((
            CounterKind::PlusOnePlusOne,
            CounterAmount::Fixed(1),
        )),
        ..CopyExceptions::default()
    };
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability_with_exceptions(doppel, exceptions)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel);
    assert_eq!(
        chars.name, "Bear",
        "the copy itself still applies as normal"
    );
    assert_eq!(
        engine
            .state()
            .object(doppel)
            .unwrap()
            .counters
            .count(&CounterKind::PlusOnePlusOne),
        1,
        "the additional-effect exception granted a real counter"
    );
}

// A bare copy grants no counter.
#[test]
fn become_copy_without_the_exception_grants_no_counter() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);

    assert_eq!(
        engine
            .state()
            .object(doppel)
            .unwrap()
            .counters
            .count(&CounterKind::PlusOnePlusOne),
        0
    );
}

// CR 707.3
// Copying a copy uses its current copiable values, so a Clone copying a Doppelganger that copied a Bear ends up a Bear.
#[test]
fn become_copy_of_a_copy_uses_its_current_copiable_values() {
    let p0 = PlayerId(0);
    let clone_obj = ObjectId(0);
    let already_copied = ObjectId(1);
    let mut already_copied_obj = card_object(
        already_copied,
        doppelganger_definition(),
        p0,
        ZoneKind::Battlefield,
    );
    // already_copied has become a copy of Bear before this BecomeCopy resolves.
    already_copied_obj.copiable_override = Some(bear_definition());
    let state = one_player_state(
        vec![
            card_object(
                clone_obj,
                doppelganger_definition(),
                p0,
                ZoneKind::Battlefield,
            ),
            already_copied_obj,
        ],
        vec![become_copy_ability(clone_obj)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(
        &mut engine,
        p0,
        clone_obj,
        vec![Target::Object(already_copied)],
    );

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), clone_obj);
    assert_eq!(
        chars.name, "Bear",
        "should copy Bear's values, not the Doppelganger's own printed name"
    );
}

// CR 707.2b
// Changing the original's copiable values afterward does not change the earlier copy, since the snapshot is a clone.
#[test]
fn copying_the_original_again_later_does_not_change_an_earlier_copy() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![become_copy_ability(doppel)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, vec![Target::Object(bear)]);
    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name,
        "Bear"
    );

    // `bear` itself now becomes a copy of something else entirely.
    engine
        .state_mut()
        .object_mut(bear)
        .unwrap()
        .copiable_override = Some(salamander_definition());
    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), bear).name,
        "Salamander"
    );

    // `doppel`'s own earlier copy is unaffected - still Bear, not Salamander.
    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name,
        "Bear",
        "doppel's copy was frozen at copy-time (CR 707.2b) and must not follow bear's later change"
    );
}

// A BecomeCopy with no legal target, such as a player, is a silent no-op.
#[test]
fn become_copy_with_no_object_target_is_a_no_op() {
    let p0 = PlayerId(0);
    let doppel = ObjectId(0);
    let bear = ObjectId(1);
    let state = one_player_state(
        vec![
            card_object(doppel, doppelganger_definition(), p0, ZoneKind::Battlefield),
            card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
        ],
        vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(1),
            source: doppel,
            kind: AbilityKind::Activated,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::BecomeCopy(CopyExceptions::default())),
            intervening_if: None,
        }],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, Vec::new());

    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name,
        "Doppelganger"
    );
}

// CR 707.10f, 707.2a
// A copy token matches the copied object's current copiable values and self-registers its abilities.
#[test]
fn copy_token_matches_source_characteristics_and_abilities() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    state.objects.insert(
        bear,
        card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(bear);

    let card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                10,
                "Clone Spell",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(bear, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf
        .contents
        .iter()
        .find(|&&id| id != bear)
        .copied()
        .expect("the copy token landed");
    let token = engine.state().object(token_id).unwrap();
    assert!(token.is_token);
    assert_eq!(token.definition.name, "Bear");
    assert!(matches!(token.definition.power, Some(PTValue::Fixed(2))));
    assert!(token.definition.keywords.contains(KeywordSet::FLYING));

    let token_abilities: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == token_id)
        .collect();
    assert_eq!(
        token_abilities.len(),
        1,
        "the copy token self-registered Bear's granted ability"
    );
    assert!(matches!(
        token_abilities[0].effect,
        Some(AbilityEffect::GainLife(1))
    ));
}

// CR 707.9b
// A copy token's P/T exception overrides its base values before the snapshot is frozen.
#[test]
fn copy_token_with_pt_exception_overrides_base_power_and_toughness() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    state.objects.insert(
        bear,
        card_object(bear, bear_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(bear);

    let exceptions = CopyExceptions {
        set_power_toughness: Some((1, 1)),
        add_keywords: KeywordSet::empty(),
        retain_name: false,
        retain_color: false,
        grant_recurring_copy_trigger: false,
        enters_with_additional_counters: None,
    };
    let card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                10,
                "Clone Spell",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(bear, exceptions),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf
        .contents
        .iter()
        .find(|&&id| id != bear)
        .copied()
        .expect("the copy token landed");
    let token = engine.state().object(token_id).unwrap();
    assert_eq!(
        token.definition.name, "Bear",
        "name still comes from the copied object"
    );
    assert!(
        matches!(token.definition.power, Some(PTValue::Fixed(1))),
        "overridden by the P/T exception"
    );
    assert!(
        matches!(token.definition.toughness, Some(PTValue::Fixed(1))),
        "overridden by the P/T exception"
    );
}

// A CopyOf naming an unresolvable ObjectId creates no token.
#[test]
fn copy_token_of_a_missing_source_creates_nothing() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let missing = ObjectId(999);

    let card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(
                10,
                "Clone Spell",
                AbilityEffect::CreateToken {
                    spec: TokenSpec::CopyOf(missing, CopyExceptions::default()),
                    count: 1,
                },
            ),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .is_empty(),
        "no token was created"
    );
}

#[test]
fn game_ops_become_copy_decodes_pt_and_keyword_exceptions() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation(
            r#"return Game.Ops.BecomeCopy({ power = 1, toughness = 1, keywords = { "Trample" } })"#,
        )
        .expect("primitive loads");
    let Some(AbilityEffect::BecomeCopy(exceptions)) = effect else {
        panic!("expected Some(AbilityEffect::BecomeCopy(_)), got {effect:?}")
    };
    assert_eq!(exceptions.set_power_toughness, Some((1, 1)));
    assert!(exceptions.add_keywords.contains(KeywordSet::TRAMPLE));
}

#[test]
fn game_ops_become_copy_with_no_opts_is_a_bare_copy() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation("return Game.Ops.BecomeCopy()")
        .expect("primitive loads");
    let Some(AbilityEffect::BecomeCopy(exceptions)) = effect else {
        panic!("expected Some(AbilityEffect::BecomeCopy(_)), got {effect:?}")
    };
    assert_eq!(exceptions.set_power_toughness, None);
    assert_eq!(exceptions.add_keywords, KeywordSet::empty());
}

#[test]
fn game_ops_become_copy_rejects_power_without_toughness() {
    let mut runtime = ScriptRuntime::new();
    let result = runtime.load_operation("return Game.Ops.BecomeCopy({ power = 1 })");
    assert!(
        result.is_err(),
        "power/toughness must be set together, not just one"
    );
}
