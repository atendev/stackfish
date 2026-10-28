---
id: 024
title: How the engine recovers from a failed command
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

`StepStatus::Error` resets the engine to `PriorityWindow` but leaves whatever `execute_command` mutated before it failed. 001 made a pipeline error fatal and left rollback to this ticket. What does the engine guarantee when an input is rejected, and how does it keep that guarantee?

## Notes

- Origin: 001, 002, 010, 011.
- Facts: `GameState` has no `Clone` or `PartialEq`. The Lua runtime and its handle registry live on `Engine`, outside `GameState`. The main commands already check first and mutate second (`cast.rs` returns every `Err` before line 393 and mutates from line 483, and `execute_declare_attackers` checks before it taps). Nobody has audited every branch. The failures that matter are found late, in `step()`: menace, must-attack and mode combinations.
- The doc on `StepStatus::Error` (`engine/process_state.rs`) and the "no rollback" notes in `engine/mod.rs` are the places that state the old behavior.

## Resolution

A rejected input changes nothing. The engine keeps that by checking everything before it mutates anything. It does not snapshot and restore.

- Guarantee. A rejected `PlayerAction` or Answer leaves `GameState` and the ledger as they were, so the host can retry. This is what 001 promised.
- Scope. The guarantee covers input rejection only. A script error during resolution stays "log a `ScriptError` event and continue". An engine bug (a broken invariant or a panic) stays fatal. `Session` catches the panic, marks itself dead and reports a stable error code. The Godot adapter already catches panics, `Session` does not.
- Mechanism. Validate before mutate. Every command runs all its checks first and touches state afterwards. Snapshot and restore was rejected: it needs `Clone` on all of `GameState`, and it would also have to restore the ledger, the RNG state and the Lua handle registry on `Engine`. A rejection that leaves a mutation behind is a bug in the command, and the test harness below is how we catch it.
- Unit. One Answer. Each rejected Answer changes nothing. A cast that was accepted and then unwinds through the existing payment unwind keeps that flow. No second rollback mechanism.
- Errors. Split `CommandError` into `Rejected` and `Internal`. `Rejected` can only be built in the validation phase. The Session turns `Rejected` into a normal rejection response and continues, and turns `Internal` into the fatal path.
- Enforcement. The engine enforces its own rules. `submit_action` and the other `submit_*` methods validate with the shared predicates, so a rejection does not depend on the entry point. The facade check against the entries stays as an early filter.
- Entries. Menace, must-attack and impossible mode pairs are removed from `legal_attackers`, `legal_blockers` and the modal entries. One shared predicate serves the entries and the command validation, so the two cannot drift apart.
- Tests. Derive `Clone` and `PartialEq` on the `GameState` type graph behind a test-only feature and compare exact state before and after a rejection. If the derive spreads too far (Lua handles, for example), fall back to a fingerprint of ledger length, `next_object_id`, RNG draws consumed and a sorted listing of zones and object flags. Feed the harness many illegal inputs against many states, and let the proof-game bot supply states.

Build tickets, in this order:

1. 025 Atomicity harness and audit.
2. 035 Error split and Session mapping.
3. 026 Whole-declaration combat predicates.
4. 036 Mode-combination predicate.
