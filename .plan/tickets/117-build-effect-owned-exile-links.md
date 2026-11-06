---
id: 117
title: Build effect-owned exile links and a library-top ExileLinked
type: task
mode: AFK
status: open
blocked_by: [92]
claimed: false
---

## Question

Build what 092 decided: `ExileLink { None, Source, Cards }` on `ZonePermission`, the `exiled_this_resolution` scratch list and its two clear points, `:SetExiledByThisEffect()`, the `Cards` prune in both purges, and `Game.Ops.ExileTopLinked(n)`.

## Notes

- Origin 092. The full decision, including the clear points, the `owners` rule for a `Cards` row, the TODOs to add and remove, and the tests, is in `closed/092-effect-owned-exile-links.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update `docs/lua/stubs/game.lua`, `player-modifiers.md`, `docs/LUA_API.md` and `docs/lua/index.md`, and keep `lua_api_doc_sync` passing.
- Act on Impulse is the scenario card. No proof-game card. Check whether the seed 9 pin moves anyway.
