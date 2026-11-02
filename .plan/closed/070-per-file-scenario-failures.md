---
id: 070
title: Report scenario failures per file instead of stopping at the first
type: task
mode: HITL
status: closed
blocked_by: [060]
claimed: false
---

## Question

Change `crates/engine/tests/scenarios.rs` so a run reports every failing scenario file and which assertion failed in each, instead of panicking on the first file with only its path.

## Notes

- Optional. Found by 060. Only worth doing if 071 goes ahead or the scenario count grows.
- Options include collecting all reports before one assert, or one generated `#[test]` per file.

## Resolution

The scenario test collects every file's result and fails once at the end, not one generated `#[test]` per file. Per-file tests would need a build script or a custom harness in the consolidated `tests/main.rs` binary, and one failure message that lists every file gives the same information.

`ScenarioReport::hard_failure_lines` returns one line per hard failure: the entry index, its description, and the reason (`expected X, got Y`, `errored: <msg>`, or a note that the action ran despite `expect_error`). `crates/engine/tests/scenarios.rs` runs each file inside `catch_unwind`, so a load or setup panic is reported with its message and the run continues. Files appear sorted, with paths relative to `tests/scenarios/`. `docs/scenario_format.md` describes the output.

A manual check broke one assertion and one file's TOML. The failure message listed both files with their reasons. Built in `2b9a08a` on `phase5/per-file-scenario-failures`.
