---
id: 87
title: Build expiring timing and zone permissions
type: task
mode: AFK
status: open
blocked_by: [84]
claimed: false
---

## Question

Build the expiring permissions decided in 084: the `duration` field on both permission structs, the source-only purges, `expire_permissions` in `engine/turn.rs`, `AbilityEffect::GrantTimingPermission` and `GrantZonePermission`, the Lua ops and durations, the TODOs, the docs and the tests.

## Notes

- Origin 084. The full decision is in `closed/084-expiring-permissions.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update the game stub and the docs 084 lists, and keep `lua_api_doc_sync` passing.
- The 083, 084 and 085 builds all touch `ZonePermission`. Rebase on whichever lands first.

## Resolution

<Filled in at close: the decision and the reason for it.>
