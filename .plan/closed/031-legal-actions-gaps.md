---
id: 031
title: Remaining legal_actions gaps
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Close the gaps in what `Engine::legal_actions` offers.

## Notes

- Origin 010 and 011.
- Target candidates ignore X.
- `DeclareBands` is not offered.
- Timing grants from continuous effects are missing.
- Nested mana abilities during a Payment Decision.
- `Target::Ability` candidates for script picks.

## Resolution

Built on `phase5/legal-actions-gaps`. Four of the five gaps are closed, and timing grants moved to a ticket of their own.

- X: already merged on `phase5/x-aware-target-candidates`. Entries list every candidate legal for some X.
- `DeclareBands`: `open_decision` offers it after attackers are declared, when `legal_bands()` lists an attacker that forms a legal pair. An empty reply declares none. `CombatState::bands_declared` closes it, and a second declaration is refused (CR 702.22e). The Decision opens after attack triggers are on the stack (TODO, CR 508.1e).
- Nested mana abilities: a Payment Decision lists the payer's mana abilities while they hold priority. An `Action` reply activates one, and the payment opens again under a fresh id (CR 605.3a). An unless-paid payment lists none (TODO).
- `Target::Ability`: a script `SelectTarget` pick lists the abilities on the stack that its filter accepts (CR 113.9).
- Timing grants ("as though it had flash", CR 601.3b-d) need a rules primitive and a Lua builder in the cast command, not only an entry. They moved to ticket 076, Timing permissions granted by effects.

See `docs/implementation/17-legal-actions.md`.
