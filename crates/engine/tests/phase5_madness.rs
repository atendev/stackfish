//! Phase 5 vertical slice: CR 702.35 Madness.
//!
//! Two abilities: a static "if this card would be discarded, exile it
//! instead" (already fully generic - `ReplacementPayload::
//! ReplaceZoneDestination(Exile)` matched against `EventFilter::
//! WouldMoveZone(Hand, Graveyard)`, the same "Leyline of the Void" pattern
//! both real discard producers (cleanup hand-size discard and a cost-payment
//! `Discard`) already route through), and a triggered "when this card is
//! exiled this way, you may cast it by paying [cost] rather than its mana
//! cost; if you don't, put it into your graveyard."
//!
//! New pieces: `GameEventKind::MadnessExiled`/`ConditionPrimitive::
//! MadnessExiled` (a purpose-built marker, same shape `Cycled` already
//! established, logged only when the winning redirect was this object's own
//! - see `replacement::madness_marker_event`'s own doc comment), `AbilityEffect
//! ::MadnessCastOrDiscard` (always pauses - `ProcessState::
//! AwaitingMadnessChoice` - since the "yes" branch is a full nested cast),
//! and `GameState::pending_timing_bypass` (a one-shot CR 601.3 timing waiver
//! consumed by `commands::cast::execute_cast_spell`, since CR 702.35a lets a
//! player cast the card as the triggered ability resolves, ignoring the
//! normal sorcery-speed/priority rules that ability text would otherwise be
//! bound by).

use std::collections::HashMap;

use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

/// "Test Madness Bolt" - a SORCERY (deliberately, to prove the timing
/// bypass: sorcery-speed casting is normally illegal outside a main phase
/// with an empty stack and priority, exactly the situation a madness cast
/// happens in) with a bare `Madness {R}` alternative cost from exile. No
/// target/effect - this slice only needs to prove the zone-move/pause
/// machinery, not any particular resolution effect (same posture
/// `phase5_flashback_alternative_costs.rs`'s own `bolt_definition` already
/// takes).
fn madness_bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Madness Bolt".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Test Madness Bolt deals 2 damage to any target. Madness {R}".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_madness_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Madness {R}".to_string(),
            cost: CostSpec::mana_only(ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] }),
            from_zone: ZoneKind::Exile,
            exile_on_leaving_stack: false,
            escape: false,
        }],
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn filler_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
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
        script_ref: ScriptReference::ByKey("test_filler".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Discard Outlet: Discard a card: Draw a card." - the cost-payment
/// discard producer (as opposed to cleanup hand-size discard), so Madness's
/// redirect can be proven against both real discard producers this engine
/// has, same dual-producer coverage `phase5_cycling_triggers.rs` established
/// for `Cycled`.
fn discard_outlet_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Discard Outlet".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Discard a card: Draw a card.".to_string(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_discard_outlet".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn discard_outlet_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec {
            mana: None,
            additional: vec![AdditionalCost::Discard {
                filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            }],
            alternative: Vec::new(),
        }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::DrawCards(1)),
        intervening_if: None,
    }
}

/// CR 702.35a's static half - "if this card would be discarded, exile it
/// instead," self-registered on the card itself. Already fully generic
/// engine machinery (`phase5/replaceable-cleanup-discard`/`phase5/
/// replaceable-cost-zone-moves`) - this slice's only job is the marker event
/// (`madness_marker_event`) that distinguishes this from any other exile-
/// instead-of-discard effect.
fn madness_redirect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Hand),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

/// CR 702.35a's triggered half - "when this card is exiled this way, you may
/// cast it... if you don't, put it into your graveyard."
fn madness_trigger(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::MadnessExiled)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::MadnessCastOrDiscard),
        intervening_if: None,
    }
}

/// End-step setup mirroring `phase5_cycling_triggers.rs`'s own
/// `two_player_state` - a hand-size-over-the-limit p0 will hit
/// `StepStatus::AwaitingDiscard` once both players pass End-step priority.
fn two_player_state(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Battlefield, ZoneKind::Exile]))
    }
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

/// Drives a hand-size-8 p0 through both End-step passes to the cleanup
/// discard, then discards `card` specifically, then drains once more so any
/// resulting MadnessExiled trigger is already on the stack.
fn discard_to_cleanup(engine: &mut Engine, p0: PlayerId, p1: PlayerId, card: ObjectId) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected p0 end-step priority, got {status:?}");
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "expected p1 end-step priority, got {status:?}");
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 8, max_hand_size: 7 } if player == p0), "got {status:?}");
    engine.submit_discard(vec![card]).expect("valid discard");
    drain(engine)
}

fn setup_hand_with_seven_filler(state: &mut GameState, p0: PlayerId) {
    for i in 0..7 {
        let id = ObjectId(100 + i);
        state.objects.insert(id, card_object(id, filler_definition(100 + i, "Vanilla"), p0, ZoneKind::Hand));
        state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(id);
    }
}

#[test]
fn cleanup_discard_redirects_to_exile_and_the_madness_trigger_fires_exactly_once() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool::default());
    state.objects.insert(card, card_object(card, madness_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.replacement_effects.push(madness_redirect(card));
    state.abilities.push(madness_trigger(card));
    setup_hand_with_seven_filler(&mut state, p0);

    let mut engine = Engine::new(state);
    let status = discard_to_cleanup(&mut engine, p0, p1, card);

    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Hand, ZoneKind::Exile), "madness redirect must have exiled instead of discarding");
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.contains(&new_id));
    assert!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.is_empty(), "never touched the graveyard");
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::MadnessExiled { .. })),
        "expected a MadnessExiled marker event"
    );
    assert_eq!(engine.state().stack.len(), 1, "exactly one trigger, not one per hand card");
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "CR 514.3: a new trigger reopens priority, got {status:?}");
}

