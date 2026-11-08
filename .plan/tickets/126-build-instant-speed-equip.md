---
id: 126
title: Build instant-speed equip
type: task
mode: HITL
status: open
blocked_by: [125]
claimed: false
---

## Question

Build "you may activate equip abilities any time you could cast an instant" as "Timing permission follow-ups" decided.

## Notes

- Read `.plan/closed/098-timing-permission-follow-ups.md` for the decisions.
- `TimingPermission.spells` becomes `TimingSubject::{Spells(FilterSpec), EquipAbilities}` in the same list. `TimingPermissionBuilder` gains a subject setter.
- `activate.rs:129` and `entries.rs:304` read one shared helper. Update the `AbilityEffect::Attach` comment at `types/ability.rs:480` if it no longer holds.
- Remove the TODO at `legality.rs:30` and update the `legality.rs:41` doc.
- Test with Leonin Shikari (unconditional) and Forge Anew (`DuringYourTurn`). CR 702.6a, 602.5d.
- Lua API change: update `docs/LUA_API.md`, `docs/lua/player-modifiers.md` and the stub in `docs/lua/stubs/game.lua`.

## Resolution

<Filled in at close.>
