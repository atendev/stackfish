---
id: 062
title: Strict key, type and counter-name validation in Lua decoders
type: build
mode: HITL
status: open
blocked_by: [061]
claimed: false
---

## Question

Apply section 4 of [052](file:///home/aten/Documents/Github/stackfish/.plan/closed/052-decide-lua-api-canonical-forms.md): unknown keys, wrong-typed values and unknown counter strings raise Lua errors at load time and game time.

## Notes

- Map: `.plan/map-cleanup.md`, track C. Third of four slices. Runs after 061 so the allowed key sets are final.
- Scope:
  - A shared `reject_unknown_keys` helper across the 9 dedicated decoders, the inline `Search`, `ChooseN` and `CastSpell` option tables, and the game-time callback readers.
  - Replace `.ok().flatten()` and `if let Ok(Some(..))` in `decode_creature_token_opts`, `decode_landwalk_quality` and `decode_copy_exceptions`.
  - Error on conflicting keys in `decode_affected_value` / `Game.Filter`.
  - `parse_counter_kind_name` errors on unknown strings. Custom counters use an explicit constructor.
- `layers.rs` (`resolve_primitive`) calls `invoke_continuous_effect(...).ok().flatten()`, so a bad return value from a continuous-effect function is dropped at game time, not raised. Decide in this slice how a script error surfaces there (found in 053).
- Tests: one rejection test per decoder family, a game-time typo test, a counter-name test and a conflicting-keys test.
- Same `CLAUDE.md` slice workflow as 053.

## Resolution

<Filled in at close.>
