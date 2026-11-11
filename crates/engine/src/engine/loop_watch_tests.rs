//! Unit tests for the CR 104.4b fingerprint and the watch's clearing rule. Played
//! loops are in `tests/loop_draw.rs`.

use std::collections::VecDeque;

use super::atomicity_tests::{definition, object};
use super::loop_watch::fingerprint;
use super::tests::{minimal_state, trigger};
use super::*;
use crate::types::card::ModeChoice;
use crate::types::command::{PlayerAction, PlayerActionKind};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{AbilityId, EventId, PlayerId, ScriptHandle, SequenceNo};
use crate::types::object::GameObject;
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::turn::{Phase, Step};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn two_players() -> GameState {
    let mut state = minimal_state();
    state.turn_order.push(P1);
    state
}

fn logged(state: &mut GameState, kind: GameEventKind) {
    let n = state.ledger.len() as u64;
    state.ledger.push(GameEvent {
        event_id: EventId(n),
        sequence_no: SequenceNo(n),
        kind,
        caused_by_command: None,
        parent_event: None,
    });
}

#[test]
fn the_fingerprint_relabels_fresh_ids_and_drops_counters() {
    // CR 104.4b
    let mut a = two_players();
    a.pending_timing_bypass = Some(ObjectId(3));
    let mut b = two_players();
    b.pending_timing_bypass = Some(ObjectId(41));
    b.next_object_id = 42;
    b.next_event_id = 7;
    b.next_sequence_no = 9;
    logged(
        &mut b,
        GameEventKind::RngDrawn {
            purpose: "test".to_string(),
            value: 1,
        },
    );
    assert_eq!(fingerprint(&a), fingerprint(&b));
}

fn trigger_on_stack(stack_id: u64, source: u64) -> StackObject {
    StackObject {
        ability_index: None,
        ability_text: None,
        stack_id: ObjectId(stack_id),
        kind: StackEntryKind::TriggeredAbility,
        source: ObjectId(source),
        source_lki: None,
        owner: P0,
        controller: P0,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        leave_stack_rider: None,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    }
}

#[test]
fn the_fingerprint_relabels_a_fresh_stack_id() {
    // CR 104.4b
    let mut a = two_players();
    a.stack.push(trigger_on_stack(7, 2));
    let mut b = two_players();
    b.stack.push(trigger_on_stack(30, 2));
    assert_eq!(fingerprint(&a), fingerprint(&b));
}

#[test]
fn the_fingerprint_keeps_id_order() {
    // CR 104.4b
    let mut a = two_players();
    a.pending_timing_bypass = Some(ObjectId(1));
    a.pending_free_cast = Some(ObjectId(2));
    let mut b = two_players();
    b.pending_timing_bypass = Some(ObjectId(2));
    b.pending_free_cast = Some(ObjectId(1));
    assert_ne!(fingerprint(&a), fingerprint(&b));
}

#[test]
fn the_fingerprint_sees_real_differences() {
    // CR 104.4b
    let base = two_players();
    let mut life = two_players();
    life.players[0].life -= 1;
    let mut passes = two_players();
    passes.consecutive_passes = 1;
    let mut rng = two_players();
    rng.rng.draws_consumed += 1;
    let card = object(ObjectId(5), definition(Vec::new(), ModeChoice::Exactly(1)));
    let mut untapped = two_players();
    untapped.objects.insert(ObjectId(5), card.clone());
    let mut tapped = two_players();
    tapped.objects.insert(
        ObjectId(5),
        GameObject {
            tapped: true,
            ..card
        },
    );
    assert_ne!(fingerprint(&base), fingerprint(&life));
    assert_ne!(fingerprint(&base), fingerprint(&passes));
    assert_ne!(fingerprint(&base), fingerprint(&rng));
    assert_ne!(fingerprint(&untapped), fingerprint(&tapped));
}

