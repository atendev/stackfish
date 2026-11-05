---
id: 82
title: Build zone permissions (play or cast from another zone)
type: task
mode: AFK
status: closed
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

Zone permissions are built as 080 decided. `ZonePermission` lives in `GameState.zone_permissions`. `can_cast_from_zone` and `can_play_land_from_zone` decide the zone for the cast command, `validate_play_land`, `apply_play_land` and both entry scans, so `legal_actions` and the commands agree. A cast or land play removes the card from its real zone (the owner's, or the shared exile), reports that zone, and gives control to the actor (CR 601.2a, 110.2), which 080 did not state. `Game.CreateZonePermission` takes `SetZone` and `SetKind` as strings, like `SetFunctionsFrom`. `retire_object` and `remap_abilities` purge permissions with their source. Crucible of Worlds replaced one Grizzly Bear in the red-green deck. No proof seed from 0 to 29 plays a graveyard land, so `scenarios/permissions/crucible_graveyard_land.toml` covers the play. `BEHAVIOUR_REV` is 7 with the seed 9 pin moved in its own commit. The four TODOs from 080 are written and the two stale ones are gone. Notes are in `docs/implementation/36-zone-permissions.md`.
