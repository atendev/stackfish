---
id: 116
title: Build divided damage and counters
type: task
mode: AFK
status: closed
blocked_by: [91]
claimed: false
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

Built on `phase5/divided-damage-counters` and audited (requirements by haiku, APPROVE in round 3; implementation by sonnet, APPROVE in round 2). CI is green on bd99762. The decision report is `assets/116-build-divided-damage-and-counters-report.md`.

`CastSpell`, `ActivateAbility`, the madness and cascade casts and the trigger target answer carry a flat `division`, index for index with `targets` (CR 601.2d, 602.2b, 603.3d). `validate_effect_division` checks it before anything changes. A modal spell checks each divided word on its own, and `StackObject.division` keeps the split. `DividedDamage` and `DividedCounters` give each legal target its share in one event batch. An illegal target's share is lost, and the spell fizzles only when every target is illegal (CR 608.2b). `check_divided_shape` runs at every load path. It refuses a total of 0, a spec that is not one word, a word minimum above the total, and divided counters on a word that isn't an object word. `Game.Ops.DividedDamage` and `DividedCounters` are documented in the stub, `LUA_API.md`, `game-constants.md` and `targeting-and-choices.md`.

`TargetSlot.divide_total` gives the total and caps `max`. The wire adds `division` to `cast_spell`, `activate_ability`, `may_cast`, the `targets` reply and snapshot stack entries, each defaulting to empty, so `PROTOCOL_VERSION` stays 1. The bot splits at random. Arc Lightning replaces one Shock in the red-green proof deck, and the seed loop asserts a divided cast. A replay round trip keeps the division and the damage. The replay pin did not move, so `BEHAVIOUR_REV` stays 10.

The four "no division" TODOs from 091 are gone. Open TODOs: an X total, untargeted division (CR 608.2d), divided amounts in Chain and function ops, a copy's new division (CR 707.10c), `Game.Ops.CastSpell` announcing no division, `:SetTarget` and mode words taking one target, no minimum of one for "one, two or three targets", and no TOML division. `07-targeting.md` records that a redirect effect (103) must keep the announced division.
