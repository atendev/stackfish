//! Meta-tests for the atomicity harness in `stackfish-testutil`. A harness
//! that never fails proves nothing, so these check that it does fail when an
//! input changes the engine, however small the change, and that it names
//! what changed.

use std::panic::{catch_unwind, AssertUnwindSafe};

use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::{
    assert_action_rejected, assert_unchanged_after, card_object, minimal_card_definition, snapshot_diff,
    two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);

fn engine() -> Engine {
    let mut state = two_player_state(zones(P0, PlayerId(1), &[ZoneKind::Hand, ZoneKind::Battlefield]));
    let id = ObjectId(1);
    let definition = minimal_card_definition(1, "Bear", "bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE);
    state.objects.insert(id, card_object(id, definition, P0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    Engine::new(state)
}

fn panic_message(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("the harness should have panicked");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn a_call_that_changes_nothing_passes() {
    let mut engine = engine();
    let answer = assert_unchanged_after(&mut engine, "no-op", |e| e.state().turn_number);
    assert_eq!(answer, 1);
}

#[test]
fn a_call_that_flips_a_flag_fails_and_names_the_field() {
    let mut engine = engine();
    let message = panic_message(|| {
        assert_unchanged_after(&mut engine, "tap", |e| {
            e.state_mut().object_mut(ObjectId(1)).unwrap().tapped = true;
        });
    });
    assert!(message.contains("state.objects"), "{message}");
}

#[test]
fn a_call_that_only_advances_a_counter_fails() {
    let mut engine = engine();
    let message = panic_message(|| {
        assert_unchanged_after(&mut engine, "id", |e| {
            e.state_mut().allocate_object_id();
        });
    });
    assert!(message.contains("state.next_object_id"), "{message}");
}

#[test]
fn a_call_that_appends_to_the_ledger_fails() {
    let mut engine = engine();
    let message = panic_message(|| {
        assert_unchanged_after(&mut engine, "ledger", |e| {
            let event_id = e.state_mut().allocate_event_id();
            let sequence_no = e.state_mut().allocate_sequence_no();
            e.state_mut().ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind: GameEventKind::Tapped { object: ObjectId(1) },
                caused_by_command: None,
                parent_event: None,
            });
        });
    });
    assert!(message.contains("state.ledger"), "{message}");
}

/// The shape of the bugs the audit found: a call that mutates and then returns `Err`.
#[test]
fn a_mutate_then_err_call_fails_even_though_it_returns_err() {
    let mut engine = engine();
    let message = panic_message(|| {
        let _: Result<(), ()> = assert_unchanged_after(&mut engine, "mutate then err", |e| {
            e.state_mut().object_mut(ObjectId(1)).unwrap().damage_marked = 1;
            Err(())
        });
    });
    assert!(message.contains("state.objects"), "{message}");
}

#[test]
fn an_accepted_action_is_not_a_rejection() {
    let mut engine = engine();
    let message = panic_message(|| {
        assert_action_rejected(&mut engine, "pass", PlayerAction { player: P0, kind: PlayerActionKind::PassPriority });
    });
    assert!(message.contains("expected the action to be rejected"), "{message}");
}

#[test]
fn an_accepted_submit_changes_the_snapshot_so_it_cannot_masquerade_as_a_rejection() {
    let mut engine = engine();
    let before = engine.snapshot();
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    let diff = snapshot_diff(&before, &engine.snapshot());
    assert!(!diff.is_empty(), "queuing an action must show up in the snapshot");
}

#[test]
fn identical_snapshots_have_no_diff_and_a_command_id_difference_is_reported() {
    let mut engine = engine();
    let first = engine.snapshot();
    assert!(snapshot_diff(&first, &engine.snapshot()).is_empty());
    let mut second = engine.snapshot();
    second.next_command_id += 1;
    assert_eq!(snapshot_diff(&first, &second).len(), 1);
    second.script_handles += 1;
    assert_eq!(snapshot_diff(&first, &second).len(), 2);
    engine.state_mut().turn_number = 9;
}
