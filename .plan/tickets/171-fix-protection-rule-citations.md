---
id: 171
title: Fix the CR 702.16 citations in Rust comments
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Which Rust comments cite the wrong CR 702.16 subrule, and what should each one cite? Fix each one.

## Notes

- Found during the stale note sweep. `docs/comp_rules.md` numbers the subrules this way: 702.16b is targeting, 702.16c is Auras, 702.16d is Equipment and Fortifications, 702.16e is damage and 702.16f is blocking.
- Comments that cite 702.16c for targeting: `commands/legality.rs:261`, `engine/choice_answers.rs:154`, and the targeting tests in `tests/protection_from_{everything,card_type,subtype,player}.rs`.
- Comments that cite 702.16d for blocking: `commands/combat/declare.rs` (the doc comment near line 552 and the comments near 639 and 739), and the blocking tests in the same four test files.
- Comments that cover both Auras and Equipment but cite only 702.16d: `commands/legality.rs:391` and the module doc of `tests/hexproof_shroud.rs`.
- Check each hit against the rule text before editing. Comment-only change, so run `python3 tools/comment_diff.py` and get exit code 0.

## Resolution

<Filled in at close.>
