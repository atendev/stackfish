---
id: 039
title: Comment rules and the comment-only diff check
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

What are the comment rules for `CLAUDE.md`, and what script proves a comment pass changed no code, and where does that script live?

## Notes

- Map: `map-cleanup.md`, section A. Blocks 040, which is the pilot comment pass.
- Proposed rules from the map. Comment why, not what. Cite a rule in one line as `// CR 704.5g` and never quote or paraphrase its text. Public items get one or two lines of doc. No ticket numbers or history in comments. Keep every `TODO`.
- Script: strip comments from the source before and after a pass and diff the results. An empty diff proves no code changed.
- Existing comments quote CR text at length, for example `crates/engine/src/sba.rs`. Engine non-test source is 38% comments.
- Skills: grilling, domain-modeling, unslop.

## Resolution

Rules are in the new Comments section of `CLAUDE.md`. The decisions:

- A rule citation is the CR number only. No quoted or paraphrased text, no label.
- No history in comments: no tickets, backlog items, phases, slices, branch names, "used to", "now", "new" or "see note N". State the reason if it still matters.
- Public items get two lines, up to about five for an invariant, a rule interaction or a trap. `//!` module docs are three lines or fewer. Private items get a comment only for a why the code cannot show.
- Every `TODO` stays, in the form `// TODO: <what is missing>` with an optional CR cite. Wording can be trimmed, but a `TODO` is never deleted.
- Rust source and Rust tests only. A test comment is one `// CR` line naming the rule it pins, plus a why line when the setup is not obvious.

The check is `tools/comment_diff.py`, Python with the standard library only, with tests in `tools/test_comment_diff.py`. It lexes each `.rs` file (nested block comments, raw strings, byte and char literals), drops comments, ignores whitespace, and compares against a git ref (default `HEAD`) for the given paths (default `crates`). It exits 1 on any code difference or a drop in `TODO` count, and prints comment-line counts per changed file. On the unchanged tree it lexes all 363 files and reports no differences. A comment-only edit passes, and a code edit or a deleted `TODO` fails. If the script ever reports a false difference on a tree it should accept, switch to a Rust tool built on `proc-macro2`.

