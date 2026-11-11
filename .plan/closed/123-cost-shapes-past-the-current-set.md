---
id: 123
title: Cost shapes past the current set
type: grilling
mode: HITL
status: closed
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

Decided in an auto-mode grilling run over two rounds. A subagent stood in for the user and took every recommendation, with notes for the builds that are folded in below.

**Recount.** `../stackfish-db/stackfish.db` has 38,344 rows now. Activation costs, by the text before the colon, in distinct cards: remove counters 335, exile this card from your graveyard 145, other exile shapes 221, tap other untapped permanents 172 (already `TapPermanents`), energy 55, return to hand 49, reveal 42, untap 23, put a counter 15, exert 8, mill 8, blight 7, collect evidence 6, put on library 5, forage 2. Spells with "As an additional cost to cast this spell": 346. Of those, sacrifice 138, pay life 63, discard 47, exile 36, tap 27, reveal 23, behold 19, collect evidence 9, blight 7. About 68 are "A or B" choices, 36 use X and 84 are optional. The classifier is a regex, so the build tickets recount the shape they build.

**Spell additional costs come first.** Spells have no place for a printed additional cost today. Only a mode's cost (CR 700.2h) and buyback reach `CostSpec.additional`. A new `CardDefinition.additional_costs: Vec<AdditionalCost>`, declared by a script function `s.additional_costs()` beside `buyback_cost()`, is appended in `commands/cast.rs` with the mode costs (CR 118.8, 601.2f). Modelling each one as a one-mode spell was ruled out, since it misuses CR 700.2.

**Self or chosen.** New variants take `subject: CostSubject { Source, Chosen { filter, count } }`. The existing paired variants (`TapSelf` and `TapPermanents`, `SacrificeSelf` and `Sacrifice`, `DiscardSelf` and `Discard`) stay as they are, so no wire or script name moves. Moving them onto `CostSubject` is fog.

**Shapes in scope.**
- `RemoveCounters { subject, kind: Option<CounterKind>, amount }`, where `None` means a counter of any kind. It is paid with one `CostPayment::RemoveCounter(ObjectId, CounterKind)` per counter. Candidates are objects with at least one matching counter. Validation refuses more counters of a kind than the object has.
- `PayEnergy(n)` with `CostPayment::Energy(n)`, paid from the player's `CounterKind::Energy` counters (CR 107.14), in the `LifePayment` shape.
- `Exile { from: Hand | Graveyard | Battlefield, subject }`, paid with the existing `CostPayment::Exile(id)`. `ExileFromGraveyard` stays for Escape's "other cards". An ability with `Exile { from: Graveyard, subject: Source }` can be activated only from the graveyard (CR 113.6m), through the same check in `commands/activate.rs` and `commands/entries.rs` that lets `DiscardSelf` activate from hand.
- `ReturnToHand { subject }` and `PutOnLibrary { subject, top }` for permanents the payer controls, paid with `CostPayment::Return(id)` and `CostPayment::PutOnLibrary(id)`.
- `Reveal { subject, count }` from hand, paid with `CostPayment::Reveal(id)`. The card stays in hand, and the existing `Revealed` event reaches the Seats.
- `PutCounter { kind, amount }` on the source only.
- `UntapSelf` for `{Q}`. The source must be tapped (CR 107.6), and a creature's ability needs CR 302.6, as `{T}` does.
- `Mill(amount)`, with no candidates. It moves cards from the library to a public zone, so commit pays it after every other cost (CR 601.2h).

**One object, one moving cost.** `PaymentPlan::validate` lets one object pay two additional costs today, so the same card can be discarded and revealed, or the same permanent returned and sacrificed. Validation refuses an object id used by two moving costs (sacrifice, discard, exile, return, put on library). Tap, untap, reveal, remove counter and put counter may share an object with each other and with one moving cost. CR 601.2h lets the other costs be paid in any order, so commit chooses to pay the costs that don't move objects first, and "{T}, Sacrifice this" stays legal.

**A or B.** `AdditionalCost::OneOf(Vec<AdditionalCost>)`. The player announces a branch index with `CastSpell` or `ActivateAbility`, and the Entry lists the branches the way it lists alternative costs. The cost is narrowed to that branch before the Payment Decision opens, so the Payment Decision keeps one fixed shape (CR 601.2b, 601.2f). It does not share the alternative-cost path, because an alternative cost replaces the mana cost (CR 118.9) and these add to it.

**X counts.** Counted shapes take `CostAmount { Fixed(u32), X }`. X is the one value announced with the spell or ability (CR 107.3a, 601.2b, 602.2b), and the Entry's X flag is set when the mana cost or any additional cost uses X. Lua refuses `"X"` on a shape with no amount, and refuses it everywhere until the X build lands. CR 107.3f covers X outside every cost and does not apply.

**Wire.** Every new variant and the branch index are additive wire arms and fields, and `PROTOCOL_VERSION` stays 1, as with the stun counter arm in 102.

**Lua.** `Game.Costs.RemoveCounters(kind|nil, amount, filter?)`, `PayEnergy(n)`, `Exile(zone, filter?, count?)`, `ReturnToHand(filter?)`, `PutOnLibrary(top, filter?)`, `Reveal(filter, count)`, `PutCounter(kind, n)`, `UntapSelf()`, `Mill(n)` and `OneOf({...}, {...})`. A missing filter means the source. Spells return a list of these from `s.additional_costs()`. Every build except "One object pays one moving cost" changes the Lua API, so it updates `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`.

**Out of this ticket.**
- Optional additional costs and kicker need a per-cast fact that the effect reads (CR 118.8b, 702.33a), so they go to the new grilling ticket "Optional additional costs and kicker".
- Set-level constraints (Illuminated Folio's shared color, collect evidence's total mana value, CR 701.59a) are fog. Per-candidate lists stay the whole Payment Decision contract.
- Exert waits on the "doesn't untap" fog item (CR 701.43a). Behold, blight and forage are keyword actions with few cards and are fog.

**Build order.**
1. "Build spell additional costs".
2. "One object pays one moving cost".
3. "Build counter and untap costs" (`CostSubject`, `RemoveCounters`, `PutCounter`, `UntapSelf`).
4. "Build energy costs".
5. "Build zone-moving costs" (`Exile` with graveyard activation, `ReturnToHand`, `PutOnLibrary`, `Mill`).
6. "Build reveal costs".
7. "Build A-or-B cost choices".
8. "Build X-tied cost counts".

Step 1 gives spells a place for any shape, and step 7 depends on it. Step 2 gates the payment shapes in steps 3 to 6, which are tested through activated abilities and so do not wait on step 1. Steps 7 and 8 come last because they change what the player announces, not the payment.
