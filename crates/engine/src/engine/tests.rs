use super::*;
use super::triggers_pipeline::is_permutation;
use crate::replacement;
use crate::triggers::PendingTrigger;
use crate::types::counter::CounterSet;
use crate::types::effect::ConsumptionPolicy;
use crate::types::ids::{AbilityId, ObjectId, PlayerId, Target};
use crate::types::mana::ManaPool;
use crate::types::player::PlayerState;
use crate::types::turn::Step;
use std::collections::{HashMap, VecDeque};

fn minimal_state() -> GameState {
    let p0 = PlayerId(0);
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
        phase: Phase::PreGame,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: Vec::new(),
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        hand_reveal_modifiers: Vec::new(),
        draw_limit_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: crate::types::game::RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 0,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
        linked_ability_generation: std::collections::HashMap::new(),
        pending_timing_bypass: None,
        pending_free_cast: None,
    }
}

fn trigger(id: u64, controller: PlayerId) -> PendingTrigger {
    PendingTrigger { source: ObjectId(id), controller, ability: AbilityId(id), once_per_turn: false }
}

#[test]
fn is_permutation_accepts_reorderings_and_rejects_substitutions() {
    let group = vec![trigger(1, PlayerId(0)), trigger(2, PlayerId(0))];
    // A true reordering (same elements, different sequence) must pass.
    assert!(is_permutation(&[group[1], group[0]], &group));
    // Same length but a substituted element must be rejected - length
    // alone can't catch this.
    assert!(!is_permutation(&[group[0], trigger(3, PlayerId(0))], &group));
    // A duplicate of one element instead of using both must be rejected.
    assert!(!is_permutation(&[group[0], group[0]], &group));
    // Wrong length, caught the same way as before.
    assert!(!is_permutation(&[group[0]], &group));
}

#[test]
fn submit_trigger_order_rejects_non_permutation_and_accepts_real_reordering() {
    let player = PlayerId(0);
    let group = vec![trigger(1, player), trigger(2, player)];

    let mut engine = Engine::new(minimal_state());
    engine.process_state = ProcessState::AwaitTriggerOrderChoice {
        player,
        group: group.clone(),
        ordered: Vec::new(),
        pending_groups: VecDeque::new(),
    };
    let substituted = vec![group[0], trigger(99, player)];
    let err = engine.submit_trigger_order(substituted).unwrap_err();
    assert!(matches!(err, TriggerOrderError::NotAPermutation));

    // Engine must still be sitting in AwaitTriggerOrderChoice - a
    // rejected answer must not have consumed the pending state.
    assert!(matches!(engine.process_state, ProcessState::AwaitTriggerOrderChoice { .. }));

    let reordered = vec![group[1], group[0]];
    engine.submit_trigger_order(reordered).expect("a true reordering must be accepted");
    assert!(matches!(engine.process_state, ProcessState::PutTriggersApnap { .. }));
}

// ---------------------------------------------------------------------
// Replacement/prevention (CR 614-616) - real ReplacementCheck/
// AwaitingReplacementChoice pipeline wiring, driven directly (not
// through combat/SBA - see docs/implementation.md's own note on why
// those two producers can't exercise this for real yet).
// ---------------------------------------------------------------------

fn dummy_object(id: ObjectId, controller: PlayerId) -> crate::types::object::GameObject {
    use crate::types::card::{CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet};
    use crate::types::object::VisibilityPolicy;
    crate::types::object::GameObject {
        object_id: id,
        lineage: crate::types::ids::ObjectLineage { previous: None },
        definition: crate::types::card::CardDefinition {
            definition_id: 1,
            name: "Test Object".to_string(),
            mana_cost: crate::types::mana::ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: Vec::new(),
            oracle_text: String::new(),
            power: Some(PTValue::Fixed(1)),
            toughness: Some(PTValue::Fixed(1)),
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::ByKey("test".to_string()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        },
        copiable_override: None,
        owner: controller,
        controller,
        zone: crate::types::zone::ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None, attached_to_player: None,
        timestamp: crate::types::ids::SequenceNo(0),
        controlled_since_turn: 0,
        is_commander: false,
        is_token: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        copy_generation: 0,
        entered_via_escape: false,
    }
}

fn prevent_all_damage(source: ObjectId) -> crate::types::effect::ReplacementEffect {
    use crate::types::effect::{EffectFilter, EventFilter, ReplacementPayload, ReplacementSpec};
    crate::types::effect::ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt {
                affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::PreventDamage,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn replacement_check_applies_single_match_and_commits_rewritten_event() {
    let p0 = PlayerId(0);
    let effect_source = ObjectId(0);
    let mut state = minimal_state();
    state.objects.insert(effect_source, dummy_object(effect_source, p0));
    state.replacement_effects.push(prevent_all_damage(effect_source));

    let mut engine = Engine::new(state);
    let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(p0), amount: 7, is_combat_damage: false };
    engine.process_state =
        ProcessState::ReplacementCheck { remaining: VecDeque::from([event]), checked: Vec::new() };

    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            _ => break,
        }
    }

    let last = engine.state().ledger.last().expect("CommitEvents must have appended the resolved event");
    assert!(
        matches!(last.kind, GameEventKind::DamageDealt { amount: 0, .. }),
        "the registered PreventDamage effect must have zeroed the committed event's amount"
    );
}

