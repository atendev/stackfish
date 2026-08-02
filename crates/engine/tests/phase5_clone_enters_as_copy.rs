//! Phase 5 vertical slice: CR 707.6/614.12a "enter as a copy" (Clone
//! itself) - "You may have this creature enter as a copy of any creature on
//! the battlefield." The last of the explicitly named layer-1/copy gaps
//! (`copy-exceptions`/`layer-engine-copy`'s own notes): `BecomeCopy` (an
//! existing permanent re-copying something) and `TokenSpec::CopyOf` (a
//! token copying something) both already existed; this is the third real
//! entry point - a card itself choosing, as it enters, to become a copy.
//!
//! Same category of gap as Devour/Amplify: `ReplacementPayload::EntersAsCopy`
//! is never applied by `replacement::apply_enters_battlefield_modifiers` at
//! all - `replacement::pending_copy_choice` is checked directly by
//! `ResolveStackTop`, pausing on `ProcessState::AwaitingCopyChoice` and
//! resuming via `Engine::submit_copy_choice`.
//!
//! Unlike Devour/Amplify, a real choice reuses `AbilityEffect::BecomeCopy`'s
//! own machinery wholesale (purge + re-run `run_card_script`, `CopyExceptions`
//! applied first) and logs the same `GameEventKind::BecameCopy`. The resume
//! then always runs `apply_enters_battlefield_modifiers` (copied or not),
//! with the entering spell's own real `costs_paid`-derived `colors_spent`
//! carried through - this is what makes the comp rules' own worked example
//! real: a Clone copying a Fading + "enters tapped" creature enters tapped
//! with fade counters too (CR 707.2's Skyshroud Behemoth example).

use stackfish_engine::engine::{CopyChoiceError, Engine, StepStatus};
use stackfish_engine::layers;
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
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
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn artifact_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn enters_as_copy_effect(source: ObjectId) -> ReplacementEffect {
    enters_as_copy_effect_with_exceptions(source, CopyExceptions::default())
}

fn enters_as_copy_effect_with_exceptions(source: ObjectId, exceptions: CopyExceptions) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::EntersAsCopy(exceptions),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

// ---------------------------------------------------------------------
// pending_copy_choice - unit level.
// ---------------------------------------------------------------------

