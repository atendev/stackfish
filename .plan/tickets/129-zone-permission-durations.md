---
id: 129
title: Zone permission durations
type: grilling
mode: HITL
status: open
blocked_by: [117]
claimed: false
---

## Question

How does a resolved Zone permission last "for as long as it remains exiled" and "for as long as you control [this permanent]"?

## Notes

- Origin: "Timing permission follow-ups". 67 printed cards read "for as long as it remains exiled" and 8 read "for as long as you control" or "remains on the battlefield", by a query of `../stackfish-db/stackfish.db`.
- A `Permanent`-duration linked row does not work for the exile case. The card's id changes when it leaves exile (CR 400.7), so the row would outlive the link (CR 607.2a). Build on `ExileLink` from the effect-owned exile links build.
- "For as long as you control" needs a controller check beside `WhileSourceOnBattlefield`. CR 611.2b covers a duration that never starts.
- Decide whether one ticket builds both, and the citation for each. `timed_grant_duration` refuses these durations today.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
