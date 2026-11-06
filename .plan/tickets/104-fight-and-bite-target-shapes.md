---
id: 104
title: Fight and bite target shapes
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Which target shapes and damage rules do `Game.Ops.Fight` and `Bite` support beyond a two-entry requirement, a `count = 2` entry and a single `count = 1` entry?

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (Fight and bite).
- Gaps to rule on:
  - "Fights up to one target creature" (`up_to` and `at_least` entries) and "fights each creature".
  - Damage equal to something other than power.
  - Last known information when the source left the battlefield (CR 608.2h).
  - Fight or bite as one mode of a multi-mode spell, which depends on ticket 090.
  - A two-word fight on a triggered or activated ability has no test.
  - A scenario TOML `target_requirement` cannot declare two words, which ticket 095 may change.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
