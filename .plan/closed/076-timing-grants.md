---
id: 076
title: Timing permissions granted by effects
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How should an effect grant permission to cast a spell or activate an ability at a time it otherwise could not ("as though it had flash", CR 601.3b-d), so that the cast command and `Engine::legal_actions` read one rule?

## Notes

- Origin 031. That ticket closed the other `legal_actions` gaps and moved this one out because it needs a new rules primitive and Lua API, not only an entry.
- Today only the card's own `Flash` keyword counts: `commands/cast.rs` (TODO near the timing check) and `commands/entries.rs` (`cast_entries`).
- Options to weigh: a permission effect on `state.continuous_effects` ("you may cast X as though it had flash"), or granting `FLASH` through the layer system to cards outside the battlefield. CR 601.3 treats the permission as separate from the keyword.
- Zone permissions ("play lands from your graveyard", CR 601.3) share the shape. Decide whether they are in scope.
- The TODO at `commands/legality.rs:30` cites CR 113.6, which is about where abilities function. Fix the citation when this is built.
- A Lua builder means updating the game stub in the docs (`docs/lua/`).
- Use the `grilling` and `domain-modeling` skills.

## Resolution

An effect that lets a player cast spells as though they had flash becomes a Timing permission: a new list, `GameState.timing_permissions`, beside `land_play_modifiers` and `cost_modifiers`, with a player filter and a spell filter. FLASH is not granted through the layers, because CR 601.3b makes it a permission and not a keyword, and layers do not filter by zone, so the bit would reach every keyword reader. One predicate, `can_cast_now` in `commands/legality.rs`, is read by the cast command and `cast_entries`, so `legal_actions` and the command agree. Land plays and Equip keep `is_sorcery_speed`, and a permission is removed when its source leaves, through `retire_object`. Zone permissions, end-of-turn grants, conditional flash and instant-speed equip are follow-ups. Build 079 does the rest, and the audit found that `cost_modifiers` outlive their source, which is 081.

The full answer set is below.

- Representation: a `TimingPermission { source, controller, affected, spells }` struct. The new field is also wired into `engine/loop_watch.rs` (destructure and fingerprint), `testutil/src/atomic.rs` (`snapshot_diff` list) and every `GameState` literal. The spell filter defaults to every spell.
- Predicate: `pub(in crate::commands) fn can_cast_now(state, player, card)` uses `card.controller` for a card in hand. It replaces the timing block at `cast.rs:132-149` and `entries.rs:96-107`. The rejection messages do not change: "actor does not hold priority" stays limited to an instant or flash card, and everything else keeps the sorcery-speed message.
- Scope: `FilterSpec::matches` returns false when `extra` is set, so a scripted filter with `extra` never matches, and the docs say so. Zone permissions go to 080, since they touch `required_zone`, `validate_play_land` and the entry scans.
- Lua: `Game.CreateTimingPermission(c)` returns a builder with `SetFunctionsFrom`, `SetAffected` (required), `SetFilter` (optional) and `Register`. Wiring is `script/globals.rs`, `script/types.rs` (`pending_timing_permissions`) and the drain in `script/runtime.rs`. Docs are `docs/LUA_API.md` (`lua_api_doc_sync` fails without it), `docs/lua/stubs/game.lua`, `docs/lua/player-modifiers.md` and `docs/lua/index.md:13`.
- TODOs: the three timing TODOs are rewritten with CR 601.3b, 601.3d, 117.1a and 702.6a, and `legality.rs:30` moves out of its doc block. CR 113.6 is cited in the builder doc only.
- Tests: `tests/timing_permissions.rs`, `can_cast_now` unit tests and a `script_timing_permission` test. Vedalken Orrery enters BLUE_BLACK in place of one `wind_drake`, and the same commit moves the replay pin and `BEHAVIOUR_REV` (currently 4).

