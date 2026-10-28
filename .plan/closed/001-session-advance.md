---
id: 001
title: How a Session advances
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Does the host call step for every ledger entry, or does the Session run the engine to the next Decision and return the batch of events produced on the way? If both, which is the default and how does a replay viewer use it?

## Notes

- `Engine::step()` returns `StepResult { events_added, status }` (`crates/engine/src/engine/process_state.rs`, `engine/mod.rs:769`). DESIGN.md requires one step-able core for live play and replay.
- Consider: animation pacing in Godot, replay scrubbing, a bot that answers instantly, and what happens to events when an Answer is rejected.

## Resolution

- Answer is the default way to advance. The Session applies the Answer, runs the engine until the next Decision or game over, and returns every event produced on the way as one batch. The host never loops on `step()` itself.
- Advance is a second message, valid only while no Decision is open and the game is not over. It takes an optional step budget. With no budget it uses the Session's default cap. A budget of 1 is a single `step()`. A run that hits the budget returns the events so far with a "still running" status, and the host sends Advance again.
- A new Session runs nothing at creation. The host's first Advance produces the pregame events and the first Decision. Creation can fail only on bad setup (ticket 006).
- Answer takes no budget. It always runs to the next Decision, and the Session's default cap applies to it too.
- Validity: Answer only while a Decision is open and only from the player it names. Advance only while none is open. Anything else gets an error reply with no events and no state change. After game over, both are errors.
- A rejected Answer (invalid before the engine mutates anything) leaves the same Decision open and emits no events. The host may retry.
- A pipeline failure (`StepStatus::Error`) is fatal. The Session reports the error plus the events produced before it, then refuses every later message. Rollback is left to the engine atomicity item in `map.md`.
- Replay is not a Session feature. A viewer is a host. It creates a fresh Session from the recorded seed and setup, sends Advance with budget 1, and sends each recorded Answer when a Decision opens. The Session exports the seed, the setup and the accepted Answers in order. Rewinding builds a new Session and steps forward. Snapshots stay open in DESIGN.md.
- Animation pacing stays in the host. Events carry `sequence_no`, so Godot can play a batch at its own speed.
- `step()` does not map to one ledger entry. It can add zero or several events. DESIGN.md says "one ledger entry" and should say "one state transition".

Handed to other tickets:

- 007 decides whether the last Decision rides in the same response as the final events, and the wire shape of the status values (decision open, still running, game over, error).
- 009 can assume a call blocks for at most one budget slice, so a Godot host can pick a slice size per frame.
- TODO, not solved here: CR 104.4b infinite-loop detection. The budget only stops a runaway from freezing the host.

