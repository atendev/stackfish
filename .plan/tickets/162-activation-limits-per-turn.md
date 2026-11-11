---
id: 162
title: Activation limits per turn
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How does an activated ability declare "Activate only once each turn" and its combined forms, and where are spent uses recorded?

## Notes

- Origin: "Activate only as a sorcery on any ability". 108 printed cards say "once each turn", plus "as a sorcery and only once each turn" (8), "during your turn and only once each turn" (5), "during your upkeep and only once each turn" (3), "once" (16) and "as a sorcery and only once" (4).
- The limit is a CR 602.5b restriction that stays with the object across a control change. A new object is a new ability (CR 400.7).
- Weigh a field on `ActivationRestriction` and spent uses keyed by ability, like `GameState.zone_permission_uses` from 097, cleared each turn. Also weigh "Activate only once" with no turn reset.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
