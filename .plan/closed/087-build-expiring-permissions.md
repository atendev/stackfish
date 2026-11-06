---
id: 87
title: Build expiring timing and zone permissions
type: task
mode: AFK
status: closed
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

Built as 084 decided. Both permission structs carry `duration`, and `source_bound()` keeps `retire_object` and `remap_abilities` purging only static rows. `expire_permissions` in `engine/turn.rs` runs at cleanup, end of combat and the turn change, where "until your next turn" compares the stored controller. `AbilityEffect::GrantTimingPermission` and `GrantZonePermission` carry a boxed row, boxed so `ActivatePlan` stays under `clippy::large_enum_variant`. Their resolvers in `direct_effects.rs` stamp the resolving object and controller. `Game.Ops.GrantTimingPermission(builder, duration)` and `GrantZonePermission` share each builder's `build` with `:Register` and refuse every duration but the three drained ones (CR 611.2b).

Deviations: a Timed linked grant is accepted rather than refused. It works from a permanent that stays, and it matches nothing from a spell, which is a TODO (CR 607.2a). The seed 9 replay pin did not move, so `BEHAVIOUR_REV` stays 8 (084 assumed it was 7). There is no scenario fixture: `tests/permission_expiry.rs` resolves real spells and abilities. Notes are in `docs/implementation/37-expiring-permissions.md`.
