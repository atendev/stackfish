---
id: 176
title: Triggered mana abilities
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How should triggered mana abilities (CR 605.1b, 605.4a) work: abilities that trigger from a mana ability, add mana, and resolve at once without the stack, such as Mana Reflection or Wild Growth?

## Notes

- Found by 174. Today every trigger goes on the stack.
- Questions to settle: where the trigger resolves inside a mana activation or a payment, what the Payment Decision shows, and what the Lua builder looks like.

## Resolution

Decided with the user over three grilling rounds and approved by the auditor. Full record: [assets/176-triggered-mana-abilities-report.md](../assets/176-triggered-mana-abilities-report.md).

- Scope covers CR 605.1b triggers only. Mana Reflection, Contamination and Pulse of Llanowar are replacement effects (CR 106.12b) and become a fog item.
- A new `ManaAbilityResolved { source, ability, player, units, tapped }` event is emitted beside `ManaAdded`. `tapped` means the activation tapped its own source as a cost (CR 106.12a).
- Triggered mana abilities split off at the trigger collect stage and resolve at once, in registration order, at priority or mid-payment (CR 605.4a). A TODO covers CR 603.3b ordering.
- Lua gets `Game.CreateTriggeredManaAbility(c)` (no target, mana operation only, load error otherwise) and `Game.Conditions.TappedForMana({ object, controller = "Any", produced })`, which ordinary triggers may also use (CR 605.5a). `Game.Ops.AddManaOfProducedType(n)` is added too. The mana goes to the player who tapped.
- A new `ChooseManaColors` Decision picks colors, with `PROTOCOL_VERSION` still 1. The Payment Decision shows nothing new. The bot picks a color the payment still needs.
- Surfaced: Build triggered mana abilities, and Hold triggers fired during a payment until the cast finishes. A probe showed an ordinary trigger is pushed above a half-cast spell (CR 601.2i, 603.3).
