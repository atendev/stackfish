---
id: 017
title: Reconcile stale notes and policy conflicts
type: task
mode: HITL
status: closed
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

- `.plan/` is committed. `map.md` now says so, which matches `README.md` and the repo. Only `.agent-state.md` stays out of git.
- The user confirmed that deleting the branches for 010 to 015 was intended, because the slice workflow deletes a branch after its merge. 010 and 011 no longer say "Not pushed yet". Their merge commits on master are the record.
- 009 now points to 015 and 018 for its two threading claims. Creating `StackfishSession` off the main thread aborts Godot with the safeguards on. Dropping it off-thread ran without a crash in one smoke run, and a wrong-thread call gives `null`.
- The user confirmed 007's rejected-Answer response (`decision_open` for the same Decision, empty `events`, an `error`) and the `seat`-less `setup` request. `crates/host/src/session.rs` already does both, so no code changes.
- DESIGN.md already said "one state transition", so it needed no edit. 001's note now says so. Its snapshot question stays open for 020.
- 041 also said master was not pushed. It now matches the repo.
