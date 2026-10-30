---
id: 055
title: Audit pub vs pub(crate) visibility and dead helpers across crates
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Which `pub` items in `crates/` are used only inside their own crate (candidates for `pub(crate)` or private), and which functions, methods, constants and types are never referenced anywhere (dead helpers)?

## Notes

- Map: `.plan/map-cleanup.md`, track D. This feeds ticket 057 (tighten visibility, remove dead helpers, apply workspace lints), together with 056.
- Crates: `crates/engine`, `crates/carddb`, `crates/scenario`, `crates/testutil`, `crates/host`, `crates/godot` (and any others in the workspace).
- Cross-crate use counts: a `pub` item referenced from another crate's `src/` or `tests/`, or from `crates/*/tests/`, is legitimately public. Items used only by their own crate's integration tests under `tests/` also count as external use, since those compile as separate crates.
- Deliver an inventory, not changes: no edits to `crates/`.

## Resolution

Read-only audit of all seven crates. Method: `RUSTFLAGS="-W unreachable_pub -W dead_code -W unused" CARGO_TARGET_DIR=/tmp/sf-audit-target cargo check --workspace --all-targets` on the repo, then a scratch copy under `/tmp` where one crate at a time had every `pub` rewritten to `pub(crate)` and each privacy error put its name back to `pub`, until the build was clean. Items still narrowed are unused by any other crate's `src/`, any `crates/*/tests/`, testutil, cli and godot. The proof is name-granular, so the narrowable counts are a lower bound. `dead_code` fires nothing on the raw repo because rustc treats `pub` as a root; dead items came from the scratch copy.

**Counts (narrowable items + fields / dead / test-only).**
- engine: 85 (55 + 30) / 6 / 5. The 6 dead is the five items below plus the `invoke_cost` stub; the empty `script_api` module is counted only as narrowable. All other engine `pub` items are named by another crate, mostly the integration tests, testutil, host and scenario.
- carddb: 7 / 0 / 0. scenario: 137 (23 + 114, mostly serde schema types) / 0 / 0. host: 187 (73 + 114) / 3 / 1. The host module lists below sum to 184; the other 3 sit in host files the audit did not itemise. testutil: 1. godot: 7. cli: 0 (bin crate).

**Dead, safe to delete.**
- `engine/src/decision/mod.rs:155` `RejectionCode::as_str`.
- `engine/src/types/card.rs:184` `enum Keyword` (its doc says to match on it; nothing does).
- `engine/src/engine/process_state.rs:121` `StepResult.events_added` (written at `engine/mod.rs:628,631`, never read; `docs/implementation/00-overview.md:80` documents it).
- `engine/src/replacement.rs:37` `NeedsOrderingChoice.is_destroy` and `engine/src/script/types.rs:41` `ScriptContext.chosen_targets` (written at `triggers.rs:102`, never read). No TODO names either, so confirm they are not reserved before deleting.
- `host/src/bot.rs:168` `Bot::with_starting_life`, `:196` `keeping_transcript`, `:203` `transcript`. Only docs mention them.

**Dead but TODO-backed, keep.** `engine/src/script/runtime.rs:549` `invoke_cost` (`todo!()` stub, `docs/implementation/05-script-bridge.md:170`) and the empty `pub mod script_api` at `engine/src/lib.rs:10`. `script_api` holds only a TODO, so it is not a dead item; keep the file and narrow the module to `mod`.

**Test-only (dead in the normal build; 5 in engine, 1 in host, 6 total).** `Engine::fatal_error` (`engine/mod.rs:449`), `ProcessState::AwaitingAttachChoice` (`process_state.rs:331`; the only non-test construction is the self-restore inside its own match arm at `engine/mod.rs:720`, so production never enters it. The `step()` arm at `engine/mod.rs:717` also needs gating or removal, so `#[cfg(test)]` on the variant alone will not compile), `is_permanent` (`types/object.rs:157`), `top` (`types/zone.rs:54`), `Session::internal_error` (`host/src/session.rs:189`). Gate with `#[cfg(test)]` or delete with their tests. `VisibilityPolicy::is_visible_to` (`types/object.rs:20`) is TODO-backed, keep. Items used only by integration tests or testutil must stay `pub`: `commander_color_identity`, `illegal_cards_for_color_identity`, `pipeline_context_active`, `is_hand_revealed`, `propose_attach`, `load_function`, `Bot::recording`, `injecting_illegal_answers`, `CardDb::load_by_name`, `active_tests`.

**Narrowable, by crate.**
- engine, highest confidence: `execute_command`, `EngineCommand`, `CommandOutcome`, `CommandError`, `PriorityHandoff`, `PendingPaymentRequest`, `PaymentResolution` (commands/mod.rs); `ProcessState` and the resume and zone-move enums (engine/process_state.rs, 13); `PipelineContext`, `PipelineExit`; `apply_replacements`; `apply_cost_modifiers`; the `types/mana.rs` cost-delta helpers (7); the script runtime helpers `handle_count`, `invoke_condition`, `invoke_target_legal`, `invoke_player_target_legal`, `resume_operation`, `resume_operation_bool`; `mod engine_context` and `mod script_api` in `lib.rs`. `EngineCommand`, `CommandOutcome` and `ProcessState` appear in public signatures, so narrowing them raises `private_interfaces` warnings. Decide first whether they should be nameable.
- carddb: `keywords::parse`, `script::vanilla_stub`, `types_line::parse`, `ParsedTypeLine`.
- scenario: `schema.rs` (136, mostly serde types), `report::is_hard_failure`.
- host: `wire/*` 119, `setup.rs` 29, `envelope.rs` 22, `view.rs` 9, `bot.rs` 4, `session.rs` 1. godot: `core.rs` 7. testutil: `atomic::assert_unchanged`.

**Could not verify.** Enum variants and trait methods cannot be narrowed. `pub use` re-exports were not checked beyond the names they list. Lua and GDScript consumers of the Rust API (godot smoke) were not examined. Same-named items (`new`, `id`, two `build_alternative_costs`, two `build_buyback_cost`) stay `pub` in the compile proof.

**Feeds 057.** Decide whether engine internals (`commands`, `engine::process_state`) should stay nameable by integration tests before narrowing. Start with the dead list, then the compile-proven narrowings, one crate per commit. One guard-held shell command in the audit was not retried.
