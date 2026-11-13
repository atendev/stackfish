---
id: 113
title: Bridge condition events into Lua
type: task
mode: AFK
status: closed
blocked_by: [89]
claimed: false
---

## Question

Build what 089 decided. That covers `event_to_lua` for the nine new event kinds with no wildcard arm, the `CardsMilled` ledger event, the inverse counter-name function, the zero guard on `CostPayment::Life`, the seven `Game.Conditions.*` tags, the TODOs, the docs, the stub and the tests.

## Notes

- Origin 089. The full decision, including the field table and the required tests, is in `closed/089-lua-event-visibility.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The Lua API changes, so update `docs/lua/stubs/game.lua` and the docs 089 lists, and keep `lua_api_doc_sync` passing.
- Adding `CardsMilled` and the zero guard may move the replay pin. Bump `BEHAVIOUR_REV` if it does.
- If "Build library top views and the shown-top event" lands first, add `LibraryTopShown` to the nil arms when the wildcard goes (094).
- The "Stun counters and tap or untap replacement hooks" ticket builds on `Tapped` and `Untapped` visibility from this one.

## Resolution

Built on `phase5/condition-events` in an auto-mode run, then merged.

- `event_to_lua` bridges `zone_change`, `damage_dealt`, `life_changed`, `tapped`, `untapped`, `counter_added`, `player_counter_added`, `attacker_declared`, `blocker_declared` and `cards_milled`, and names every other `GameEventKind` with no wildcard. Scry and surveil stay `nil`.
- `target_to_lua` is shared with `resume_operation`. `counter_kind_name` inverts `parse_counter_kind_name`, and a custom counter gives its own name.
- `CardsMilled { player, cards }`: `ScriptedZoneMoveKind::Mill { since_ledger }` scans the ledger when its queue drains and logs one event per owner in first-move order. Cards in a hand or library count as milled but are left out of `cards`.
- `CostPayment::Life(0)` writes no `LifeChanged`.
- `Game.Conditions` gains `BecomesTapped`, `BecomesUntapped`, `GainsLife`, `LosesLife`, `Attacks`, `Blocks` and `Mills`.
- The host wire gains a public `cards_milled`. The replay pin did not move, so `BEHAVIOUR_REV` stays 9.
- TODOs: `Each*` siblings, `Blocks` once per pair (CR 509.3a), hidden-zone milled cards, mill as a cost, skipping Lua for unread event kinds.
