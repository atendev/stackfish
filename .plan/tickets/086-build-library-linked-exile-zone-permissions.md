---
id: 86
title: Build library-top and linked-exile zone permissions
type: task
mode: AFK
status: open
blocked_by: [83]
claimed: false
---

## Question

Build the library-top and linked-exile zone permissions decided in 083: the three `ZonePermission` flags, `is_top_of_library`, `GameState.exiled_with` and its purges, the entries and Seat visibility for a library card, the Lua setters, the TODOs, the docs, the tests, Future Sight in the blue-black proof deck and the `BEHAVIOUR_REV` bump.

## Notes

- Origin 083. The full decision is in `closed/083-library-linked-exile-zone-permissions.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update the game stub and the docs 083 lists, and keep `lua_api_doc_sync` passing.
- The 083, 084 and 085 builds all touch `ZonePermission`. Rebase on whichever lands first.

## Resolution

<Filled in at close: the decision and the reason for it.>
