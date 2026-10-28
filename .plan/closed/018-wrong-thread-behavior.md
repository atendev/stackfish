---
id: 018
title: Wrong-thread behavior in the Godot adapter
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

009 promised that a wrong-thread call returns an empty `PackedByteArray`. With gdext's safeguards on, GDScript gets `null` and a script error instead. Document `null`, or compile the check out of release builds with `godot/safeguards-release-disengaged`?

## Notes

- Origin: 009, 015. This was the only open item that contradicted a closed decision.
- Facts: gdext 0.5.5 checks the main thread on every call, in debug and release builds. Only the release-disengaged feature removes it, and only in release builds.

## Resolution

Document `null`. gdext's safeguards stay on in every build. Disengaging them would give `null` in debug builds and an empty array in release builds, so a host could not write one check, and it would drop gdext's other release checks for a return value nothing uses.

- A `handle` or `last_error` call from another thread returns `null` and raises a script error. A host treats `null` from any call as a programming error and does not retry. It reads `last_error()` only after an empty array.
- The adapter's `wrong_thread` code stays as a backstop for builds with gdext's check compiled out. The docs say a host does not rely on it.
- The same contract says that `StackfishSession.new()` off the main thread aborts the process, and that freeing the last reference off-thread ran without a crash in one smoke run, which is not proof. No behavior changes.
- 009 stays as written, with a pointer to this ticket.
- The smoke test pins the result: `null` by default, an empty array and `wrong_thread` with `SAFEGUARDS_OFF=1`.

The edits (class doc comment in `crates/godot/src/lib.rs`, a "Thread contract" section in `docs/implementation/20-godot-adapter.md`, the backlog entry removed, `smoke.gd` and `run.sh`) are on `phase5/wrong-thread-contract`. The smoke change has not run, because there is no Godot binary on this machine.
