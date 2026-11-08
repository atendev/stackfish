---
id: 122
title: Delete invoke_cost and the unpayable Other cost
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Delete `ScriptRuntime::invoke_cost` and the four `Other` cost variants, as "Script-backed costs, or delete invoke_cost" decided. Costs stay structured data.

## Notes

- Origin: the Resolution of [closed/096-script-backed-costs.md](../closed/096-script-backed-costs.md), which lists every site by file and line.
- Remove `invoke_cost` (`crates/engine/src/script/runtime.rs`), `AdditionalCost::Other`, `CostPayment::Other`, `WireAdditionalCost::Other` and `WirePayment::Other`, with their match arms and the comments naming them. A grep for those names over `crates/` must come back empty.
- The TODO on `invoke_cost` goes with the function. The commit body says the feature it names is cancelled. `tools/comment_diff.py` does not apply, because this change deletes code.
- `PROTOCOL_VERSION` stays 1. Add a test that a payment with `"kind": "other"` gets `malformed_message` and changes nothing.
- Update `docs/lua/not-available-yet.md` lines 28 and 73, `docs/lua/game-constants.md` line 255 and item 11 of `docs/implementation/99-backlog.md`. State the current position with no history. `docs/lua/stubs/game.lua` needs no edit; confirm by grep.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
