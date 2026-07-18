//! Phase 5 vertical slice: CR 613.1a/707 layer 1 (copy effects) - backlog
//! item 1.5c. Unlike every other layer (2, 4-7), layer 1 has no
//! `ContinuousEffect`/`EffectPrimitive` case and is never queried through
//! `layers::applicable_effects_in_layer` - CR 707.2b/707.2c require a copy's
//! granted values to freeze at the moment it's applied, which the other
//! layers' "re-derive from `state.continuous_effects` on every call" shape
//! would violate outright. Real support is `GameObject.copiable_override`
//! instead: a frozen snapshot written once by whichever of the two entry
//! points below produced it, read by `layers::effective_characteristics`
//! ahead of `definition`.
//!
//! Two entry points: `AbilityEffect::BecomeCopy` (an existing permanent
//! re-copying another object, e.g. Vesuvan Doppelganger's "become a copy of
//! target creature") and `TokenSpec::CopyOf` (a freshly created token that's
//! a copy of another object, e.g. Clone-as-token-generator text). Both purge
//! whatever the copying object had self-registered under its pre-copy
//! identity and re-run `ScriptRuntime::run_card_script` against the new
//! snapshot (CR 707.2a: abilities are copiable).
//!
//! Narrower than the full rules text - see this slice's own pre-slice
//! scoping notes in docs/implementation.md: no CR 707.9 exception/
//! modification support (always a bare copy), no ETB-replacement "enters as
//! a copy" path (Clone itself), no stack-copy (CR 707.10, Fork-style), no
//! double-faced-card copying (CR 707.8).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

const SELF_REGISTER_UNCONDITIONAL: &str = r#"
local s = {}
function s.initial_effect(c)
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
        mana_cost: ManaCost { symbols: Vec::new() },
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
        effect: None,
        modes: Vec::new(),
    }
}

fn salamander_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Salamander".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
        effect: None,
        modes: Vec::new(),
    }
}

fn doppelganger_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 3,
        name: "Doppelganger".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        // CR 704.5f: a 0-toughness creature would die to SBAs before this
        // slice's own machinery ever runs - nonzero so the doppelganger
        // survives to actually activate BecomeCopy.
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn instant_definition(id: u64, name: &str, effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
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
        effect: Some(effect),
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
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

fn become_copy_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { card_types: Some(CardTypeSet::CREATURE), controller: None, zone: None },
            count: 1,
            up_to: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::BecomeCopy),
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
        zones: vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: bf_contents }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities,
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

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
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
            },
            PlayerState {
                player_id: p1,
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
            },
        ],
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
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
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

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected payment pause, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("free cost");
    drain(engine)
}

/// Activates the ability (no cost), pays through the resulting
/// `AwaitingPayment` pause with an empty plan, then passes priority once
/// (sole player) so the pushed `ActivatedAbility` resolves.
fn activate_pay_and_resolve(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    activate(engine, player, source, targets);
    pay_free_and_drain(engine);
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("player holds priority");
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

// ── AbilityEffect::BecomeCopy ───────────────────────────────────────────

/// CR 707.2/707.3: activating "become a copy of target creature" changes
/// the activator's *effective* characteristics (via `layers::
/// effective_characteristics`, the same query every other layer uses) to
/// the target's - name, P/T, and keywords all come from the copy.
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

/// CR 707.2a: the copy's abilities come from the copied text, and the
/// object doesn't keep its pre-copy abilities alongside them - the
/// `BecomeCopy` ability itself (sourced from `doppel`) is purged by the
/// same resolution that grants Bear's self-registered `GainLife(1)`
/// trigger, so `doppel` ends up with exactly Bear's ability, not both.
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

    let doppel_abilities: Vec<_> = engine.state().abilities.iter().filter(|a| a.source == doppel).collect();
    assert_eq!(doppel_abilities.len(), 1, "exactly Bear's granted ability, not both old and new");
    assert!(matches!(doppel_abilities[0].effect, Some(AbilityEffect::GainLife(1))));

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(events.iter().any(|e| matches!(e, GameEventKind::BecameCopy { object, copied } if *object == doppel && *copied == bear)));
}

