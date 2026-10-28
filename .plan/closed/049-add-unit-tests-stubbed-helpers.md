---
id: 049
title: Add unit tests for engine helpers that need a small GameState or layer stubs
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Add local `#[cfg(test)]` unit tests for the next batch of engine helpers that are deterministic but need a small `GameState` or a layer stub to exercise.

## Notes

- This follows ticket 048. Pick helpers from the ticket 047 audit that were skipped because they need a controlled state or a fake layer result.
- Good starting candidates:
  - `FilterSpec::matches` for `FilterController::You`/`Opponent` and multi-field combinations.
  - `ReplacementEffect::matches` and `ReplacementEffect::apply` with a minimal state.
  - `ContinuousEffect` query helpers (`active_for`, `sort_by_timestamp`).
  - `commands::combat` predicates that take a `GameState`.
  - `commands::legality` mode-combination and targeting helpers.
  - `script::decode` chain-word helpers that need a stub runtime.
- Keep each test deterministic and avoid `Engine` mutation or I/O.
- Do not alter production code; `comment_diff.py` should show only test-module additions.

## Resolution

Added `#[cfg(test)]` modules to three files, using minimal `GameState` fixtures and `ScriptRuntime::new()` as a stub runtime.

- `crates/engine/src/layers.rs`: helpers build `GameObject`s, `ContinuousEffect`s and a `GameState`; tests cover `effective_controller`, `effective_characteristics`, type/color/keyword/PT changes, `plus_one_counters`, and interaction ordering (add/set/double/switch PT).
- `crates/engine/src/commands/legality.rs`: helpers build creatures with PT and a one-player `GameState`; tests cover `is_sorcery_speed`, `participates_in_damage_step`, `lethal_damage_needed`, `split_targets_by_requirement`, `mode_can_join_others`, `solo_only_modes`, and `combine_chosen_modes`.
- `crates/engine/src/commands/combat/predicate.rs`: helpers build creatures and a two-player combat `GameState`; tests cover `required_attackers`, `attack_declaration_is_legal`, `pass_would_skip_a_required_attack`, `blockers_needed`, `block_declaration_is_legal`, and `blockable_attackers`.

All new tests pass (`cargo test -p stackfish-engine --lib`). `comment_diff.py` reports the expected test-module additions as code differences; the tool tests pass.

Remaining gaps from the original candidate list (`FilterSpec::matches`, `ReplacementEffect`, `ContinuousEffect` active/sort helpers, `script::decode`) are left for a follow-up ticket.
