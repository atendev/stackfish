---
id: 116
title: Build divided damage and counters
type: task
mode: AFK
status: open
blocked_by: [91]
claimed: true
---

## Question

Build what 091 decided: `division` on `CastSpell`, `ActivateAbility` and the trigger-target answer, `StackObject.division`, `validate_division`, `DividedDamage` and `DividedCounters` with per-share resolution, `TargetSlot::divide_total`, the Lua ops, the wire field, the bot and a proof card.

## Notes

- Origin 091. The full decision, including the validation rules, load errors, TODOs to keep and tests, is in `closed/091-divide-among-targets.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update `docs/lua/stubs/game.lua`, `game-constants.md` and `targeting-and-choices.md`, and keep `lua_api_doc_sync` passing.
- Resolve the four TODOs named in 091 (`cast.rs`, `legality.rs`, `resolve/mod.rs`, `multi_target_resolution.rs`). Keep the new TODOs 091 lists.
- Check whether the replay pin moves and bump `BEHAVIOUR_REV` if it does.
- Ticket 115 also adds a field to `ActivateAbility`. Rebase on whichever lands first.

## Resolution

<Filled in at close.>
