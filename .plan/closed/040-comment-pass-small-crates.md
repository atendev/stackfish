---
id: 040
title: Comment pass on the small crates and scenario
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Apply the comment rules from `CLAUDE.md` to `carddb`, `cli`, `godot`, `host`, `scenario` and `testutil`, with no code change. This is the pilot that proves the rules and `tools/comment_diff.py` before the engine passes (041 to 044).

## Notes

- Map: `map-cleanup.md`, section A. Blockers 038 and 039 are closed.
- One crate per commit. Permission asked before each commit.
- Before asking to commit a crate, `python3 tools/comment_diff.py crates/<crate>` had to exit 0.
- Keep every `TODO`. Cite rules by number only. No history words in comments.
- Branch `phase5/comment-pass-small-crates`, cut from `master` after `phase5/comment-rules` was merged. Not yet merged.

## Resolution

All six crates are done, in six commits, one per crate. Against the branch base, `tools/comment_diff.py` reports 56 files, comment lines 1,805 to 740, `TODO` 24 to 27 and 0 failing. No code changed. The tests of every crate touched pass.

| Crate | Comment lines before | After |
|---|---|---|
| cli | 14 | 3 |
| testutil | 62 | 28 |
| carddb | 133 | 48 |
| godot | 76 | 46 |
| host | 552 (script count) | 342 |
| scenario | 968 | 273 |

What the pilot showed about the rules and the check:

- The rules work. Module docs fit in three lines, and the long ones pointed at `docs/DESIGN.md` or `docs/scenario_format.md` without losing anything.
- The check counts the word "TODO", prose mentions included. A comment that only referred to another `TODO` failed the check when it was cut. Keep a one-line `TODO` marker where such a reference matters.
- The check cannot tell a new `TODO` from a kept one. The pass added three in `scenario` and one in `cli` by turning "not yet supported" notes into `TODO`s. Say so in the commit message so the user can veto them.
- A grep for history words (ticket, phase, slice, backlog, "used to") is a quick second check after the script. It found nothing after the pass.
- Rewriting a whole file with the check as the safety net is practical for a file that is mostly comments, like `schema.rs` (435 to 130). The script proves the code is unchanged.
- Private items lose their doc comments unless they hold a trap. Public wire and schema docs mostly stay, because they are the protocol reference.

Rule gaps to settle before 041 to 044:

- `TODO(tag):` forms were rewritten to `// TODO:`. The rule says this, but the engine has many more of them.
- A rule cite that also explains a trap, such as the CR 111.7 token case, takes a `// CR` line plus one why line. That fits the test-comment rule, and the pass used it in non-test code too.
- Variant docs like `{C} (CR 107.4c): colorless mana only` were left alone. They carry a short description with the cite. Decide whether that counts as a label after the number.
