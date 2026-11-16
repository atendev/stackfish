---
id: 117
title: Build effect-owned exile links and a library-top ExileLinked
type: task
mode: AFK
status: closed
blocked_by: [92]
claimed: false
---

## Question

Build what 092 decided: `ExileLink { None, Source, Cards }` on `ZonePermission`, the `exiled_this_resolution` scratch list and its two clear points, `:SetExiledByThisEffect()`, the `Cards` prune in both purges, and `Game.Ops.ExileTopLinked(n)`.

## Notes

- Origin 092. The full decision, including the clear points, the `owners` rule for a `Cards` row, the TODOs to add and remove, and the tests, is in `closed/092-effect-owned-exile-links.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update `docs/lua/stubs/game.lua`, `player-modifiers.md`, `docs/LUA_API.md` and `docs/lua/index.md`, and keep `lua_api_doc_sync` passing.
- Act on Impulse is the scenario card. No proof-game card. Check whether the seed 9 pin moves anyway.

## Resolution

Built what 092 decided, on `phase5/effect-owned-exile-links`. A read-only sonnet auditor approved in round 2, and `tools/check.sh --full` passes with 2725 tests.

- Shape: `ZonePermission.linked` is `ExileLink { None, Source, Cards }`. A `Cards` row covers exactly its ids and ignores `owners`. `GameState.exiled_this_resolution` lists the cards a resolution exiled. `ScriptedZoneMoveKind::ExileLinked { source, stack_id }` pushes a card onto it only when the card landed in exile, so a card a replacement redirects is not covered.
- Clears: at the start of a resolution, and at the pipeline commit of `StackObjectResolved` (including the modal resume path) and `Fizzled`, so the list is empty whenever a player has priority.
- Grant: `resolve/direct_effects.rs` copies the list into a `Cards` row. `prune_linked_cards` in `commands/attach.rs` drops a card id from every `Cards` row in both purges, so a card that is played, cast or leaves exile stops being covered (CR 400.7).
- Lua: `Game.Ops.ExileTopLinked(n)` and `:SetExiledByThisEffect()`. `ZonePermissionBuilder::build(call, allow_this_effect)` refuses the setter under `:Register()`, with `:SetLinked()`, and for a zone other than `"Exile"`. `ExileTopLinked` inside `ForEachPlayer` is a load error through the existing generic message, with a TODO and a test.
- Tests: `exile_top_linked.rs` (11), a prune unit test in `commands/tests.rs`, and `scenarios/permissions/act_on_impulse.toml`. No proof-game card, and the seed 9 pin did not move, so `BEHAVIOUR_REV` stays 10. `PROTOCOL_VERSION` is unchanged.
- Docs: the Lua stub, `LUA_API.md`, `lua/index.md`, `player-modifiers.md`, `game-constants.md`, notes 36 and 37, `DESIGN.md` and the backlog.
- Open: "until the end of your next turn" has no `EffectDuration` (Light Up the Stage), `ExileTopLinked` has no per-player form, and the cards are exiled face up.
