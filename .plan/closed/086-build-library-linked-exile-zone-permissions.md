---
id: 86
title: Build library-top and linked-exile zone permissions
type: task
mode: AFK
status: closed
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

Built on `phase5/library-top-linked-exile-build` and merged into master. `ZonePermission` gained `library_top`, `linked` and `reveals_top`. `GameState.exiled_with` records links and loses them when the exiled card or the source changes zones (CR 400.7, 607.2a). The top card is found at every check (CR 401.5), and land play from the top still uses the land-drop limit (CR 305.2a). Future Sight joins the blue-black proof deck, and `BEHAVIOUR_REV` is 8.

The build differs from 083 in these places, each for the reason given:
- `is_top_of_library` and `library_top_visible_to` live in `types/player.rs`, because the host needs them and `commands::legality` is private. `library_top_visible_to` takes `Option<PlayerId>`, so a spectator sees only a revealed top.
- There is no exile builder, so the bare op `Game.Ops.ExileLinked` records a link in place of an exile-builder `SetLinked()`.
- Host visibility is computed from the permissions at check time, not through `VisibilityPolicy`.
- `remap_abilities` also purges `exiled_with` by exiled card, because cast and land moves go through `remap_abilities` only.
- A revealed top appears in snapshots and decisions but no event announces it, so the seat fold test checks libraries by count. A reveal event for the top card is not built.
