---
id: 070
title: Report scenario failures per file instead of stopping at the first
type: task
mode: HITL
status: open
blocked_by: [060]
claimed: false
---

## Question

Change `crates/engine/tests/scenarios.rs` so a run reports every failing scenario file and which assertion failed in each, instead of panicking on the first file with only its path.

## Notes

- Optional. Found by 060. Only worth doing if 071 goes ahead or the scenario count grows.
- Options include collecting all reports before one assert, or one generated `#[test]` per file.

## Resolution

<Filled in at close: the decision and the reason for it.>
