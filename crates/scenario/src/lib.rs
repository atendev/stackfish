//! Scenario tooling: loads a declarative TOML file describing a game setup
//! and a sequence of actions, builds a real `GameState`, drives it through
//! `Engine`, and reports what happened.
//!
//! Deliberately does NOT call the engine's own inherent constructors
//! (`GameObject::new`, `PlayerState::new`, `Zone::new`, etc) to build the
//! initial state — it builds `GameState`/`GameObject`/`PlayerState`/`Zone`
//! via direct struct literals instead, since their fields are `pub`. Partly
//! bootstrapping (`GameObject::new` now takes `&GameState` itself, which
//! doesn't exist yet while this tool is still assembling the very first
//! one), partly to keep this tool independent of which inherent-method
//! bodies have been filled in yet — and means setup+assertions (no actions)
//! already runs for real today, with zero panics, as a baseline smoke test
//! of the Phase 1 data model.
//!
//! Two exceptions, implemented for real (not `todo!()`) because nothing
//! else can construct them — `CounterSet`/`Engine` both have a private
//! field, so external code can't struct-literal around them:
//!   - `CounterSet::new()` (counter.rs)
//!   - `Engine::new()` / `Engine::state()` (engine.rs)
//!
//! Running *actions* through `Engine::submit_action`/`step()` still hits
//! `todo!()` until Phase 5/7 fill those bodies in — expected. Wrapped in
//! `catch_unwind` so one stubbed action doesn't abort the whole report;
//! each action's outcome is reported individually.

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