#[test]
fn hash_collections_compare_by_content() {
    // CR 104.4b
    let mut a = two_players();
    let mut b = two_players();
    for n in 0..32 {
        a.once_per_turn_triggered.insert(AbilityId(n));
        b.once_per_turn_triggered.insert(AbilityId(31 - n));
        a.lands_played_this_turn
            .insert(PlayerId(n as u32), n as u32);
        b.lands_played_this_turn
            .insert(PlayerId(31 - n as u32), 31 - n as u32);
    }
    assert_eq!(fingerprint(&a), fingerprint(&b));
    b.lands_played_this_turn.insert(PlayerId(0), 5);
    assert_ne!(fingerprint(&a), fingerprint(&b));
}

#[test]
fn the_fingerprint_sees_ledger_derived_values() {
    // CR 121.2b
    let base = two_players();
    let mut drew = two_players();
    let card = object(ObjectId(9), definition(Vec::new(), ModeChoice::Exactly(1)));
    logged(
        &mut drew,
        GameEventKind::ZoneChange {
            object: ObjectId(10),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(card),
        },
    );
    assert_ne!(fingerprint(&base), fingerprint(&drew));

    // CR 702.54a
    let mut damaged = two_players();
    logged(
        &mut damaged,
        GameEventKind::DamageDealt {
            source: ObjectId(1),
            target: Target::Player(P1),
            amount: 1,
            is_combat_damage: false,
        },
    );
    assert_ne!(fingerprint(&base), fingerprint(&damaged));
}

#[test]
fn literals_keep_their_braces_and_ids() {
    // CR 104.4b
    // An id or a brace inside a quoted name must not be relabeled or sorted.
    let mut a = two_players();
    let mut card = object(ObjectId(5), definition(Vec::new(), ModeChoice::Exactly(1)));
    card.definition.name = "ObjectId(900) {b, a}".to_string();
    a.objects.insert(ObjectId(5), card.clone());
    let printed = fingerprint(&a);
    assert!(printed.contains("\"ObjectId(900) {b, a}\""));
}

#[test]
fn a_player_choice_clears_the_watch() {
    // CR 104.4b
    let mut engine = Engine::new(minimal_state());
    engine.process_state = ProcessState::AwaitTriggerOrderChoice {
        player: P0,
        group: (1..=2).map(|n| trigger(n, P0)).collect(),
        ordered: Vec::new(),
        pending_groups: VecDeque::new(),
    };
    engine.loop_watch.push("seen".to_string());
    engine.step();
    assert!(engine.loop_watch.is_empty());
}

#[test]
fn a_priority_window_keeps_the_watch() {
    // CR 104.4b
    let mut engine = Engine::new(minimal_state());
    engine.state.phase = crate::types::turn::Phase::Beginning;
    engine.loop_watch.push("seen".to_string());
    engine.step();
    assert_eq!(engine.loop_watch.len(), 1);
}

#[test]
fn a_pass_on_an_empty_stack_clears_the_watch() {
    // CR 104.4b
    let mut state = minimal_state();
    state.phase = Phase::Main;
    state.step = Step::PreCombatMain;
    let mut engine = Engine::new(state);
    engine.loop_watch.push("seen".to_string());
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    assert!(engine.loop_watch.is_empty());
}

#[test]
fn a_forced_pass_with_a_stack_is_watched() {
    // CR 104.4b
    let mut state = minimal_state();
    state.phase = Phase::Main;
    state.step = Step::PreCombatMain;
    state.stack.push(trigger_on_stack(7, 2));
    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    assert_eq!(engine.loop_watch.len(), 1);
}

#[test]
fn the_fingerprint_sees_a_permission_duration() {
    // CR 104.4b, 611.2a
    let permission = crate::types::player::TimingPermission {
        source: ObjectId(5),
        controller: P0,
        affected: crate::types::cost::FilterController::You,
        spells: crate::types::cost::FilterSpec::ANY,
        duration: crate::types::effect::EffectDuration::WhileSourceOnBattlefield,
    };
    let mut fixed = two_players();
    fixed.timing_permissions.push(permission.clone());
    let mut timed = two_players();
    timed
        .timing_permissions
        .push(crate::types::player::TimingPermission {
            duration: crate::types::effect::EffectDuration::UntilEndOfTurn,
            ..permission
        });
    assert_ne!(fingerprint(&fixed), fingerprint(&timed));
}
