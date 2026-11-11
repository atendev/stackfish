---
id: 149
title: Fix Lua key-error order and lint hash set algebra
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Build fixes 1 and 2 from "Audit the remaining order leaks": make the Lua key checks name bad keys in a fixed order, and add the hash set algebra methods to the clippy lint.

## Notes

- Origin 106. The findings are in `docs/research/order-leaks.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- `reject_unknown_keys` (`script/decode.rs`), `decode_entry_options` (`script/globals.rs`) and the `Game.ChooseN` opts check in the Lua prelude (`script/globals.rs`) walk a table with `pairs` and error on the first bad key. Lua 5.4 seeds string hashes per state, so the named key changes between runtimes and reaches every Seat through `ScriptError.message`. Collect the bad keys, sort them, and name them in a fixed order.
- Test: load a script with several unknown keys in a fresh `ScriptRuntime` many times, in one test, and assert one message every time. Cover a game-time path as well, such as `Game.ChooseN` with two unknown opts keys.
- Add `HashSet::union`, `difference`, `intersection`, `symmetric_difference`, `HashMap::extract_if` and `HashSet::extract_if` to `disallowed-methods` in `clippy.toml`. Check that clippy accepts each path on the pinned toolchain.
- Add a rule to the Lua scripting docs (`docs/lua/`): a script that needs order walks with `ipairs` or sorts its keys, and never lets `pairs` order over string keys decide an outcome. Check `docs/lua/stubs/game.lua`; the API does not change.
- Keep the TODO at `script/runtime.rs:46`.

## Resolution

<Filled in at close.>
