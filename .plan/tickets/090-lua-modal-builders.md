---
id: 90
title: Modal builders in Lua for spells, triggers and activated abilities
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How does a card script declare modes? Modal spells ("choose one") and modal triggered abilities (CR 603.3c) have no Lua builder, and a modal activated ability cannot exist at all because `RegisteredAbility` has no `modes` field (CR 700.2b).

## Notes

- Origin: backlog item 7, `docs/lua/not-available-yet.md` (triggered abilities and modal spells entries), the TODOs at `types/card.rs:255` and `commands/activate.rs:507`.
- Decide the builder shape, how a mode's target requirement and operation are declared, and whether `RegisteredAbility` gains a `modes` field or activated abilities reuse the spell path.
- The engine already supports mode choice for cards built in Rust, including `solo_only` modes (ticket 036) and opponent-chosen modes. Read `docs/implementation/24-mode-combination.md` and `12-coroutines.md` first.
- Lua API changes mean the game stub in `docs/lua/stubs/game.lua` must be updated by the build.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
