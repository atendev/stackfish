---
id: 96
title: Script-backed costs, or delete invoke_cost
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Does the engine get costs written as Lua functions, or does `ScriptRuntime::invoke_cost` go away? It is a `todo!()` body kept alive by `#[expect(dead_code)]`, and no payment path calls it.

## Notes

- Origin: backlog item 11 and `docs/lua/not-available-yet.md` ("No script-backed costs", "`Game.Costs.Other` doesn't exist"). `Game.Costs.*` covers every structured cost shape the card set needs today.
- Decide whether any real card needs an unstructured cost, and if so what a cost callback may do (check, pay, undo on unwind). Atomicity rules apply: a rejected input changes nothing (ticket 024).
- Deleting is a valid answer. The `todo!()` in `commands/mod.rs` already went away in ticket 035.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
