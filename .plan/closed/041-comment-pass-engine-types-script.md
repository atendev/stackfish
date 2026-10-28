---
id: 041
title: Comment pass on engine types and script
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Apply the comment rules from `CLAUDE.md` to `crates/engine/src/types/` and `crates/engine/src/script/`, with no code change. `tools/comment_diff.py` must exit 0 before each commit is proposed.

## Notes

- Map: `map-cleanup.md`, section A. Blocker 040 is closed and merged.
- One area per commit (`types/`, then `script/`). Permission asked before each commit.
- Keep every `TODO`. Rewrite `TODO(tag):` forms to `// TODO:`. Cite rules by number only. No history words.
- Rule gaps from 040 to settle in the requirements report: `TODO(tag)` forms, a CR cite plus one why line outside tests, and variant docs of the form `{C} (CR 107.4c): colorless mana only`.
- Branch `phase5/comment-pass-engine-types-script`, cut from `master`.
- Jev audit: architecture_refactor, sonnet, risk 1.0%.

## Resolution

Both areas are done in two commits on master (merge 6dae011). `tools/comment_diff.py` reports 27 files, comment lines 6,960 to 3,554, 0 failing. No code changed, and the engine tests pass (2,024 tests, 226 binaries).

- `types/`: 5,040 to 2,509 comment lines. `script/`: 1,920 to 1,045.
- `TODO` mentions rose from 70 to 103. 33 are new one-line TODOs made from "not modeled yet" notes, listed in the commit messages for veto. One stands in for a mention of a finished item.
- Rule gap settled: a doc comment keeps a short description beside the CR number, and labels such as `IMPORTANT` go.
- Mis-attached doc blocks were fixed (EffectPrimitive in effect.rs, decode_affected_value in decode.rs).
- The branch was deleted locally and on origin. `master` is 3 commits ahead of origin and not pushed.
