---
id: 88
title: Build zone permission riders
type: task
mode: AFK
status: closed
blocked_by: [85]
claimed: false
---

## Question

Build the zone permission riders decided in 085: `LeaveStackRider`, `leave_stack_rider` on `StackObject`, `ZonePermission.rider`, `cast_zone_source`, `StackObject::leave_destination` at the three exits, `SetRider` in Lua, the TODOs, the docs, the tests and the Kess scenario fixture.

## Notes

- Origin 085. The full decision is in `closed/085-zone-permission-riders.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update the game stub and the docs 085 lists, and keep `lua_api_doc_sync` passing.
- The 083, 084 and 085 builds all touch `ZonePermission`. Rebase on whichever lands first.

## Resolution

Built as 085 decided. A spell's `StackObject.leave_stack_rider` replaces the flashback bool, and `leave_destination` is its only reader at resolve, fizzle and counter. `cast_zone_source` returns the rider of the permission that allowed the cast, and `cast_permission_rider` picks a rider-less permission first, then the first in list order, since `ZonePermission` has no id. `ZonePermissionBuilder:SetRider` takes `ExileInstead` or `ExileInsteadOfGraveyard` and only on a Cast permission. The old "no rider" TODO on `can_cast_from_zone` was trimmed to the "die or leave exile this way" rider that is still missing.

Tests are in `zone_permissions.rs` (10 rider cases), `script_zone_permission.rs` and a `leave_destination` unit test. The Kess, Dissident Mage scenario casts Kess first, because a scenario's battlefield setup does not register a card's static script. The replay pin did not move, so `BEHAVIOUR_REV` stays 8. Notes are in `docs/implementation/38-zone-permission-riders.md`.
