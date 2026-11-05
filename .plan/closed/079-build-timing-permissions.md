---
id: 79
title: Build timing permissions (flash grants)
type: task
mode: AFK
status: closed
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

Built as decided in 076. `TimingPermission` and `GameState.timing_permissions` hold "cast spells as though they had flash" effects. `can_cast_now` serves the cast command and `cast_entries`, so `legal_actions` and the command agree, and land plays and Equip keep `is_sorcery_speed`. `Game.CreateTimingPermission` registers one from Lua, and the docs and stub list it.

Departures from 076, each for a reason found while building:
- The permission tests the caster, not `card.controller`, because a commander's owner may cast it while someone else controls it.
- `remap_abilities` also drops a source's permissions, since a cast or land play moves the card without `retire_object` and a hand-functioning permission would outlive it.
- `affected` is a plain `FilterController`.
- `BEHAVIOUR_REV` went from 5 to 6, not from 4.

Vedalken Orrery in the proof deck exposed that `token_ceased` gave no Seat enough to fold a hand count, so it gained `zone` and `owner` and follows the `left_game` visibility rule. The same-seed test runs seeds 0 to 19. Details are in `docs/implementation/35-timing-permissions.md`.
