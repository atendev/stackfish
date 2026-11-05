---
id: 88
title: Build zone permission riders
type: task
mode: AFK
status: open
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

<Filled in at close: the decision and the reason for it.>
