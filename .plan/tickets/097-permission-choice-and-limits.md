---
id: 97
title: Player choice between matching permissions and once-per-turn limits
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

When several zone permissions allow the same cast, who picks which one applies (CR 601.2), and where does a once-per-turn limit on a permission live?

## Notes

- Origin: the Zone permissions entry in `99-backlog.md`, the TODO at `commands/legality.rs:82`, and 088's Resolution. Today a rider-less permission wins, then list order, because permissions have no id. Kess, Dissident Mage's once-per-turn limit is not represented.
- Decide whether permissions get ids, whether the choice is a Decision before or inside the cast announcement, how `legal_actions` lists the options, and how a per-turn use count is stored and reset.
- Read `docs/implementation/36-zone-permissions.md` and `38-zone-permission-riders.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
