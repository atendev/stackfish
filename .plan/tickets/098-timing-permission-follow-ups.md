---
id: 98
title: Timing permission follow-ups
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Which of the timing permission gaps get built next? Conditional flash, "as though it were a main phase" and instant-speed equip are not built, and `Permanent`, `WhileConditionHolds` and "until the source leaves" durations are refused for a grant.

## Notes

- Origin: the Timing permissions entry in `99-backlog.md` and 076's Resolution. Land plays and Equip still use `is_sorcery_speed`.
- Decide the order and the shape of each. A condition on a permission needs the same evaluation point as `can_cast_now`. Instant-speed equip touches the activation path, not the cast path.
- CR 611.2b is why the refused durations are refused. Decide whether any real card needs them.
- Read `docs/implementation/35-timing-permissions.md` and `37-expiring-permissions.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
