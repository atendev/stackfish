# Requirements: Fix Lua key-error order and lint hash set algebra

Ticket: `.plan/tickets/149-fix-lua-key-error-order.md`. Findings: `docs/research/order-leaks.md` (fixes 1, 2 and 3).

## What success looks like

### 1. Key checks name bad keys in a fixed order

A Lua table with two or more bad keys gives the same error text on every runtime, so a `ScriptError` event's `message` is the same bytes for the same seed and Answers.

Key naming, the same rule on the Rust and Lua sides:

- a string key is named by its text,
- an integer key by its decimal text (`1`), which matches what the Rust walk prints today through mlua's string coercion (Lua 5.4 stores a float key with an integral value as an integer),
- any other key (float, boolean, table, function, userdata) by `<` + its Lua type name + `>`, for example `<number>` or `<boolean>`, never by `tostring`, which prints an address for a table.

Names are sorted by byte order. Rust sorts `String`s. Lua's `table.sort` compares strings with `strcoll`, and the engine never calls `setlocale`, so the C locale applies and that is byte order too.

Templates (single quotes on every site):

- `reject_unknown_keys` (`crates/engine/src/script/decode.rs:302`): one bad key, `{context} has unknown key 'x'` (unchanged). Two or more, `{context} has unknown keys 'a', 'b'`. Both contain `unknown key`, so existing tests keep passing.
- `Game.ChooseN` opts check (Lua prelude, `crates/engine/src/script/globals.rs:1418`): the same two templates with context `Game.ChooseN opts`.
- `decode_entry_options` (`crates/engine/src/script/globals.rs:1663`): one bad key, `{op_name} option 'x' is unknown or has the wrong type (expected control = "you"|"owner", tapped = boolean)`. Two or more, `{op_name} options 'a', 'b' are unknown or have the wrong type (expected control = "you"|"owner", tapped = boolean)`. The key quote changes from `{other:?}` double quotes to single quotes, to match the other two sites. A wrong-typed known key (`control = 5`, `tapped = "yes"`) counts as a bad key. Bad keys are reported before a bad `control` value: the walk collects all bad keys first and checks the `control` string only when there are none, so `{ control = "Owner", zeta = 1 }` names `'zeta'`. The `control` value error text is unchanged.

`reject_unknown_keys` reads keys as `Value`, not `String`, so a non-string key cannot stop the walk with a conversion error.

### 2. Lint

`clippy.toml` `disallowed-methods` also lists `std::collections::HashSet::union`, `std::collections::HashSet::difference`, `std::collections::HashSet::intersection`, `std::collections::HashSet::symmetric_difference`, `std::collections::HashMap::extract_if` and `std::collections::HashSet::extract_if`, with the same reason text as the other `HashSet` and `HashMap` entries.

Current uses, from `grep -rnE "\.(union|difference|intersection|symmetric_difference|extract_if)\(" crates` and the path-style form `grep -rnE "(HashSet|HashMap)::(union|difference|intersection|symmetric_difference|extract_if)" --include=*.rs .` over the whole repo (no path-style hits):

- `crates/host/src/replay.rs:100-101`: `symmetric_difference` and `intersection` on `used`, a `BTreeSet<String>` (`crate::setup::keys` returns `BTreeSet`). Not a hash set, so the lint does not apply. No change.
- `crates/host/tests/seat_visibility.rs:885`: `HashSet::union` on two `HashSet<String>`s from the `ids_of` closure, collected into a `HashSet`. The result is order-free, but the lint will refuse it and `--all-targets` lints tests. Change: build `libraries` with `let mut libraries = ids_of("library", 0); libraries.extend(ids_of("library", 1));`. Same set, no hash-order walk the lint names.

Path check on the pinned 1.98.0 toolchain: add a temporary `pub fn __lint_probe()` to `crates/engine/src/lib.rs` that calls each of the six methods once on a `HashSet<u32>` or `HashMap<u32, u32>`, run `cargo clippy --workspace --all-targets -- -D warnings` (the exact command at `tools/check.sh:38`, checked), and confirm six `disallowed_methods` errors, one per path. A path clippy cannot resolve shows up as a config warning instead, so the check also confirms there is no such warning. Remove the probe, rerun, and confirm a clean pass. Record the output in the decision report, `.plan/assets/149-fix-lua-key-error-order-report.md`.

### 3. Lua docs rule

A new `## Iteration order` section in `docs/lua/card-script-basics.md` (the file exists and `docs/lua/index.md:5` links it): a script that needs order walks with `ipairs` or sorts its keys, and never lets `pairs` order over string keys decide an outcome, because Lua seeds string hashes per state and the order can change between runs and replays. The index line for the file gets "iteration order" added to its topic list. `docs/lua/stubs/game.lua` was checked: `Game.ChooseN` (`:1393`) and the reanimation ops keep their signatures, so the stub needs no change.

### 4. DESIGN.md

The Lua sandbox bullet in `docs/DESIGN.md` (lines 232-235) gains a sentence: the engine's own key checks name bad keys in sorted order, and scripts must not let `pairs` order decide an outcome (`docs/lua/card-script-basics.md`). The clippy bullet (lines 246-249) notes that the list covers the hash set algebra methods and `extract_if` as well. The existing "(TODO)" at the end of the sandbox bullet stays.

## Comprehensive rules

None. This is engine determinism, not a game rule. No CR citation is added.

