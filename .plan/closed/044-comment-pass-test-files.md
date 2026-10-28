---
id: 044
title: Comment pass on test files
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Apply the comment rules from `CLAUDE.md` to every test file under `tests/` directories, with no code change. Keep every `TODO` and every rule cite. Keep comments that say which rule a test pins.

## Notes

- Map: `map-cleanup.md`, section A. Blocker 040 is closed.
- Scope: `crates/engine/tests` (224 files, about 8,130 comment lines), `crates/scenario/tests` (5 files), `crates/carddb/tests` (1 file), `crates/host/tests` (7 files).
- A test comment is one `// CR <number>` line that names the rule the test pins, plus a why line only when the setup is not obvious.
- One directory per commit. Permission asked before each commit.
- Before asking to commit a directory, `python3 tools/comment_diff.py <path>` must exit 0, and that crate's tests must pass.
- Branch `phase5/comment-pass-tests`, cut from `master`.

## Resolution

Done and merged to `master` (merge 8550105, after 5445a65, 6776c96, and 2087a05).

- 237 test files checked across `crates/engine/tests/` (224 files), `crates/scenario/tests/` (5 files), `crates/carddb/tests/` (1 file), and `crates/host/tests/` (7 files).
- Comment lines reduced from 8,230 to 1,727 (~79% reduction).
- Zero code changes; `python3 tools/comment_diff.py` against base exited 0 across all files.
- All 28 existing `TODO`s preserved; 49 new one-line `TODO`s surfaced from prose notes on test gaps and edge cases.
- All tests in engine (2,024 tests), scenario, carddb, host, and tool unittests pass.