/// CR 707.3: copying an object that's itself already a copy uses *that*
/// object's current copiable values, not its own printed `CardDefinition` -
/// a Clone copying a Vesuvan Doppelganger that already copied a Bear ends
/// up a Bear, not a Doppelganger.
#[test]
fn become_copy_of_a_copy_uses_its_current_copiable_values() {
    let p0 = PlayerId(0);
    let clone_obj = ObjectId(0);
    let already_copied = ObjectId(1);
    let mut already_copied_obj = card_object(already_copied, doppelganger_definition(), p0, ZoneKind::Battlefield);
    // Simulates `already_copied` having already become a copy of Bear
    // before this test's own BecomeCopy resolves.
    already_copied_obj.copiable_override = Some(bear_definition());
    let state = one_player_state(
        vec![card_object(clone_obj, doppelganger_definition(), p0, ZoneKind::Battlefield), already_copied_obj],
        vec![become_copy_ability(clone_obj)],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, clone_obj, vec![Target::Object(already_copied)]);

    let chars = effective_characteristics(engine.state(), &ScriptRuntime::new(), clone_obj);
    assert_eq!(chars.name, "Bear", "should copy Bear's values, not the Doppelganger's own printed name");
}

/// CR 707.2b: once an object has been copied, changing the copiable values
/// the *original* now grants (e.g. it becomes a copy of something else
/// afterward) does not retroactively change the earlier copy - the frozen
/// `copiable_override` snapshot is a real clone, not a live reference back
/// to the source.
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
    assert_eq!(effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name, "Bear");

    // `bear` itself now becomes a copy of something else entirely.
    engine.state_mut().object_mut(bear).unwrap().copiable_override = Some(salamander_definition());
    assert_eq!(effective_characteristics(engine.state(), &ScriptRuntime::new(), bear).name, "Salamander");

    // `doppel`'s own earlier copy is unaffected - still Bear, not Salamander.
    assert_eq!(
        effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name,
        "Bear",
        "doppel's copy was frozen at copy-time (CR 707.2b) and must not follow bear's later change"
    );
}

/// A `BecomeCopy` with no legal target (e.g. targeting a player) is a
/// silent no-op, same "skip rather than guess" stance every other
/// unreachable-target arm in this engine takes.
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
            source: doppel,
            kind: AbilityKind::Activated,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::BecomeCopy),
            intervening_if: None,
        }],
    );
    let mut engine = Engine::new(state);

    activate_pay_and_resolve(&mut engine, p0, doppel, Vec::new());

    assert_eq!(effective_characteristics(engine.state(), &ScriptRuntime::new(), doppel).name, "Doppelganger");
}

// ── TokenSpec::CopyOf ───────────────────────────────────────────────────

/// CR 707.10f: "create a token that's a copy of [object]" - the token's
/// characteristics match the copied object's current copiable values, and
/// (CR 707.2a) it self-registers the copied abilities, unlike every other
/// `TokenSpec` (which has no script at all).
#[test]
fn copy_token_matches_source_characteristics_and_abilities() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);

    let card = ObjectId(1);
    state.objects.insert(
        card,
        card_object(
            card,
            instant_definition(10, "Clone Spell", AbilityEffect::CreateToken { spec: TokenSpec::CopyOf(bear), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents.iter().find(|&&id| id != bear).copied().expect("the copy token landed");
    let token = engine.state().object(token_id).unwrap();
    assert!(token.is_token);
    assert_eq!(token.definition.name, "Bear");
    assert!(matches!(token.definition.power, Some(PTValue::Fixed(2))));
    assert!(token.definition.keywords.contains(KeywordSet::FLYING));

    let token_abilities: Vec<_> = engine.state().abilities.iter().filter(|a| a.source == token_id).collect();
    assert_eq!(token_abilities.len(), 1, "the copy token self-registered Bear's granted ability");
    assert!(matches!(token_abilities[0].effect, Some(AbilityEffect::GainLife(1))));
}

/// `TokenSpec::CopyOf` naming an `ObjectId` that doesn't resolve creates no
/// token - "skip rather than guess," same stance every other unresolvable
/// reference in this engine takes.
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
            instant_definition(10, "Clone Spell", AbilityEffect::CreateToken { spec: TokenSpec::CopyOf(missing), count: 1 }),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "no token was created");
}
