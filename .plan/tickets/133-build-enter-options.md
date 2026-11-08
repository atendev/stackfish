---
id: 133
title: Build enter options
type: task
mode: HITL
status: open
blocked_by: [130]
claimed: false
---

## Question

Build `EnterOptions` on `ReturnToBattlefield`, `ReturnAllToBattlefield` and `ScriptedZoneMoveKind::Reanimate`, as "Reanimation options" decided.

## Notes

- Read `closed/100-reanimation-options.md`.
- `control`, `tapped` and `exile_if_leaves` move into the struct, and `decode_entry_options` fills it. "Build die and leave riders" runs first so `exile_if_leaves` is decoded before it moves.
- `ScriptedZoneMoveKind` loses `Copy`. List the copy sites in `replacement_resume.rs` and `choice_answers.rs` in the requirements report.
- No new behaviour, so the replay pin must not move.

## Resolution

<Filled in at close.>
