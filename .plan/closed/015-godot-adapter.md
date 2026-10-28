---
id: 015
title: Build B6 crates/godot adapter with load smoke test
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build `crates/godot` with a smoke test that loads it in Godot. The test also calls the adapter from a second thread and records whether dropping the object off the main thread is safe.

## Notes

- Branch: `phase5/godot-adapter`. Follow the CLAUDE.md slice workflow: requirements report first, approval before every commit, closing report, user merges.
- Waits on 012 (B3).
- Decisions: 008 (gdext 0.5.5 supports Godot 4.2 to 4.7; messages pass as `PackedByteArray`; the adapter enforces main-thread use itself) and 009 (the Session lives on Godot's main thread with no engine thread or channels; each `#[func]` checks the creating thread; a wrong-thread call returns an empty `PackedByteArray` and logs an error; one Godot class holds one Session; no time limit in the adapter).
- Mark any stubbed parts with TODO.

## Resolution

Built on `phase5/godot-adapter` and merged. Notes are in `docs/implementation/20-godot-adapter.md`.

- `crates/godot` is a `cdylib` (`godot` 0.5.5, `api-4-2`) with one class, `StackfishSession`. It has `create`, `handle` and `last_error`. A plain Rust `Core` holds the Session, the creating thread and a dead flag. A panic kills the instance.
- The smoke test (`crates/godot/smoke/run.sh`) loads the library in Godot 4.7.2 headless, plays a proof game to game over (1140 answers), calls the class from a second thread and drops the last reference off the main thread.
- With gdext's safeguards on (the default), a wrong-thread call is refused by gdext first, and GDScript gets `null` plus a script error. 009's empty array only holds with gdext's check compiled out (`godot/safeguards-release-disengaged`), where the adapter's guard answers with an empty array and `wrong_thread`.
- Dropping the last reference off the main thread did not crash. The Rust destructor ran on the dropping thread.
- Creating the object off the main thread aborts Godot with the safeguards on, and works with them off.
- Open: `create` serves only the proof card set until the carddb `CardSource` exists. Export, web, hot reload and the late-join snapshot are untouched. CI does not run the smoke test, because it needs a Godot binary.