#[test]
fn declining_puts_the_exiled_card_into_the_graveyard() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool::default());
    state.objects.insert(card, card_object(card, madness_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.replacement_effects.push(madness_redirect(card));
    state.abilities.push(madness_trigger(card));
    setup_hand_with_seven_filler(&mut state, p0);

    let mut engine = Engine::new(state);
    discard_to_cleanup(&mut engine, p0, p1, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let (exiled_id, player) = match status {
        StepStatus::AwaitingMadnessChoice { player, card } => (card, player),
        other => panic!("expected AwaitingMadnessChoice, got {other:?}"),
    };
    assert_eq!(player, p0);

    engine.decline_madness_cast().expect("valid decline");
    let status = drain(&mut engine);

    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty(), "must have left exile");
    assert!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.iter().any(|&id| id != exiled_id),
        "the declined card landed in the graveyard under a fresh id"
    );
    assert!(engine.state().stack.is_empty(), "the trigger itself finished resolving");
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
}

/// The core new mechanism this slice adds: casting via madness happens *as*
/// the triggered ability resolves (CR 702.35a), not as a later ordinary
/// priority action - proven here with a SORCERY, cast during the End step
/// (no main phase, and the trigger itself is mid-resolution) where an
/// ordinary sorcery-speed cast would be illegal. Also proves `exile_on_
/// leaving_stack: false` was the right call for Madness (unlike Flashback):
/// once cast, the spell resolves into the graveyard normally, not back into
/// exile.
#[test]
fn casting_via_madness_bypasses_sorcery_speed_timing_and_resolves_to_the_graveyard() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool::default());
    state.objects.insert(card, card_object(card, madness_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.replacement_effects.push(madness_redirect(card));
    state.abilities.push(madness_trigger(card));
    setup_hand_with_seven_filler(&mut state, p0);

    let mut engine = Engine::new(state);
    discard_to_cleanup(&mut engine, p0, p1, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let exiled_card = match status {
        StepStatus::AwaitingMadnessChoice { card, .. } => card,
        other => panic!("expected AwaitingMadnessChoice, got {other:?}"),
    };

    // Still the End step, non-empty pipeline context, no main phase - an
    // ordinary CastSpell of a sorcery would be illegal here.
    engine
        .submit_madness_cast(Vec::new(), Vec::new(), None)
        .expect("madness cast bypasses sorcery-speed timing");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "the madness {{R}} cost still needs paying, got {status:?}");

    // Floated now, inside the cleanup step's own priority window, rather
    // than seeded into the pre-cleanup End-step state above - CR 500.5
    // empties the pool as End ends (`discard_to_cleanup` crosses that
    // boundary), so mana seeded before it wouldn't survive to pay with here.
    engine.state_mut().player_mut(p0).unwrap().mana_pool.add_unit(ManaUnit::Colored(Color::Red));
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }] })
        .expect("madness's {R} paid with the floated red mana");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(engine.state().object(exiled_card).is_none(), "the exiled card left exile to become the spell on the stack");

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");

    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Graveyard), "resolves straight to the graveyard, not back into exile");
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty());
}

/// While `AwaitingMadnessChoice` is pending, the engine isn't sitting in
/// `ProcessState::PriorityWindow` - so an ordinary priority action (anything
/// other than `decline_madness_cast`/`submit_madness_cast`) must be
/// rejected, same "sub-decision mid-pipeline" stance every other
/// `Awaiting*Choice` pause in this engine already takes.
#[test]
fn ordinary_priority_actions_are_rejected_while_awaiting_the_madness_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state(ManaPool::default());
    state.objects.insert(card, card_object(card, madness_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.replacement_effects.push(madness_redirect(card));
    state.abilities.push(madness_trigger(card));
    setup_hand_with_seven_filler(&mut state, p0);

    let mut engine = Engine::new(state);
    discard_to_cleanup(&mut engine, p0, p1, card);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingMadnessChoice { .. }), "got {status:?}");

    let result = engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority });
    assert!(matches!(result, Err(ActionError::IllegalAction(_))), "expected rejection while mid-pipeline, got {result:?}");
}

/// CR 702.35a via the *other* real discard producer - a cost-payment
/// `Discard`, not just the cleanup hand-size discard every other test above
/// drives through. Proves `replacement::madness_marker_event` is hooked at
/// both real commit sites (`engine::turn::continue_discard_moves` and
/// `engine::payment::continue_cost_zone_moves`), same dual-producer coverage
/// `phase5_cycling_triggers.rs` established for `Cycled`.
#[test]
fn a_cost_payment_discard_also_triggers_madness() {
    let p0 = PlayerId(0);
    let _p1 = PlayerId(1);
    let card = ObjectId(0);
    let outlet = ObjectId(1);
    let mut state = two_player_state(ManaPool::default());
    state.objects.insert(card, card_object(card, madness_bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.replacement_effects.push(madness_redirect(card));
    state.abilities.push(madness_trigger(card));
    state.objects.insert(
        outlet,
        GameObject { controlled_since_turn: 1, ..card_object(outlet, discard_outlet_definition(2), p0, ZoneKind::Battlefield) },
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(outlet);
    state.abilities.push(discard_outlet_ability(outlet));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source: outlet, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        .expect("p0 controls the outlet and holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(card)] })
        .expect("discarding the madness card pays the outlet's Discard cost");
    drain(&mut engine);

    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Hand, ZoneKind::Exile), "madness redirect must fire off a cost-payment discard too");
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.contains(&new_id));
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::MadnessExiled { .. })),
        "expected a MadnessExiled marker event off the cost-payment discard"
    );
    // Outlet ability plus the MadnessExiled trigger it raised.
    assert_eq!(engine.state().stack.len(), 2);
}
