---
id: 80
title: Zone permissions (play or cast from another zone)
type: grilling
mode: HITL
status: open
blocked_by: [79]
claimed: false
---

## Question

How does an effect let a player play a land or cast a spell from a zone other than hand, such as "play lands from your graveyard", so the commands and `legal_actions` read one rule? Decide whether it extends `TimingPermission` or sits beside it.

## Notes

- Origin 076. It touches `required_zone` in `commands/cast.rs`, `validate_play_land` and the entry scans in `entries.rs`, plus the CR 601.3 and 305.1 rules after the cast.
- `LandPlayModifier`'s `SetFunctionsFrom` names where the source ability functions, not where the played card comes from, so it is not the precedent.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close: the decision and the reason for it.>