#[test]
fn submit_replacement_choice_rejects_invalid_candidate_and_not_awaiting() {
    let mut engine = Engine::new(minimal_state());
    let err = engine.submit_replacement_choice(replacement::ReplacementCandidate::Registered(ObjectId(0))).unwrap_err();
    assert!(matches!(err, ReplacementChoiceError::NotAwaitingChoice));

    engine.process_state = ProcessState::AwaitingReplacementChoice {
        affected_player: PlayerId(0),
        pending_event: GameEventKind::DamageDealt {
            source: ObjectId(9),
            target: Target::Player(PlayerId(0)),
            amount: 1,
            is_combat_damage: false,
        },
        candidates: vec![
            replacement::ReplacementCandidate::Registered(ObjectId(1)),
            replacement::ReplacementCandidate::Registered(ObjectId(2)),
        ],
        excluded: Vec::new(),
        resume: ReplacementResumeTarget::ReplacementCheck { remaining: VecDeque::new(), checked: Vec::new() },
    };
    let err = engine
        .submit_replacement_choice(replacement::ReplacementCandidate::Registered(ObjectId(99)))
        .unwrap_err();
    assert!(matches!(err, ReplacementChoiceError::InvalidCandidate));
    // A rejected answer must not have consumed the pending state.
    assert!(matches!(engine.process_state, ProcessState::AwaitingReplacementChoice { .. }));
}

#[test]
fn submit_replacement_choice_applies_chosen_then_resolves_the_other_and_commits() {
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    let mut state = minimal_state();
    state.objects.insert(a, dummy_object(a, p0));
    state.objects.insert(b, dummy_object(b, p0));
    state.replacement_effects.push(prevent_all_damage(a));
    state.replacement_effects.push(prevent_all_damage(b));

    let mut engine = Engine::new(state);
    let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(p0), amount: 7, is_combat_damage: false };
    engine.process_state =
        ProcessState::ReplacementCheck { remaining: VecDeque::from([event]), checked: Vec::new() };

    // Both effects match unrestricted - one step() call should pause on
    // the CR 616.1 ordering choice rather than guessing an order.
    assert!(matches!(engine.step().status, StepStatus::Progressed));
    let ProcessState::AwaitingReplacementChoice { candidates, .. } = &engine.process_state else {
        panic!("expected AwaitingReplacementChoice - two effects matched the same event");
    };
    assert_eq!(candidates.len(), 2);

    engine
        .submit_replacement_choice(replacement::ReplacementCandidate::Registered(a))
        .expect("a is a real candidate");
    // b still matches the (already-prevented) event since it isn't
    // excluded yet - this drains it too, then commits once nothing else
    // matches (CR 616.1: a is now excluded and can't reapply).
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            _ => break,
        }
    }

    let last = engine.state().ledger.last().expect("CommitEvents must have appended the resolved event");
    assert!(matches!(last.kind, GameEventKind::DamageDealt { amount: 0, .. }));
}

// ---------------------------------------------------------------------
// CR 303.4d attach choice - dormant as of this slice (see
// AttachChoiceError's own doc comment): no real effect in this engine
// ever produces more than one simultaneous attach candidate for one
// Aura, so - same as AwaitingReplacementChoice's own unit tests above -
// these drive the pause by constructing ProcessState::AwaitingAttachChoice
// directly rather than through a real card/spell pipeline.
// ---------------------------------------------------------------------

#[test]
fn submit_attach_choice_rejects_invalid_candidate_and_not_awaiting() {
    let mut engine = Engine::new(minimal_state());
    let err = engine.submit_attach_choice(ObjectId(0)).unwrap_err();
    assert!(matches!(err, AttachChoiceError::NotAwaitingChoice));

    engine.process_state = ProcessState::AwaitingAttachChoice {
        aura: ObjectId(0),
        controller: PlayerId(0),
        candidates: vec![ObjectId(1), ObjectId(2)],
    };
    let err = engine.submit_attach_choice(ObjectId(99)).unwrap_err();
    assert!(matches!(err, AttachChoiceError::InvalidCandidate));
    // A rejected answer must not have consumed the pending state.
    assert!(matches!(engine.process_state, ProcessState::AwaitingAttachChoice { .. }));
}

#[test]
fn attach_choice_surfaces_via_step_and_a_valid_answer_attaches_and_resumes() {
    let p0 = PlayerId(0);
    let aura = ObjectId(0);
    let candidate_a = ObjectId(1);
    let candidate_b = ObjectId(2);
    let mut state = minimal_state();
    state.objects.insert(aura, dummy_object(aura, p0));
    state.objects.insert(candidate_a, dummy_object(candidate_a, p0));
    state.objects.insert(candidate_b, dummy_object(candidate_b, p0));

    let mut engine = Engine::new(state);
    engine.process_state = ProcessState::AwaitingAttachChoice {
        aura,
        controller: p0,
        candidates: vec![candidate_a, candidate_b],
    };

    let status = engine.step().status;
    let StepStatus::AwaitingAttachChoice { player, aura: paused_aura, candidates } = status else {
        panic!("expected AwaitingAttachChoice, got {status:?}");
    };
    assert_eq!(player, p0);
    assert_eq!(paused_aura, aura);
    assert_eq!(candidates, vec![candidate_a, candidate_b]);

    engine.submit_attach_choice(candidate_a).expect("candidate_a is a real candidate");
    assert!(matches!(engine.process_state, ProcessState::PriorityWindow));
    assert_eq!(engine.state().object(aura).unwrap().attached_to, Some(candidate_a));
    assert!(engine.state().object(candidate_a).unwrap().attachments.contains(&aura));
    assert!(!engine.state().object(candidate_b).unwrap().attachments.contains(&aura));
}
