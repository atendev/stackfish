---
id: 179
title: Build triggered mana abilities
type: task
mode: HITL
status: open
blocked_by: [176]
claimed: false
---

## Question

Build triggered mana abilities as "Triggered mana abilities" decided: `ManaAbilityResolved`, `Game.Conditions.TappedForMana`, `Game.CreateTriggeredManaAbility`, the collect-stage split, the `ChooseManaColors` Decision and `Game.Ops.AddManaOfProducedType` (CR 605.1b, 605.4a).

## Notes

- The full design is in `closed/176-triggered-mana-abilities.md` and `assets/176-triggered-mana-abilities-report.md`.
- Scope: fixed color, any color and produced-type triggers on the enchanted land, the source itself, or a filtered permanent. Snow tagging from the trigger's source. Wild Growth and Fertile Ground join the proof deck. Zhur-Taa Druid tests `TappedForMana` on an ordinary `Game.CreateTriggeredAbility`.
- TODOs: counted amounts (Elvish Guidance, Runaway Growth), "may" (Snowfall), "mana being added" triggers, Tyvar's resolves condition.
- The wire stays at `PROTOCOL_VERSION` 1. The bot picks a color the open payment still needs, else the first allowed color.
- Lua API change: update `docs/lua/stubs/game.lua` and the Lua docs. Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
