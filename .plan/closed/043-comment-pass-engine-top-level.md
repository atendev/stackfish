---
id: 043
title: Comment pass on engine top-level modules
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Apply the comment rules from `CLAUDE.md` to `sba.rs`, `replacement.rs`, `layers.rs`, `triggers.rs`, `script_api.rs`, `engine_context.rs`, `color_identity.rs` and `lib.rs` under `crates/engine/src/`, with no code change. `tools/comment_diff.py` must exit 0 before each commit is proposed.

## Notes

- Map: `map-cleanup.md`, section A. Blocker 040 is closed; 041 and 042 are merged, so their rule settlements apply.
- Size, measured 2026-10-28 on `master`. Comment lines / total lines: `sba.rs` 591/1203, `replacement.rs` 452/1257, `layers.rs` 276/652, `triggers.rs` 192/412, `script_api.rs` 59/59, `engine_context.rs` 26/43, `color_identity.rs` 35/96, `lib.rs` 0/14.
- Keep every `TODO`. Rewrite `TODO(tag):` forms to `// TODO:`. Cite rules by number only. No history words.
- Doc comments keep a short description beside the CR number. Check the item a doc block sits on.
- Branch `phase5/comment-pass-engine-top-level`, cut from `master`. Permission asked before each commit.

## Resolution

Done and merged to `master` (merge 5a8ab4a, after 53424a3 and c55926c).

- Eight files under `crates/engine/src/`: comment lines 1,631 to 400, `TODO` 6 to 12. `python3 tools/comment_diff.py` exits 0 and `cargo test -p stackfish-engine` passes 2,024 tests.
- Two commits: the three small modules (`script_api.rs`, `engine_context.rs`, `color_identity.rs`), then `triggers.rs`, `layers.rs`, `replacement.rs` and `sba.rs`. `lib.rs` had no comments.
- Six new `TODO`s came from prose notes about real gaps: lost departed triggers and lost earlier death events at an ordering-choice pause (`sba.rs`), Umbra armor and commander return skipping replacement checking (`sba.rs`), effective controller not read at gameplay sites (`layers.rs`), and Madness not tracking which exile effect won (`replacement.rs`). Ticket 047 should check them against the backlog.
- Stale notes dropped after checking the code: `SpellCast` exists, commander damage is counted, `StackEntryKind::Copy` is handled, and the old `sba.rs` TODO items (CDA toughness, layer consumers, regeneration) are built.
