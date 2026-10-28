---
id: 042
title: Comment pass on engine core
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Apply the comment rules from `CLAUDE.md` to `crates/engine/src/engine/`, `crates/engine/src/commands/` and `crates/engine/src/decision/`, with no code change. `tools/comment_diff.py` must exit 0 before each commit is proposed.

## Notes

- Map: `map-cleanup.md`, section A. Blocker 040 is closed and merged. 041 is merged, so its rule settlements apply.
- Size, measured 2026-10-28 on `master`. `engine/`: 28 files, 13,578 lines, 4,727 comment lines. `commands/`: 14 files, 6,047 lines, 1,930 comment lines. `decision/`: 6 files, 1,256 lines, 220 comment lines.
- One area per commit (`decision/`, `commands/`, then `engine/`, smallest first). Permission asked before each commit.
- Keep every `TODO`. Rewrite `TODO(tag):` forms to `// TODO:`. Cite rules by number only. No history words.
- Rule settled in 041: a doc comment keeps a short description beside the CR number, and labels such as `IMPORTANT` go.
- Mis-attached doc blocks are a known hazard from 041. Check the item a doc block sits on.
- The `StepStatus::Error` and `engine/mod.rs` "no rollback" comments were rewritten by the atomicity work (025). Check them against the code before trimming.
- Branch `phase5/comment-pass-engine-core`, cut from `master`. Open files in 026 and 036 review may touch `commands/` and `decision/`, so check for overlap before cutting.
- Run `python3 tools/comment_diff.py [paths]` per area, then the engine tests.
- Progress. `decision/` is done and merged (3791a68): comment lines 220 to 195, TODOs 3 to 3, engine tests pass. `commands/` is committed (ca1c09e) and `engine/` is committed (9439a3e) on `phase5/comment-pass-engine-core`, both pushed and awaiting merge. `engine/`: 28 files, comment lines 4,728 to 1,397, TODOs 23 to 37, 2,024 engine tests pass. One TODO removed: the CR 111.7 token cessation note, since `collect_ceasing_tokens` in `sba.rs` enforces it.

## Resolution

All three areas are done and merged to `master` (merge 5098bbf, after `decision/` 3791a68, `commands/` ca1c09e and `engine/` 9439a3e).

- `crates/engine/src/engine`, `commands` and `decision`: 48 files, comment lines 2,843 to 2,843 against `HEAD`, TODOs 72 to 72. `python3 tools/comment_diff.py` exits 0.
- `cargo test -p stackfish-engine`: 2,024 passed, 0 failed.
- The CR 111.7 token cessation note was removed from `engine/`, since `collect_ceasing_tokens` in `sba.rs` enforces it.
