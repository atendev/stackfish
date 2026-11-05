---
id: 82
title: Build zone permissions (play or cast from another zone)
type: task
mode: AFK
status: open
blocked_by: [80]
claimed: false
---

## Question

Build the zone permissions decided in 080: `ZonePermission`, `ZonePermissionKind`, `has_zone_permission`, `GameState.zone_permissions`, `can_cast_from_zone`, `can_play_land_from_zone`, `Game.CreateZonePermission`, the purges in `retire_object` and `remap_abilities`, the TODO changes, the docs, the tests and the proof card.

## Notes

- Origin 080. The full decision is in `closed/080-zone-permissions.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update the game stub and the docs 080 lists, and keep `lua_api_doc_sync` passing.
- `apply_play_land` and `from_owner` in `cast.rs` change as well as the predicates.
- Crucible of Worlds enters a proof deck in place of one Grizzly Bear. Confirm the deck slot at build time. The replay pin and `BEHAVIOUR_REV` (6 to 7) move in their own commit.
- Delete the TODOs at `entries.rs:5` and in the `can_cast_now` doc block. Write the four TODOs 080 lists.
- If no proof seed plays a graveyard land, add a scenario fixture and do not weaken the proof assertion.

## Resolution

<Filled in at close: the decision and the reason for it.>
