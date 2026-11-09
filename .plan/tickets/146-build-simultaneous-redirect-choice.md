---
id: 146
title: Build the simultaneous redirect choice
type: task
mode: HITL
status: open
blocked_by: [144]
claimed: false
---

## Question

Let the shielded player choose which simultaneous damage a limited redirect shield takes, as "Damage redirected to another target" decided.

## Notes

- Read `closed/103-damage-redirect-to-another-target.md`.
- A reading by analogy to CR 615.7, which is written for prevention. Say so in the code and the docs.
- The chooser is the shielded player or the watched permanent's controller. It applies to `NextEvent` and `Budget` shields when two or more events in `remaining` match.
- It needs look-ahead over `remaining` and a new pause with a Decision listing the applicable events. Exactly one Decision is open at a time, and a pending shield choice resumes through the pipeline.
- Closes `// TODO: the shielded player chooses among simultaneous damage events (CR 615.7)`.
- Tests: a double block with a deathtouch blocker, a choice that leaves the other event unchanged, a `Budget` spread over two events, and no Decision when only one event matches.

## Resolution

<Filled in at close.>
