use std::collections::HashMap;

use stackfish_engine::*;
use stackfish_engine::engine::Engine;

use crate::schema::*;
use crate::actions::{build_payment_plan, resolve_card, resolve_target, to_player_action};
use crate::assertions::{AssertionOutcome, check_assertion};
use crate::auto_pay::chain_auto_pay;
use crate::build::{build_initial_state, parse_phase, parse_step, register_script_operation, register_script_target_requirement, register_target_script, register_trigger_ability};
use crate::build_new_game::build_new_game_engine;
use crate::load::load;
use crate::run::{StepOutcome, run_action, run_advance, run_bottom_cards, run_discard, run_mulligan, run_payment, run_script_choice, run_unwind_payment};

pub enum EntryOutcome {
    Action(StepOutcome),
    Assertion(AssertionOutcome),
}

pub struct EntryReport {
    pub description: String,
    pub outcome: EntryOutcome,
    /// Mirrors `ScenarioStep::Action`. Always `false` for an assertion or for the legacy
    /// `[[actions]]` and `[[assertions]]` shape, which cannot set it.
    pub expect_error: bool,
}

impl EntryReport {
    /// A hard failure is a failed assertion, an engine rejection that `expect_error` did not
    /// ask for, or an accepted action when `expect_error` did. `NotImplemented` is never one.
    pub fn is_hard_failure(&self) -> bool {
        match (&self.outcome, self.expect_error) {
            (EntryOutcome::Assertion(AssertionOutcome::Failed { .. }), _) => true,
            (EntryOutcome::Action(StepOutcome::Errored(_)), expect_error) => !expect_error,
            (EntryOutcome::Action(StepOutcome::Ran), expect_error) => expect_error,
            _ => false,
        }
    }
}

pub struct ScenarioReport {
    pub entries: Vec<EntryReport>,
}

impl ScenarioReport {
    pub fn has_hard_failures(&self) -> bool {
        self.entries.iter().any(EntryReport::is_hard_failure)
    }
}

/// Follows each `ZoneChange` in `new_events` from the old id (in `last_known_info`) to the new
/// one, since a zone change mints a new `ObjectId` and leaves the `card_ids` entry stale.
// CR 400.7
pub(crate) fn remap_moved_card_ids(card_ids: &mut HashMap<String, ObjectId>, new_events: &[GameEvent]) {
    for event in new_events {
        if let GameEventKind::ZoneChange { object: new_id, last_known_info: Some(old), .. } =
            &event.kind
        {
            for tracked_id in card_ids.values_mut() {
                if *tracked_id == old.object_id {
                    *tracked_id = *new_id;
                }
            }
        }
    }
}

pub(crate) fn run_action_spec(
    engine: &mut Engine,
    card_ids: &mut HashMap<String, ObjectId>,
    action_spec: &ActionSpec,
    expect_error: bool,
) -> EntryReport {
    let ledger_before = engine.state().ledger.len();
    let outcome = match action_spec {
        ActionSpec::SubmitPayment { mana, .. } => run_payment(engine, build_payment_plan(mana)),
        ActionSpec::UnwindPayment { .. } => run_unwind_payment(engine),
        ActionSpec::SubmitDiscard { cards, .. } => {
            let card_obj_ids =
                cards.iter().map(|k| resolve_card(k, card_ids)).collect();
            run_discard(engine, card_obj_ids)
        }
        ActionSpec::KeepHand { player } => run_mulligan(engine, PlayerAction {
            player: PlayerId(*player),
            kind: PlayerActionKind::KeepHand,
        }),
        ActionSpec::TakeMulligan { player } => run_mulligan(engine, PlayerAction {
            player: PlayerId(*player),
            kind: PlayerActionKind::TakeMulligan,
        }),
        ActionSpec::BottomCards { player, first_n } => {
            run_bottom_cards(engine, PlayerId(*player), *first_n)
        }
        ActionSpec::SubmitScriptChoice { target, .. } => {
            run_script_choice(engine, resolve_target(target, card_ids))
        }
        ActionSpec::AdvanceToPhase { phase } => {
            let target = parse_phase(phase);
            run_advance(engine, |state| state.phase == target, &format!("phase {phase:?}"))
        }
        ActionSpec::AdvanceToStep { step } => {
            let target = parse_step(step);
            run_advance(engine, |state| state.step == target, &format!("step {step:?}"))
        }
        ActionSpec::CastSpell { player, auto_pay, .. } => {
            let outcome = run_action(engine, to_player_action(action_spec, card_ids));
            if *auto_pay { chain_auto_pay(engine, PlayerId(*player), outcome) } else { outcome }
        }
        ActionSpec::ActivateAbility { player, auto_pay, .. } => {
            let outcome = run_action(engine, to_player_action(action_spec, card_ids));
            if *auto_pay { chain_auto_pay(engine, PlayerId(*player), outcome) } else { outcome }
        }
        other => run_action(engine, to_player_action(other, card_ids)),
    };
    remap_moved_card_ids(card_ids, &engine.state().ledger[ledger_before..]);
    EntryReport { description: action_spec.describe(), outcome: EntryOutcome::Action(outcome), expect_error }
}

pub fn run(file: &ScenarioFile) -> ScenarioReport {
    let (mut engine, mut card_ids) = if let Some(game) = &file.game {
        build_new_game_engine(file, game)
    } else {
        let (state, card_ids, trigger_scripts, target_scripts, script_target_requirements, script_operations) =
            build_initial_state(file);
        let mut engine = Engine::new(state);
        for pending in trigger_scripts {
            register_trigger_ability(
                &mut engine,
                pending.source,
                pending.condition.as_deref(),
                &pending.operation,
            );
        }
        for pending in target_scripts {
            register_target_script(&mut engine, pending.object, &pending.script);
        }
        for pending in script_target_requirements {
            register_script_target_requirement(&mut engine, pending.object, &pending.source);
        }
        for pending in script_operations {
            register_script_operation(&mut engine, pending.object, &pending.source);
        }
        (engine, card_ids)
    };
    let mut entries = Vec::new();

    if !file.steps.is_empty() {
        for step in &file.steps {
            match step {
                ScenarioStep::Action { spec: action_spec, expect_error } => {
                    entries.push(run_action_spec(&mut engine, &mut card_ids, action_spec, *expect_error));
                }
                ScenarioStep::Assertion(assertion_step) => {
                    let spec = assertion_step.to_assertion_spec();
                    let outcome = check_assertion(&spec, engine.state(), &card_ids);
                    entries.push(EntryReport {
                        description: spec.describe(),
                        outcome: EntryOutcome::Assertion(outcome),
                        expect_error: false,
                    });
                }
            }
        }
    } else {
        for action_spec in &file.actions {
            entries.push(run_action_spec(&mut engine, &mut card_ids, action_spec, false));
        }
        for assertion_spec in &file.assertions {
            let outcome = check_assertion(assertion_spec, engine.state(), &card_ids);
            entries.push(EntryReport {
                description: assertion_spec.describe(),
                outcome: EntryOutcome::Assertion(outcome),
                expect_error: false,
            });
        }
    }

    ScenarioReport { entries }
}

pub fn run_path(path: &std::path::Path) -> ScenarioReport {
    run(&load(path))
}

