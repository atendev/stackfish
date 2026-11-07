---
id: 121
title: Move the targeting and layer tests to TOML
type: task
mode: AFK
status: open
blocked_by: [120]
claimed: false
---

## Question

Move the 27 tests 095 listed from `targeting_basics.rs`, `targeting_up_to_n.rs`, `targeting_at_least_n.rs` and `layer_engine_basics.rs` into scenarios, and delete each Rust test once its scenario asserts the same thing.

## Notes

- Origin 095. The per-test list is in `closed/095-scenario-schema-additions.md`. `cda_power_and_toughness_are_still_skipped_not_guessed` stays in Rust.
- A TOML version must not be weaker than the Rust test it replaces. Compare each pair in the build report.
- Fizzle scenarios use a response instant with `s.target_requirement()` and `Game.Ops.Destroy`. One must pass before its Rust test is deleted.
- Resolution effects use a one-line inline `s.operation()` script. No moved layer test mixes a Lua static with a TOML `continuous_effect`.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch, and run a manual break check as 071 did.

## Resolution

<Filled in at close.>
