use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{ActionError, Engine, EngineSnapshot, StepStatus};
use stackfish_engine::*;

use crate::drain;

/// Where an action was turned down: straight away by `submit_action`, or
/// when `step()` ran the queued command.
#[derive(Debug)]
pub enum Rejected {
    AtSubmit(ActionError),
    AtStep(CommandError),
}

/// The parts of `before` and `after` that differ, by name. Empty means the
/// two snapshots are equal.
pub fn snapshot_diff(before: &EngineSnapshot, after: &EngineSnapshot) -> Vec<String> {
    let mut parts = Vec::new();
    macro_rules! check {
        ($($field:ident),* $(,)?) => {
            $(if before.state.$field != after.state.$field {
                parts.push(format!("state.{}", stringify!($field)));
            })*
        };
    }
    check!(
        players,
        turn_order,
        active_player,
        turn_number,
        phase,
        step,
        priority_player,
        consecutive_passes,
        lands_played_this_turn,
        combat,
        last_active_turn_number,
        objects,
        zones,
        stack,
        continuous_effects,
        replacement_effects,
        cost_modifiers,
        hand_size_modifiers,
        hand_reveal_modifiers,
        draw_limit_modifiers,
        land_play_modifiers,
        abilities,
        ledger,
        rng,
        next_event_id,
        next_sequence_no,
        next_object_id,
        pending_step_skips,
        once_per_turn_triggered,
        commander_return_offered,
        linked_ability_generation,
        script_ability_ids,
        ability_function_zones,
        off_battlefield_scripts_run,
        linked_exiles,
        pending_timing_bypass,
        pending_free_cast,
    );
    if parts.is_empty() && before.state != after.state {
        parts.push("state (a field missing from snapshot_diff)".to_string());
    }
    if before.next_command_id != after.next_command_id {
        parts.push(format!("next_command_id {} -> {}", before.next_command_id, after.next_command_id));
    }
    if before.script_handles != after.script_handles {
        parts.push(format!("script_handles {} -> {}", before.script_handles, after.script_handles));
    }
    if before.transient != after.transient {
        parts.push(format!("engine fields:\n  before: {}\n  after:  {}", before.transient, after.transient));
    }
    parts
}

/// Panics, naming what changed, unless `engine` matches `before` exactly.
pub fn assert_unchanged(before: &EngineSnapshot, engine: &Engine, context: &str) {
    let diff = snapshot_diff(before, &engine.snapshot());
    assert!(diff.is_empty(), "{context}: a rejected input changed the engine: {}", diff.join(", "));
}

/// Runs `f`, then asserts the engine is exactly as it was before. Returns
/// what `f` returned so the caller can also check the rejection itself.
pub fn assert_unchanged_after<R>(engine: &mut Engine, context: &str, f: impl FnOnce(&mut Engine) -> R) -> R {
    let before = engine.snapshot();
    let result = f(engine);
    assert_unchanged(&before, engine, context);
    result
}

/// Drives `action` through `submit_action` and `step()` and asserts it is
/// rejected and changes nothing. A rejection is allowed to come from either
/// call: `submit_action` queues the action and `step()` runs it, so most
/// rejections only show up at `step()` (ticket 035 moves them earlier).
pub fn assert_action_rejected(engine: &mut Engine, context: &str, action: PlayerAction) -> Rejected {
    let before = engine.snapshot();
    let rejected = match engine.submit_action(action) {
        Err(err) => Rejected::AtSubmit(err),
        Ok(()) => match drain(engine) {
            StepStatus::Error(err) => Rejected::AtStep(err),
            other => panic!("{context}: expected the action to be rejected, but the engine went on to {other:?}"),
        },
    };
    assert_unchanged(&before, engine, context);
    rejected
}
