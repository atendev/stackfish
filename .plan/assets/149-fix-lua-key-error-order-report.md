# Decision report: Fix Lua key-error order and lint hash set algebra

Branch `phase5/fix-lua-key-error-order`. Requirements: `.plan/assets/149-fix-lua-key-error-order-requirements.md`. Research: `docs/research/order-leaks.md`.

Commits:

- `db1437e` adds the requirements report.
- `650c27c` fixes the key checks and adds the lint entries.
- `3b33ce4` documents the iteration order rule.

## Gates

- The requirements audit used one `haiku` auditor. It returned NOT APPROVE in round 1 (10 items) and in round 2 (5 items), then APPROVE in round 3.
- The implementation audit used one `sonnet` auditor and returned APPROVE in round 1. Its one required item was this report.
- The commit gate was skipped by the master's instruction: commit every time, and the master merges. The CLAUDE.md `Merge` / `Don't merge` question is the master's to ask.
- `tools/check.sh --full` passed on `3b33ce4`: fmt, clippy, 2640 workspace tests with 0 failures, and the tools unittests. `db_scripts_smoke.rs` skipped because `stackfish-db` is not checked out.

## Decisions

Each entry gives the question, the options, the choice, who made it and the audit verdict.

1. **How should several bad keys be named?** The options were to name every bad key sorted, or to name only the smallest one. The implementer chose every key, sorted, because a script author fixes all their typos in one pass. Requirements auditor: APPROVE.

2. **What text should the error use?** One key keeps `{context} has unknown key 'x'`. Two or more give `{context} has unknown keys 'a', 'b'`. Both still contain `unknown key`, so no existing test changed. `decode_entry_options` uses `option 'x' is unknown or has the wrong type (...)` and `options 'a', 'b' are unknown or have the wrong type (...)`. Its key quoting changed from `{other:?}` double quotes to single quotes, to match the other two sites. The implementer chose the text and the requirements auditor asked for it to be written out in full (round 1). APPROVE.

3. **How should a non-string key be named?** The options were Lua's `tostring`, the mlua string coercion, or a type tag. The rule is the same on both sides: a string by its text, an integer in decimal, and anything else as `<type>`. `tostring` prints an address for a table, and a float's text can differ between Lua and Rust. The Lua side tells an integer from a float with `math.type`. The requirements auditor found the first draft inconsistent between Rust and Lua (round 1) and asked for a float case (round 2). Both were fixed. APPROVE.

4. **How should names be sorted?** By bytes. Rust sorts `String`s. Lua's `table.sort` uses `strcoll`, and the engine never calls `setlocale`, so the C locale applies, which is byte order. During implementation the implementer found that the Lua side sorted the quoted names, which puts `'a!'` before `'a'` because `!` is below the quote mark. It now sorts first and quotes after. The test `rust_and_lua_key_checks_sort_names_by_bytes` pins this. The implementation auditor approved it.

5. **Which error comes first in `decode_entry_options`, a bad key or a bad `control` value?** Bad keys first. The function collects all bad keys, and checks the `control` string only when there are none. A wrong-typed known key such as `control = 5` counts as a bad key. The implementer made this choice, and the requirements auditor asked for a test (round 1). APPROVE.

6. **What about the existing `HashSet::union` call in `crates/host/tests/seat_visibility.rs`?** The new lint refuses it, because `--all-targets` lints tests. The test builds the same set with `extend`, which the lint does not cover and which does not leak order into a set. The `symmetric_difference` and `intersection` calls in `crates/host/src/replay.rs` run on a `BTreeSet` and are unchanged. A whole-repo grep for the path-style form found no other hits. The implementer made this change. APPROVE.

7. **Does clippy accept the six paths on the pinned 1.98.0 toolchain?** Yes. A temporary probe in `crates/engine/src/lib.rs` called all six methods. `cargo clippy --workspace --all-targets -- -D warnings` reported exactly six `use of a disallowed method` errors, one each for `HashMap::extract_if`, `HashSet::difference`, `HashSet::extract_if`, `HashSet::intersection`, `HashSet::symmetric_difference` and `HashSet::union`, and no config warning. The probe was removed and the rerun was clean.

8. **Do the tests catch the seed bug and not just the new text?** Yes. On the old code, three tests failed on the "messages differ" check across 32 fresh runtimes, so the old behaviour varies within one process. With both sorts disabled, 5 tests failed, 4 of them on that same check. Then the sorted-text assertions pin the output.

9. **Where does the docs rule go?** A new `## Iteration order` section in `docs/lua/card-script-basics.md`, with a mention in the `docs/lua/index.md` entry. `docs/LUA_API.md`, which is the canonical reference, got a line in "Strict validation rules" that links the section. `docs/DESIGN.md` mentions the sorted key errors and the wider lint. `docs/lua/stubs/game.lua` needs no change, because no signature changed. The implementer added the `LUA_API.md` line, which goes beyond the report. The implementation auditor approved it.

10. **TODOs.** The TODO about per-state string hash seeds stays at `crates/engine/src/script/runtime.rs:48` (the report said `:46`, which was stale) and in `docs/DESIGN.md`. The implementation adds no new TODO.

## Accepted residuals

- A non-UTF-8 string key reads as U+FFFD on the Rust side (`to_string_lossy`) and as raw bytes on the Lua side (`tostring`). Both sides are deterministic. Only the text differs. The implementation auditor rated this non-blocking.
- `reject_uncalled_op` (`decode.rs:86`) returns the first `Game.Ops` name it finds in `pairs` order. The research rated it no leak, because no two names share a function. Unchanged.
- Card scripts can still walk string keys with `pairs`. The docs rule covers them, and the sandbox does not replace `pairs`.
