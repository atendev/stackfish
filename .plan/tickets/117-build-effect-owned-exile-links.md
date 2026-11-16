---
id: 117
title: Build effect-owned exile links and a library-top ExileLinked
type: task
mode: AFK
status: open
blocked_by: [92]
claimed: true
---

## Question

Build what 092 decided: `ExileLink { None, Source, Cards }` on `ZonePermission`, the `exiled_this_resolution` scratch list and its two clear points, `:SetExiledByThisEffect()`, the `Cards` prune in both purges, and `Game.Ops.ExileTopLinked(n)`.

## Notes

- Origin 092. The full decision, including the clear points, the `owners` rule for a `Cards` row, the TODOs to add and remove, and the tests, is in `closed/092-effect-owned-exile-links.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update `docs/lua/stubs/game.lua`, `player-modifiers.md`, `docs/LUA_API.md` and `docs/lua/index.md`, and keep `lua_api_doc_sync` passing.
- Act on Impulse is the scenario card. No proof-game card. Check whether the seed 9 pin moves anyway.

## Handoff

Branch `phase5/effect-owned-exile-links` (never pushed). Requirements report approved by the haiku auditor in round 2 (agent aca289d0ed5cc1dc7, may be gone, start a new auditor). No code audit yet, so the code audit round count is 0.

Done: `ExileLink { None, Source, Cards }` in `types/player.rs` with `covers`; `GameState.exiled_this_resolution`; `ScriptedZoneMoveKind::ExileLinked { source, stack_id }` and its commit push; clears in `resolve_stack_top` and `event_pipeline.rs`; grant fill in `direct_effects.rs`; `prune_linked_cards` in `commands/attach.rs`; `AbilityEffect::ExileTopLinked` and `Game.Ops.ExileTopLinked`; `:SetExiledByThisEffect()` with `build(call, allow_this_effect)`; all literals updated; prune unit test in `commands/tests.rs`; new `tests/exile_top_linked.rs` (11 tests, registered in `tests/main.rs`). The workspace builds.

Left, in order:
1. Two failing tests in `exile_top_linked.rs`: `act_on_impulse_covers_exactly_the_three_exiled_cards` (casting the sorcery after `PlayLand` hits "mid-pipeline": drain after the land play) and `a_card_a_replacement_sends_elsewhere_is_not_covered` (the land still lands in exile; check the `WouldMoveZone` filter matches a library card, or the effect filter's `card_types`).
2. Scenario fixture for Act on Impulse (inline `script =` on a `[[cards]]` entry, see `docs/scenario_format.md`). Check the seed 9 pin; `BEHAVIOUR_REV` is 10 and stays unless it moves.
3. Docs: `docs/lua/player-modifiers.md`, `docs/lua/stubs/game.lua`, `docs/LUA_API.md`, `docs/lua/index.md`, `36-zone-permissions.md`, `37-expiring-permissions.md`, `DESIGN.md`. Backlog: remove the impulse-draw lines (~34-35), narrow ~43-44, add the "until the end of your next turn" TODO (Light Up the Stage). Keep `lua_api_doc_sync` passing.
4. `tools/check.sh --full`, then a read-only sonnet audit of the diff against the report and CR (same auditor for rechecks, up to 5 rounds), then the decision report, the Merge prompt, and CI.

Read first: this ticket, `.plan/closed/092-effect-owned-exile-links.md`, `crates/engine/tests/exile_top_linked.rs`.
