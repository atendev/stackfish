---
id: 177
title: Let Lua target words take a count
type: task
mode: AFK
status: open
blocked_by: [116]
claimed: true
---

## Question

Lua `:SetTarget` and mode target words always make a one-target word, so a scripted divided ability (Arc Lightning style) gets one target. Let a script ask for a count.

## Notes

- Found by 116. Its TODO marks the spot. See `closed/116-build-divided-damage-and-counters.md`.
- This is a Lua API change, so update `docs/lua/stubs/game.lua` and `targeting-and-choices.md`.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

Built on `phase5/lua-target-word-counts` and audited (requirements by haiku, APPROVE in round 2; implementation by sonnet, APPROVE in round 1). CI is green on a46f11d (run 43452913946). PR #6. The decision report is `assets/177-lua-target-word-counts-report.md`.

A `:SetTarget` table takes `count`, `up_to`, `at_least` and `exclude_source`, the keys `s.target_requirement()` uses, with `count` defaulting to 1 (CR 601.2c). A bare function word is still exactly one target. Mode `target` tables already took a count through the `target_requirement()` decoder, with `count` required, so they did not change. A test pins a two-target divided mode word. Lua activated and triggered abilities now split divided damage and counters among several targets (CR 601.2d, 602.2b, 603.3d).

`decode_requirement_keys` is the one count-key decoder. It refuses `up_to` with `at_least`, `count = 0` without `at_least`, and any count key on a `"triggering"` word, for every caller. The stub's `SetTargetTable` class and the targeting, ability, `LUA_API.md` and `game-constants.md` docs describe the keys. The "`:SetTarget` and mode words take one target" TODO is gone. The other divided TODOs from 116 stay open. No new tickets.