#[test]
fn pending_copy_choice_offers_every_battlefield_creature_regardless_of_controller() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let entrant = ObjectId(0);
    let own_bear = ObjectId(1);
    let opponents_bear = ObjectId(2);
    let own_artifact = ObjectId(3);
    state.objects.insert(entrant, card_object(entrant, creature_definition("Clone", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(enters_as_copy_effect(entrant));
    put_on_battlefield(&mut state, card_object(own_bear, creature_definition("Own Bear", ""), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(opponents_bear, creature_definition("Foe Bear", ""), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(own_artifact, artifact_definition("Not A Creature"), p0, ZoneKind::Battlefield));

    let runtime = ScriptRuntime::new();
    let (_exceptions, mut candidates) =
        replacement::pending_copy_choice(&state, &runtime, entrant, ZoneKind::Stack).expect("an EntersAsCopy effect is registered with real candidates");
    candidates.sort_by_key(|id| id.0);

    // Both bears (regardless of controller) and the entrant itself (a
    // creature) all qualify; the non-creature artifact doesn't.
    let mut expected = vec![entrant, own_bear, opponents_bear];
    expected.sort_by_key(|id| id.0);
    assert_eq!(candidates, expected);
}

#[test]
fn pending_copy_choice_is_none_when_no_creatures_are_on_the_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let entrant = ObjectId(0);
    // The entrant itself isn't a creature this time either.
    state.objects.insert(entrant, card_object(entrant, artifact_definition("Odd Clone"), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(enters_as_copy_effect(entrant));

    let runtime = ScriptRuntime::new();
    assert!(replacement::pending_copy_choice(&state, &runtime, entrant, ZoneKind::Stack).is_none());
}

#[test]
fn pending_copy_choice_is_none_without_a_registered_effect() {
    let state = two_player_state();
    let runtime = ScriptRuntime::new();
    assert!(replacement::pending_copy_choice(&state, &runtime, ObjectId(0), ZoneKind::Stack).is_none());
}

// ---------------------------------------------------------------------
// Lua self-registration.
// ---------------------------------------------------------------------

const CLONE_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy())
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

#[test]
fn set_payload_enters_as_copy_registers_correctly() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition("Clone", CLONE_SCRIPT);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    ScriptRuntime::new().run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { to: Some(ZoneKind::Battlefield), .. },
            payload: ReplacementPayload::EntersAsCopy(exceptions),
        } => {
            assert_eq!(exceptions.set_power_toughness, None);
            assert_eq!(exceptions.add_keywords, KeywordSet::empty());
        }
        other => panic!("expected a WouldMoveZone(_, Battlefield)/EntersAsCopy spec, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// End to end: Lua registration + real CastSpell resolution + the pause.
// ---------------------------------------------------------------------

/// Skyshroud Behemoth, per CR 707.2's own worked example: "Fading 2" (enters
/// with two counters, modeled here as Charge) and "This creature enters
/// tapped."
const BEHEMOTH_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local tapped = Game.CreateReplacementEffect(c)
    tapped:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    tapped:SetPayload(Game.ReplacementPayloads.EntersTapped)
    tapped:SetAffected({ self_only = true })
    tapped:Register()

    local fade = Game.CreateReplacementEffect(c)
    fade:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    fade:SetPayload(Game.ReplacementPayloads.EntersWithCounters("Charge", 2))
    fade:SetAffected({ self_only = true })
    fade:Register()
end
return s
"#;

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
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

#[test]
fn choosing_to_copy_a_fading_and_tapped_creature_applies_both_on_top_of_the_copy() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let behemoth = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Clone", CLONE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(behemoth, creature_definition("Skyshroud Behemoth", BEHEMOTH_SCRIPT), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { player, object, candidates } = status else {
        panic!("expected a CR 707.6 copy choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|id| id.0);
    let mut expected = vec![object, behemoth];
    expected.sort_by_key(|id| id.0);
    assert_eq!(sorted, expected, "the entrant itself plus the Behemoth are both legal candidates");

    engine.submit_copy_choice(Some(behemoth)).expect("the Behemoth was an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution should finish cleanly, got {status:?}");

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::BecameCopy { object: o, copied } if o == object && copied == behemoth)),
        "the copy event was logged"
    );
    let runtime = ScriptRuntime::new();
    let now = layers::effective_characteristics(engine.state(), &runtime, object);
    assert_eq!(now.name, "Skyshroud Behemoth", "the entrant's name is now the copied creature's");
    assert!(engine.state().object(object).unwrap().tapped, "the copy's own EntersTapped applied");
    assert_eq!(
        engine.state().object(object).unwrap().counters.count(&CounterKind::Charge),
        2,
        "the copy's own Fading-equivalent EntersWithCounters applied too"
    );
    assert_eq!(
        engine.state().object(object).unwrap().copy_generation, 1,
        "CR 707.9e/g: EntersAsCopy is a real, distinct copy application"
    );
}

#[test]
fn choosing_not_to_copy_leaves_the_entrant_as_itself() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let behemoth = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Clone", CLONE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(behemoth, creature_definition("Skyshroud Behemoth", BEHEMOTH_SCRIPT), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else { panic!("expected a copy choice pause, got {status:?}") };

    engine.submit_copy_choice(None).expect("declining to copy is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let runtime = ScriptRuntime::new();
    let now = layers::effective_characteristics(engine.state(), &runtime, object);
    assert_eq!(now.name, "Clone", "declining leaves the entrant as its own printed self");
    assert!(!engine.state().object(object).unwrap().tapped);
    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::Charge), 0);
    assert_eq!(engine.state().object(object).unwrap().copy_generation, 0, "declining is not a copy application");
}

/// Spark Double, per CR 707.9e's "additional effect" exception shape:
/// "...if it's a creature, it enters with an additional +1/+1 counter on
/// it." Exercises the Lua-side `counter_kind`/`counter_amount` decode too
/// (`decode_copy_exceptions`), not just the Rust-level field.
const SPARK_DOUBLE_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy({ counter_kind = "PlusOnePlusOne", counter_amount = 1 }))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

