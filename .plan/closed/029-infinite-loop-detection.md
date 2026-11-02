---
id: 029
title: CR 104.4b infinite-loop detection
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Detect a mandatory loop of actions and end the game as a draw. CR 104.4b

## Notes

- Origin 001.
- The step budget only stops a runaway from freezing the host.
- The requirements report must choose how a mandatory loop is detected.

## Resolution

A forced priority pass is one where the passer has only Pass and mana abilities whose whole cost is {T}. Each forced pass with a non-empty stack fingerprints the state in `engine/loop_watch.rs`, and a repeat ends the game as `GameOver { winner: None }`. Any other accepted action or any non-priority pause clears the watch, since a real choice sits between the two states. The fingerprint is the state's `Debug` text without the ledger and id counters, with ids replaced by their rank, hash collections sorted, and the ledger values read at priority folded in. A false draw ends a game wrongly, so every gap errs toward a missed loop, except Lua-held script state, which is a TODO. Building it exposed and fixed a stale graveyard object left by Persist and Undying returns. Report: `docs/implementation/30-loop-detection.md`.
