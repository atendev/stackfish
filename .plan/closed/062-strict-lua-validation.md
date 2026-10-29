---
id: 062
title: Strict key, type and counter-name validation in Lua decoders
type: build
mode: HITL
status: closed
blocked_by: []
claimed: true
---

## Question

Apply section 4 of [052](file:///home/aten/Documents/Github/stackfish/.plan/closed/052-decide-lua-api-canonical-forms.md): unknown keys, wrong-typed values and unknown counter strings raise Lua errors at load time and game time.

## Resolution

Implemented strict validation across the Lua API surface.

- Added `reject_unknown_keys` in `crates/engine/src/script/decode.rs`, used by all dedicated table decoders (`decode_alternative_cost_table`, `decode_object_filter_keys`, `decode_creature_token_opts`, `decode_landwalk_quality`, `decode_copy_exceptions`, `decode_script_choice_request`, `decode_target_pick`) and by the inline option tables in `crates/engine/src/script/globals.rs` (`Game.Ops.Search`, `Game.Ops.CastSpell`, `Game.ChooseN`).
- `decode_object_filter_keys` now takes `extra_allowed` so target requirement count keys (`count`, `up_to`, `at_least`, `type`, `exclude_source`) and scripted filter function keys don't trigger false rejections, while every flat filter key is still validated.
- `Game.Filter({...})` explicitly rejects `self_only`, `enchanted_only` and `enchanted_player_controls` with a message pointing to `Game.Affected.*`.
- `parse_counter_kind_name` now returns `Result` and rejects unknown strings; added `Game.Counter.*` namespace with common tags plus `Game.Counter.Custom(name)` for flavor counters. All counter-consuming sites (`Game.Ops.Counters`, `Game.Ops.ReturnSelfWithCounter`, `Game.Ops.AddCountersTo`, `Game.Conditions.HadNoCounters`, `Game.ReplacementPayloads.EntersWithCounters`, `EntersWithCountersBloodthirst`, `EntersWithCountersEscape`) use `decode_counter_arg` and accept either a known string or a `CounterTag`.
- `layers.rs::resolve_primitive` now matches on `invoke_continuous_effect` and logs the error to stderr instead of silently dropping it; the game-state signature only allows `&GameState`, so the conservative fallback is `None` for that effect.
- Updated Lua docs (`docs/lua/game-constants.md`, `docs/lua/index.md`, `docs/lua/targeting-and-choices.md`) and the VSCode stub (`docs/lua/stubs/game.lua`) to describe `Game.Counter.*` and the stricter counter spelling.
- Added `crates/engine/tests/strict_lua_validation.rs` with 13 rejection/round-trip tests covering unknown filter keys, misplaced affected flags, unknown option keys for `Search`/`CastSpell`/`ChooseN`, token/landwalk/copy unknown keys, and known/custom counter handling. Updated `crates/engine/tests/script_ops_counters.rs` to expect `Game.Counter.Custom` for flavor counters.
- Also fixed a `clippy::never_loop` denial in `crates/engine/tests/token_cessation.rs` that was blocking `cargo clippy`.

Validation:
- `cargo test -p stackfish-engine` — 1899 passed.
- `cargo clippy -p stackfish-engine --all-targets` — 13 pre-existing warnings, no errors.
- `python3 tools/comment_diff.py ...` — failures are `cargo fmt` import-order changes and the intentional removal of one TODO that this ticket implemented; all 10 remaining TODOs preserved and `python3 -m unittest discover -s tools` passes.

Branch: `phase5/strict-lua-validation`.
