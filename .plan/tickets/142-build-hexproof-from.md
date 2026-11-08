---
id: 142
title: Build hexproof from
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Add "hexproof from [quality]" and reject `IgnoreKeyword("Hexproof")` and `IgnoreKeyword("Shroud")` at load, as "Ward with non-mana costs, player hexproof and hexproof from" decided.

## Notes

- Read `closed/101-ward-costs-and-player-hexproof.md`.
- A layer-6 `AddHexproofFrom(quality)` list with color, card type, monocolored and multicolored. It never sets `KeywordSet::HEXPROOF`.
- `target_blocked_by_hexproof_or_shroud` takes `source`. The quality reads the source's effective characteristics (CR 702.11d).
- "Each color" expands at decode (CR 702.11g). `RemoveKeyword(HEXPROOF)` and `RemoveAllAbilities` clear the list (CR 702.11e).
- TODOs: "that color", "activated and triggered abilities" (CR 702.11d), the CR 702.11e keyword read, and the player-relative "as though" permission that replaces the `ignored_keywords` TODO.
- Update the game stub in `docs/lua/stubs/game.lua`, and `docs/lua/not-available-yet.md`.

## Resolution

<Filled in at close.>
