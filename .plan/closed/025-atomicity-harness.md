---
id: 025
title: Atomicity harness and audit
type: task
mode: HITL
status: closed
blocked_by: [024]
claimed: false
---

## Question

Prove that a rejected input changes nothing, as 024 decided: build a harness that feeds illegal inputs and checks state and ledger are unchanged, audit every `execute_command` branch and `submit_*` method for mutate-then-`Err`, and fix what the audit finds.

## Notes

- Origin 001 and 010. Decided in 024.
- This file was written after the merge, from the commits, because the ticket was tracked only inline on `map-fixes.md`.

## Resolution

`Clone`, `PartialEq` and `Debug` are derived across `types/`, and `Engine::snapshot()` captures the state, the command counter, the Lua handle count and the pipeline and payment fields. `stackfish-testutil` has `assert_unchanged_after`, `assert_action_rejected` and `snapshot_diff`. `Bot::injecting_illegal_answers` sends illegal Answers at every Decision of a proof game and checks each is refused with the engine unchanged.

The audit found and fixed five bugs, each with a regression test. PlayLand and CastSpell changed state before a zone lookup could fail. A free mana ability tapped its source before a player lookup could fail. DeclareAttackers accepted one creature twice. A rejected action used up a command id. `submit_action` suspended a pending payment before its last rejection checks. The same branch sorted five unordered state-based action collectors in `sba.rs` (part of 028).

Built in `a45ccc7` and `0df02f7` on `phase5/atomicity-harness`, merged as `58fb861`. The audit tables are in `docs/implementation/21-atomicity.md`.
