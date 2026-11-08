---
id: 134
title: Build enter counters and keyword grants
type: task
mode: HITL
status: open
blocked_by: [133]
claimed: false
---

## Question

Build the `counters` and `keywords` options on the reanimation ops, as "Reanimation options" decided.

## Notes

- Read `closed/100-reanimation-options.md`.
- `counters = { kind, amount }`, one kind and a fixed amount, sent as `CounterAdded` so doubling replacements apply (CR 122.6). Keep `// TODO: computed counter amounts (CR 122.6)`.
- `keywords = {...}` grants through a continuous effect created at entry, with an optional duration of `"permanent"` or `"end_of_turn"`.
- Update the game stub in `docs/lua/stubs/game.lua`, `docs/lua/game-constants.md` and `docs/lua/not-available-yet.md`.

## Resolution

<Filled in at close.>
