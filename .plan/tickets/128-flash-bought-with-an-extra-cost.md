---
id: 128
title: Flash bought with an extra cost
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How does a spell express "you may cast this spell as though it had flash if you pay {2} more to cast it" (CR 601.3c), when the extra cost applies only to a cast at a time the spell could not otherwise be cast?

## Notes

- Origin: "Timing permission follow-ups". 10 printed cards: Asinine Antics, Breaking Wave, Ghitu Fire, Harbinger of the Tides, Molten Exhale, Mystical Tether, Oakshade Stalker, Rout, Saproling Symbiosis and Twilight's Call.
- The `cast.rs:364` TODO says a non-modal `CardDefinition` has no additional-cost slot, so this needs that slot first.
- Overlaps "Cost shapes past the current set". Check that ticket's state before deciding.
- Molten Exhale pays by beholding a Dragon, so the extra cost is not always mana.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
