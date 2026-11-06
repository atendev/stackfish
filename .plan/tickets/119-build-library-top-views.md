---
id: 119
title: Build library top views and the shown-top event
type: task
mode: AFK
status: open
blocked_by: [94]
claimed: false
---

## Question

Build what 094 decided: the `LibraryTopView` rows with `reveals_top` removed from `ZonePermission`, the stored shown top and its sync (frozen while payment is pending), the `LibraryTopShown` ledger event, its Seat filter in `view.rs`, and the Lua builder `Game.CreateLibraryTopView` with `Game.Ops.GrantLibraryTopView`.

## Notes

- Origin 094. The full decision, including the sync points, the audience merge, the Seat filter and the tests, is in `closed/094-library-top-reveal.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Write the CR 401.5 payment-freeze test first and watch it fail.
- Grep scripts, fixtures and docs for a play-from-top card that relied on the implicit look.
- The Lua API changes, so update `docs/lua/stubs/game.lua`, `docs/LUA_API.md` and `docs/lua/player-modifiers.md`, and keep `lua_api_doc_sync` passing.
- Bump `BEHAVIOUR_REV` to 9 if the replay hash moves, and re-pin in its own commit.

## Resolution

<Filled in at close.>
