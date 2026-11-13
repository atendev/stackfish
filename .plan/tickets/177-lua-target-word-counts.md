---
id: 177
title: Let Lua target words take a count
type: task
mode: AFK
status: open
blocked_by: [116]
claimed: false
---

## Question

Lua `:SetTarget` and mode target words always make a one-target word, so a scripted divided ability (Arc Lightning style) gets one target. Let a script ask for a count.

## Notes

- Found by 116. Its TODO marks the spot. See `closed/116-build-divided-damage-and-counters.md`.
- This is a Lua API change, so update `docs/lua/stubs/game.lua` and `targeting-and-choices.md`.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
