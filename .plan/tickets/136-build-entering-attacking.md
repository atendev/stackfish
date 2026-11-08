---
id: 136
title: Build entering attacking
type: task
mode: HITL
status: open
blocked_by: [135]
claimed: false
---

## Question

Build the `attacking` option and `EnteredAttacking` event, as "Reanimation options" decided.

## Notes

- Read `closed/100-reanimation-options.md`.
- The creature joins `combat.attackers`. No `AttackerDeclared` and no attack triggers (CR 508.4). `EnteredAttacking` joins the public list in `crates/host/src/view.rs`.
- Not attacking in the cases CR 506.3a, 506.3b, 506.3c, 506.3f and 508.4a name, and while `state.combat` is `None`.
- A defender Decision opens only with two or more legal defenders, through `AwaitingEnterChoice`. CR 508.4c ignores attack requirements and restrictions.
- Keep `// TODO: attacking entry before attackers are declared (CR 508.4)` and `// TODO: entering-attacking creatures joining a band (CR 702.22)`.

## Resolution

<Filled in at close.>
