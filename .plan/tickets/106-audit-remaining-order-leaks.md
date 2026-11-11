---
id: 106
title: Audit the remaining order leaks
type: task
mode: AFK
status: open
blocked_by: []
claimed: true
---

## Question

Which uses of `into_iter`, `extend`, `from_iter` and Lua `pairs` over unordered containers can change what a Seat sees or what a replay plays out? Ticket 075's lint refuses `for` loops and walking methods over hash containers, but accepts these four.

## Notes

- Origin: the Proof game entry in `99-backlog.md`, 075's Resolution, and the Host Session note that Lua `pairs` order over string keys is not seeded.
- Report each site with the container type, whether its order reaches state, a Decision candidate list, the ledger or the replay, and a fix (ordered container, sort, or none needed). Do not change code.
- Test the `pairs` question by running one script that iterates a string-keyed table twice with different seeds.
- Write the report to `docs/research/order-leaks.md`. A fix, if needed, becomes its own build ticket.

## Resolution

<Filled in at close.>
