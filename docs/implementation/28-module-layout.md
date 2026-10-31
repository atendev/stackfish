# 28 — Module layout after cleanup

The cleanup map split the largest engine files into submodules by subsystem so that each file has a
single owner and comment passes touch files in their final shape.

- `crates/engine/src/types.rs` became `crates/engine/src/types/` with one module per domain:
  `ability`, `card`, `combat`, `command`, `config`, `cost`, `counter`, `effect`, `event`, `game`,
  `ids`, `mana`, `object`, `player`, `stack`, `target`, `turn`, `zone`. Ticket 039 kept them in one
  re-exporting `mod.rs` rather than further splitting.
- `crates/engine/src/engine/mod.rs` (9,593 lines) became the `crates/engine/src/engine/` directory:
  `mod.rs` holds the `Engine` struct and dispatcher; `process_state.rs`, `turn.rs`, `action.rs`,
  `event_pipeline.rs`, `triggers_pipeline.rs`, `replacement_resume.rs`, `payment.rs`, `pregame.rs`,
  `library_choices.rs`, `choice_answers.rs`, and `resolve/` each own one slice of the pipeline.
- `crates/engine/src/script/globals.rs` still builds the `Game.*` Lua DSL in one registration pass;
  helpers live in `script/builders.rs`, `script/decode.rs`, `script/tags.rs`, `script/types.rs`,
  `script/encode.rs`, and `script/runtime.rs`.
- `crates/scenario/src/lib.rs` (1,924 lines) became `lib.rs` (exports), `build.rs` and
  `build_new_game.rs` (board construction), `run.rs` (scripted action execution), `actions.rs`
  (shortcut helpers), `assertions.rs` (`verify_expectations`), `load.rs` (TOML parsing) and
  `schema.rs` (scenario types).

## Visibility

- `EngineCommand`, `CommandOutcome`, `ProcessState` and `commands::execute_command` are `pub(crate)`. Hosts reach the engine through `Engine::submit_action`, `step`, `open_decision` and `answer`.
- `engine_context` and `script_api` are private modules. `commands` stays `pub` for `Rejected`, `Internal`, `AttachProposal` and `propose_attach`.
- An engine item is `pub` only when another crate or an integration test names it. Fields of public or serde-shaped types stay `pub`.
- The scenario schema types in `crates/scenario/src/schema.rs` stay re-exported `pub` because they are the documented TOML format.
- The workspace lint `unreachable_pub` covers `host`, `godot`, `carddb` and `scenario`. The engine is exempt because its modules are public.

See the closed tickets in `.plan/closed/` for the original rationale.
