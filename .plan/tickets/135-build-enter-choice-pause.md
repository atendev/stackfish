---
id: 135
title: Build the enter-choice pause
type: task
mode: HITL
status: open
blocked_by: [133]
claimed: false
---

## Question

Build `ProcessState::AwaitingEnterChoice` and the `batch` list, so a reanimated Clone, Devour, Amplify or Aura creature makes its choices, as "Reanimation options" decided.

## Notes

- Read `closed/100-reanimation-options.md`.
- The existing stack and search pause states stay. The answer methods share an origin of either a `stack_id` or a move's resume data, and hard-coded `ZoneKind::Stack` becomes the origin's `from`.
- The pause must work with `then` being `SbaLoop`, for `return_linked_exiles` (CR 610.3).
- The `batch` list is set once in `continue_scripted_zone_move`, and candidates exclude objects in the batch (CR 603.6a, 614.13a).
- Aura host is chosen before placement (CR 303.4f) and reuses the "Attach on entry" Decision shape.
- Removes the TODOs at the top of `commit_reanimate` and the Aura one inside it. A test with a reanimated Clone with an enters trigger pins that no trigger fires during the pause.

## Resolution

<Filled in at close.>
