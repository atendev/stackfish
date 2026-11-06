---
id: 93
title: Scenario battlefield setup runs card scripts
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Should a card placed on the battlefield by a scenario fixture run its script, so its static and triggered abilities register? Today it does not, and fixtures that need a static ability cast the card from hand first.

## Notes

- Origin: `map.md` Not yet specified (found by 088). The Kess scenario works around it by casting Kess.
- Decide whether setup always runs `register_abilities`, runs it only when the card spec asks, or stays as it is. Check how many existing scenarios would change behaviour or replay pins.
- Read `docs/scenario_format.md` and `crates/scenario/src/build.rs` (six TODOs).
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
