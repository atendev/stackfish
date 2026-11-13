# Ticket 171: Fix the CR 702.16 citations in Rust comments

Comment-only change.

## Decisions

1. **Targeting cites 702.16b, not 702.16c.** Changed in `legality.rs`, `choice_answers.rs` and the targeting tests of all six `protection_from_*` test files. Made by the main thread; the auditor agreed.
2. **Blocking cites 702.16f, not 702.16d.** Changed in `combat/declare.rs` (three spots) and the blocking tests. Main thread; auditor agreed.
3. **Aura and Equipment comments cite 702.16c and 702.16d.** Changed in `legality.rs`, `types/card.rs` and the `hexproof_shroud.rs` module doc. Main thread; auditor agreed.
4. **Damage cites 702.16e, not 702.16a (outside the ticket's list).** Damage tests, `replacement.rs` and the color module doc cited 702.16a, which defines the quality. Fixed. Main thread.
5. **Left alone: the assertion message at `hexproof_shroud.rs:872`.** It says 702.16d for an Aura falling off (should be 702.16c). It is a string, so editing it breaks the comment-only rule. Surfaced as a follow-up.
6. **Auditor objection rejected.** The haiku auditor asked to revert the 702.16e test comments to 702.16a. It had misread `git diff -U0` hunk headers: each changed line sits directly above a `combat_damage_..._is_prevented_by_protection` test.

## Verification
`python3 tools/comment_diff.py` exits 0 (0 failing, TODO 323 to 323). `tools/check.sh` passes.
