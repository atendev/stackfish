---
id: 047
title: Audit crates/engine/src/ modules for missing #[cfg(test)] unit tests
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

List pure helpers and data structures in `crates/engine/src/` that have no local unit tests and are only exercised end-to-end.

## Notes

- This is the first research ticket in the cleanup map, unblocked by 041.
- Focus on functions and types that can be tested in isolation (no Engine/Game state, deterministic output, no I/O).
- Ignore integration-test-only logic that intentionally needs full game state; the goal is to surface cheap unit-test gaps.
- `comment_diff.py` is not needed because this ticket makes no production code changes.

## Resolution

The audit covered every Rust source file under `crates/engine/src/`. A function or type counted as a unit-test candidate when it is deterministic, needs no `Engine`/`GameState` mutation, needs no I/O, and needs no live Lua VM. Orchestration that only makes sense inside a running game was left to the integration tests.

Findings:
- About 200 pure helpers across 35 files lack local tests.
- The densest gaps are in `types/` (zone, object, player, counter, card, mana, cost, ability, effect, target), `commands/legality.rs`, `commands/combat/*`, `commands/attach.rs`, `script/decode.rs`, `sba.rs`, `replacement.rs`, and `triggers.rs`.
- Files that already have some tests still have gaps, especially `types/mana.rs`, `types/cost.rs`, `types/ability.rs`, `types/effect.rs`, `types/game.rs`, and `replacement.rs`.
- Constructors and data-only types in `types/turn.rs`, `types/combat.rs`, `types/command.rs`, `types/config.rs`, `types/stack.rs`, and `types/ids.rs` are trivial; they are not worth testing in isolation.

The full inventory is in the audit report. The next step is ticket 048, which adds the first batch of local unit tests for the highest-value pure helpers.
