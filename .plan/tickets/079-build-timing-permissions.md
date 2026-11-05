---
id: 79
title: Build timing permissions (flash grants)
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Add `TimingPermission`, `GameState.timing_permissions`, `can_cast_now`, `Game.CreateTimingPermission`, the `retire_object` purge, the TODO rewrites, the docs and the tests and proof card decided in 076.

## Notes

- Origin 076. Slice `phase5/timing-permissions`, CLAUDE.md implementation loop. The full decision is in `closed/076-timing-grants.md`.
- The Lua API changes, so update the game stub and the docs it lists, and keep `lua_api_doc_sync` passing.
- Vedalken Orrery enters the proof deck, so the replay pin and `BEHAVIOUR_REV` move in their own commit.

## Resolution

<Filled in at close: the decision and the reason for it.>
