---
id: 064
title: Narrow pub visibility and remove dead helpers
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Should `EngineCommand`, `CommandOutcome` and `ProcessState` stay nameable outside `stackfish-engine`, and once that is settled, which of the ~424 `pub` items and 15 dead or test-only helpers listed in 055 get narrowed or removed?

## Notes

- Split from 057. The visibility half of the original 057 needs the nameability decision first, as 055 recorded.
- 055 counts: about 424 items and fields can narrow to `pub(crate)` (engine 85, scenario 137, host 187), 9 items are dead (engine 5 plus the `invoke_cost` stub, host 3), and 6 more are test-only (engine 5, host 1).
- After the decision, the narrowing and removal is a build slice for a new ticket.

## Resolution

<Filled in at close.>
