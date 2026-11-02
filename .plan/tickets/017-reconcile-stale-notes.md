---
id: 017
title: Reconcile stale notes and policy conflicts
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Bring the closed Host protocol notes back in line with what the code and repo do now, and confirm the items that were derived but never confirmed with the user.

## Notes

- Origin 007, 009, 010, 011 and 001.
- `map.md` says `.plan/` is git-ignored. `README.md` and the global instructions say to commit it, and it is committed. Pick one and fix the other.
- 010 and 011 say "Not pushed yet", but `master` matches `origin/master`. The feature branches for 010 to 015 exist neither locally nor on origin, so confirm that is intended.
- 009 says the drop-off-main-thread behavior is unconfirmed and that creating the object off-thread works. 015 found it safe to drop and found that creation aborts Godot with the safeguards on.
- 007's rejected-Answer response shape and `seat`-less `setup` request were derived and never confirmed. Confirm them with the user.
- DESIGN.md says "one ledger entry" where it should say "one state transition" (001). Its snapshot question stays open until 020.

## Resolution

<Filled in at close: the decision and the reason for it.>