#[test]
fn choosing_to_copy_applies_the_707_9e_additional_counter_exception() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Spark Double", SPARK_DOUBLE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else { panic!("expected a copy choice pause, got {status:?}") };

    engine.submit_copy_choice(Some(bear)).expect("the Bear was an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let runtime = ScriptRuntime::new();
    let now = layers::effective_characteristics(engine.state(), &runtime, object);
    assert_eq!(now.name, "Bear", "the entrant's name is now the copied creature's");
    assert_eq!(
        engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        1,
        "CR 707.9e: the additional-effect exception applied on top of the copy"
    );
}

#[test]
fn declining_to_copy_does_not_apply_the_additional_counter_exception() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Spark Double", SPARK_DOUBLE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else { panic!("expected a copy choice pause, got {status:?}") };

    engine.submit_copy_choice(None).expect("declining to copy is always legal");
    drain(&mut engine);

    assert_eq!(
        engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        0,
        "no copy application happened, so the exception never applied"
    );
}

#[test]
fn copy_choice_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Clone", CLONE_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_copy_choice(Some(ObjectId(999))).unwrap_err();
    assert!(matches!(err, CopyChoiceError::InvalidCandidate));
}

#[test]
fn submit_copy_choice_while_not_awaiting_is_rejected() {
    let mut engine = Engine::new(two_player_state());
    let err = engine.submit_copy_choice(None).unwrap_err();
    assert!(matches!(err, CopyChoiceError::NotAwaitingChoice));
}

#[test]
fn a_creature_without_entersascopy_never_pauses() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Plain Bear", ""), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = pay_free_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no EntersAsCopy effect registered - no pause, got {status:?}");
}

// ---------------------------------------------------------------------
// CR 707.9a: Vesuvan Doppelganger's own "except it has [linked upkeep
// ability]" - phase5/vesuvan-linked-copy-ability.
// ---------------------------------------------------------------------

const VESUVAN_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersAsCopy({ grant_recurring_copy_trigger = true }))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

/// CR 707.9a: choosing to copy grants the linked "at the beginning of your
/// upkeep, become a copy of target creature" ability alongside whatever the
/// copied creature's own abilities are - a real, distinct `RegisteredAbility`
/// tagged in `linked_ability_generation` with the generation this copy
/// application produced.
#[test]
fn choosing_a_vesuvan_style_copy_grants_the_linked_upkeep_ability() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Vesuvan Doppelganger", VESUVAN_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else {
        panic!("expected a CR 707.6 copy choice pause, got {status:?}")
    };

    engine.submit_copy_choice(Some(bear)).expect("bear was an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().object(object).unwrap().copy_generation, 1);
    let linked: Vec<_> = engine
        .state()
        .abilities
        .iter()
        .filter(|a| a.source == object && a.kind == AbilityKind::Triggered)
        .collect();
    assert_eq!(linked.len(), 1, "exactly the granted linked ability - Bear itself has no abilities of its own");
    assert!(matches!(linked[0].condition, Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep))));
    assert!(matches!(linked[0].effect, Some(AbilityEffect::BecomeCopy(_))));
    assert_eq!(
        engine.state().linked_ability_generation.get(&linked[0].id).copied(),
        Some(1),
        "tagged with the copy_generation this application produced"
    );
}

/// Declining the copy grants nothing - no linked ability, no bump.
#[test]
fn declining_a_vesuvan_style_copy_grants_no_linked_ability() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, creature_definition("Vesuvan Doppelganger", VESUVAN_SCRIPT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    put_on_battlefield(&mut state, card_object(bear, creature_definition("Bear", ""), p1, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingCopyChoice { object, .. } = status else {
        panic!("expected a CR 707.6 copy choice pause, got {status:?}")
    };

    engine.submit_copy_choice(None).expect("declining is always legal");
    drain(&mut engine);

    assert_eq!(engine.state().object(object).unwrap().copy_generation, 0);
    assert!(engine.state().abilities.iter().all(|a| a.source != object), "no linked ability granted");
    assert!(engine.state().linked_ability_generation.is_empty());
}
