//! Loads a TOML scenario, builds a `GameState` from struct literals, drives it through the
//! `Engine` and reports each step. A `todo!()` panic becomes a per-step outcome.
//! See `docs/scenario_format.md`.

mod actions;
mod assertions;
mod auto_pay;
mod build;
mod build_new_game;
mod card_templates;
mod db_tests;
mod inline_setup;
mod load;
mod report;
mod run;
mod schema;

pub use assertions::AssertionOutcome;
pub use build::build_initial_state;
pub use db_tests::{run_db_tests, DbTestOutcome, DbTestResult};
pub use load::{load, parse};
pub use report::{run, run_path, EntryOutcome, EntryReport, ScenarioReport};
pub use run::StepOutcome;
pub use schema::*;
