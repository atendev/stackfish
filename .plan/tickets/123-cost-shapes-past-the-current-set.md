---
id: 123
title: Cost shapes past the current set
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Which structured cost shapes does the engine add beyond today's `AdditionalCost` variants, and in what order are they built?

## Notes

- Origin: the Resolution of [closed/096-script-backed-costs.md](../closed/096-script-backed-costs.md). Costs stay structured data, with no Lua callback.
- Activation costs with no `Game.Costs.*` form, counted from oracle text in `../stackfish-db/stackfish.db` (34,738 cards): remove counters 334 cards, exile this card from your graveyard 77, other exile shapes about 190, energy 53, return to hand 44, reveal 27, put a counter 11, put on library or other zones 11, exert 8, mill 7, untap 3. Recount before relying on these numbers, because the query classified the text before each colon.
- Spell additional costs: 353 spells, of which 86 are "A or B" choices (for example, "discard a card or pay 3 life"). Others use reveal, behold, collect evidence and blight.
- Decide how a choice between costs is announced and paid (CR 601.2b, 601.2h), and whether it shares the alternative-cost path.
- Decide how a count tied to X works ("remove X counters", "sacrifice X creatures").
- Decide whether a set-level constraint is worth having. Illuminated Folio's "reveal two cards that share a color" is a relation among the chosen objects, which a per-candidate filter cannot express.
- Each shape needs a Payment Decision candidate list through `AdditionalCost::accepts` (030) and validation before anything moves (024).
- Per-shape build tickets come out of this one.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
