---
id: 113
title: Bridge condition events into Lua
type: task
mode: AFK
status: open
blocked_by: [89]
claimed: false
---

## Question

Build what 089 decided. That covers `event_to_lua` for the nine new event kinds with no wildcard arm, the `CardsMilled` ledger event, the inverse counter-name function, the zero guard on `CostPayment::Life`, the seven `Game.Conditions.*` tags, the TODOs, the docs, the stub and the tests.

## Notes

- Origin 089. The full decision, including the field table and the required tests, is in `closed/089-lua-event-visibility.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update `docs/lua/stubs/game.lua` and the docs 089 lists, and keep `lua_api_doc_sync` passing.
- Adding `CardsMilled` and the zero guard may move the replay pin. Bump `BEHAVIOUR_REV` if it does.
- If "Build library top views and the shown-top event" lands first, add `LibraryTopShown` to the nil arms when the wildcard goes (094).
- The "Stun counters and tap or untap replacement hooks" ticket builds on `Tapped` and `Untapped` visibility from this one.

## Resolution

<Filled in at close.>
