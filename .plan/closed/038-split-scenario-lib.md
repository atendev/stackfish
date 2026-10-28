---
id: 038
title: Split crates/scenario/src/lib.rs
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

`crates/scenario/src/lib.rs` is 3,222 lines in one file. How is it split by concern with no behavior change, keeping the public API and the scenario tests unchanged?

## Notes

- Map: `map-cleanup.md`, section A. Branch `phase5/split-scenario-lib`.
- Public API that must keep its paths: `parse`, `load`, `run`, `run_path`, `run_db_tests`, `build_initial_state`, `ScenarioFile`, `ScenarioReport`, `EntryOutcome`, `AssertionOutcome`, `StepOutcome`, `DbTestResult`, `DbTestOutcome`. Re-export from `lib.rs`.
- Existing submodules: `card_templates.rs`, `inline_setup.rs`.
- Safety net: `cargo test -p stackfish-scenario`, plus `crates/engine/tests/scenarios.rs` which runs the TOML fixtures.
- Pure move. No comment edits (ticket 040 owns those). Comments travel with their code.
- The module doc in `lib.rs` is outdated (`todo!()` bodies, Phase 1). Ticket 040 rewrites it.

## Resolution

`crates/scenario/src/lib.rs` went from 3,222 lines to 46 (docs, `mod` lines, re-exports). The code moved unchanged into `schema`, `build`, `build_new_game`, `actions`, `auto_pay`, `run`, `assertions`, `report`, `load` and `db_tests`, and only shared helpers gained `pub(crate)`. A sorted-line diff showed no code changes, and the scenario and engine scenario tests pass. Merged as 3cb70a3. The outdated module doc in `lib.rs` is left for ticket 040.
