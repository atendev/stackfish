---
id: 91
title: Divide damage or counters among targets
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How does a spell or ability divide an amount among its targets as it is cast (CR 601.2d)? Today each legal target takes the full stated amount, so "deals 3 damage divided as you choose among any number of targets" cannot be played correctly.

## Notes

- Origin: backlog item 8. TODOs at `commands/cast.rs:338`, `commands/legality.rs:825`, `engine/resolve/mod.rs:289` and `tests/multi_target_resolution.rs:2`.
- Decide where the division is announced, what the Answer carries, how the entries and `legal_actions` describe the amount, what happens when a target becomes illegal (CR 601.2d keeps each share), and the Lua spelling.
- Combat damage assignment already has `AssignDamage` Decisions (`docs/implementation/17-legal-actions.md`). Decide whether to reuse that reply shape.
- Also decide whether counters ("distribute three +1/+1 counters") share the mechanism.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