## TODOs

Kept, not changed:

- `crates/engine/src/script/runtime.rs:46`: `// TODO: Lua 5.4 seeds string hashes per state, so pairs order over string keys can differ between runs`. Card scripts can still walk with `pairs`. The docs rule covers them, and the sandbox does not replace `pairs` (the research recommends against it).
- `docs/DESIGN.md:234-235`: "Lua 5.4 still seeds string hashes per state, so `pairs` order over string keys can differ between runs (TODO)."

No new TODO. `into_iter`, `extend` and `from_iter` on hash containers stay outside the lint, as DESIGN.md already says.

## Comments

- `reject_unknown_keys` doc comment (`decode.rs:298-301`) becomes plural: it rejects a table whose keys are not all in `allowed` or `extra_allowed`, and names every bad key in sorted order so the message does not depend on Lua's per-state hash seed.
- `decode_entry_options` doc comment gains the same why in one clause.
- The `Game.ChooseN` prelude gets a one-line Rust comment above the `choose_n` load saying the opts check sorts its bad keys for the same reason.
- A private `lua_key_name` helper in `decode.rs` gets a comment only for the integer/other split, since the code shows the rest.
- No history words, no ticket numbers, no rules text.

## Tests

All in `crates/engine/tests/strict_lua_validation.rs`. The operation-function shape is the one `script_operation.rs:88-89` builds: `local s = {}\nfunction s.operation()\n  return function(source, controller)\n<body>\n  end\nend\nreturn s`, where `<body>` is the `return ...` line each test names. The harness uses public API only: `ScriptRuntime::new`, `ScriptRuntime::load_operation` (already used there), and `ScriptRuntime::build_operation` (`runtime.rs:499`, public) plus `ScriptRuntime::invoke_operation` (`runtime.rs:834`, public), the same pair `crates/engine/tests/script_operation.rs:84-100` uses. The game state comes from `stackfish_testutil::two_player_state(Vec::new())`, as there. Every error is compared as `format!("{err:?}")`, the same form as the existing `assert_load_error`. Each order test loads or runs in a fresh `ScriptRuntime` 32 times, collects the messages into a `BTreeSet`, asserts the set has one entry, and asserts that entry contains the full expected text.

1. Load time, `reject_unknown_keys`: `return Game.Filter({ zeta = true, alpha = true, mid = true })`. Expected text: `Game.Filter has unknown keys 'alpha', 'mid', 'zeta'`.
2. Load time, `decode_entry_options`, several keys: `return Game.Ops.ReturnToBattlefield({ zeta = 1, alpha = 1 })`. Expected: `options 'alpha', 'zeta' are unknown or have the wrong type (expected control = \"you\"|\"owner\", tapped = boolean)` (with the `Debug` escaping of the quotes).
3. Load time, `decode_entry_options`, one key: `return Game.Ops.ReturnToBattlefield({ zeta = 1 })`. Expected: `option 'zeta' is unknown or has the wrong type`.
4. Load time, key before value: `return Game.Ops.ReturnToBattlefield({ control = "Owner", zeta = 1 })` names `'zeta'` and does not contain `Owner`. A second case, `{ control = 5, tapped = true }`, names `'control'` as a bad key.
5. Game time, `Game.ChooseN`: an operation function `return Game.ChooseN({}, 1, { zeta = true, alpha = true })` built with `build_operation` and run with `invoke_operation`. Expected: `Game.ChooseN opts has unknown keys 'alpha', 'zeta'`.
6. Game time, `decode_entry_options`: an operation function `return Game.Ops.ReturnToBattlefield({ zeta = 1, alpha = 1 })` in the same harness, same expected text as test 2.
7. Non-string keys: `return Game.Filter({ [true] = 1, [2] = 1, [2.5] = 1, zeta = 1 })`. Expected: `unknown keys '2', '<boolean>', '<number>', 'zeta'`. The Lua side: `return Game.ChooseN({}, 1, { [true] = 1, [2] = 1, [2.5] = 1, zeta = true })` through `load_operation`. Expected: `Game.ChooseN opts has unknown keys '2', '<boolean>', '<number>', 'zeta'`. This reaches the prelude: `load_operation` evaluates the chunk (`runtime.rs:131-135`, `.load(source).eval()`), so the top-level `Game.ChooseN` call runs its opts check before any yield, which is how the existing `:99` test works. The Lua side tells integers from floats with `math.type(k) == "integer"`. An integral float key such as `[2.0]` needs no case: Lua 5.4 stores it as the integer key `2` (`luaH_set` normalises float keys with an exact integer value), so both sides see an integer.
8. The existing single-key tests at `strict_lua_validation.rs:25`, `:41`, `:91`, `:99` and `:137` pin the singular `reject_unknown_keys` and `ChooseN` text, and `:72` pins the `control` value text. They must still pass unchanged.

What the tests catch: on the old code, tests 1, 2, 5 and 6 fail on every run, because the old text names one key and never contains the sorted list. That pins the sorted output. To show the identical-across-32-runtimes check catches the seed bug itself, the implementer runs tests 1 and 5 once against an intermediate build that collects every bad key but does not sort, and records whether they fail. Probe B in the research saw runtimes in one process disagree, so with 32 runtimes and three keys a failure is likely but not certain. The result goes in the decision report either way. The sorted-text assertion is the guarantee, and the 32-run check is the regression guard.
