---
id: 035
title: Error split and Session mapping
type: task
mode: HITL
status: closed
blocked_by: [025]
claimed: false
---

## Question

Split `CommandError` into `Rejected`, which only the validation phase can build, and `Internal`. Map them in the Session, so a refusal leaves the game running and an internal error or a panic ends it with a stable code.

## Notes

- Origin 001 and 024.
- This file was written after the merge, from the commits, because the ticket was tracked only inline on `map-fixes.md`.

## Resolution

Every command arm is a validate step that only reads state and returns `Rejected`, followed by an apply step that can only fail with `Internal`. `Engine::submit_action` runs the validator, so every refusal comes out of `submit_action` with nothing changed. The first `Internal` error makes the engine fatal (`Engine::fatal_error`).

The Session answers a refusal with `invalid_answer` and keeps the same Decision open. An `Internal` error halts it as `pipeline_error`. A panic is caught in `handle` and `create`, marks the Session dead, and every later request gets `engine_panic`. Both fatal cases send Seats a fixed message and keep the detail in `Session::internal_error`.

Built in `0e50983` on `phase5/error-split`, merged as `bb832d4`. Notes are in `docs/implementation/22-error-split.md`.
