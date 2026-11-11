---
id: 106
title: Audit the remaining order leaks
type: task
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Which uses of `into_iter`, `extend`, `from_iter` and Lua `pairs` over unordered containers can change what a Seat sees or what a replay plays out? Ticket 075's lint refuses `for` loops and walking methods over hash containers, but accepts these four.

## Notes

- Origin: the Proof game entry in `99-backlog.md`, 075's Resolution, and the Host Session note that Lua `pairs` order over string keys is not seeded.
- Report each site with the container type, whether its order reaches state, a Decision candidate list, the ledger or the replay, and a fix (ordered container, sort, or none needed). Do not change code.
- Test the `pairs` question by running one script that iterates a string-keyed table twice with different seeds.
- Write the report to `docs/research/order-leaks.md`. A fix, if needed, becomes its own build ticket.

## Resolution

Report in `docs/research/order-leaks.md`. No hash container in non-test code leaks its order. Each is read by key only, except the loop fingerprint, which sorts hash collections before comparing. No `into_iter`, `extend`, `from_iter` or set algebra call takes a hash container.

Lua `pairs` order over string keys changes between Lua states, across processes and sometimes within one, and the game seed never reaches it. Three key checks (`reject_unknown_keys`, `decode_entry_options` and the `Game.ChooseN` opts check) name the first bad key that `pairs` meets. A table with two or more bad keys therefore gets a different `ScriptError` message, which every Seat sees. State, Decisions and Answers are unchanged, and a replay plays out the same. The one card script that uses `pairs` (Balance) walks integer player keys and folds them to minimums, so its order does not matter.

The fix is build ticket 149: sort the key errors and add the hash set algebra and `extract_if` methods to `clippy.toml`. Replacing `pairs` in the sandbox was ruled out. Decided in auto mode, then merged.
