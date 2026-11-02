---
id: 076
title: Timing permissions granted by effects
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How should an effect grant permission to cast a spell or activate an ability at a time it otherwise could not ("as though it had flash", CR 601.3b-d), so that the cast command and `Engine::legal_actions` read one rule?

## Notes

- Origin 031. That ticket closed the other `legal_actions` gaps and moved this one out because it needs a new rules primitive and Lua API, not only an entry.
- Today only the card's own `Flash` keyword counts: `commands/cast.rs` (TODO near the timing check) and `commands/entries.rs` (`cast_entries`).
- Options to weigh: a permission effect on `state.continuous_effects` ("you may cast X as though it had flash"), or granting `FLASH` through the layer system to cards outside the battlefield. CR 601.3 treats the permission as separate from the keyword.
- Zone permissions ("play lands from your graveyard", CR 601.3) share the shape. Decide whether they are in scope.
- The TODO at `commands/legality.rs:30` cites CR 113.6, which is about where abilities function. Fix the citation when this is built.
- A Lua builder means updating the game stub in the docs (`docs/lua/`).
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close: the decision and the reason for it.>
