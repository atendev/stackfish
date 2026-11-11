---
id: 159
title: Optional additional costs and kicker
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does the engine announce an optional additional cost or kicker, record that it was paid, and let the effect read that fact (CR 118.8b, 702.33)?

## Notes

- Origin 123. 84 spells have an optional "you may" additional cost and about 261 cards mention kicker.
- Decide the per-cast fact on `StackObject`, how a Lua effect reads it, multikicker, and whether buyback moves onto the same path.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run over three rounds. A sonnet subagent stood in for the user and answered each question from the code, the comp rules and the docs. A haiku auditor approved the question report and this resolution. The user approved it.

**Counts.** From `oracle_text` in `../stackfish-db/stackfish.db`, by regex: 238 cards have Kicker, 18 of them non-mana and 18 with two kicker costs. 19 have Multikicker, and none has a non-mana cost. 13 say "kicked with its [A] kicker". 15 count "each time it was kicked". All 15 are permanents: 8 enter with counters per kick, 6 have a scaling ETB trigger, and Urborg Lhurgoyf mills as it enters. 71 have "enters, if it was kicked". 64 have a target that exists only if kicked. 84 spells have a "you may" additional cost. 41 have Buyback.

**One list on the card.** `CardDefinition.optional_costs: Vec<OptionalCost>`, with `OptionalCost { kind: OptionalCostKind, mana: Option<ManaCost>, additional: Vec<AdditionalCost>, repeatable: bool }` and `OptionalCostKind { Kicker, Optional, Buyback }`. "Kicker A and/or B" is two entries (CR 702.33b). Multikicker is one Kicker entry with `repeatable` (CR 702.33c). The loader rejects `repeatable` on an entry with `additional` costs, since no multikicker card has one, and keeps a TODO for CR 702.33c. The loader also rejects two Buyback entries. The 128 `FlashCost` stays separate, because it changes timing.

**Announcement.** `CastSpell.optional_costs: Vec<u32>` is a times-paid count per index into the card's list, announced with modes and X (CR 601.2b). Empty means none. The cast rejects an index out of range, or a count above 1 on a non-repeatable entry. A count has no maximum, as with X, and an unpayable count fails at payment (decision 003). The Entry gains `optional_costs: Vec<OptionalCostOffer { index, repeatable }>`.

**Cost.** Optional costs join `CostSpec` in `commands/cast.rs` after mode costs and before modifiers and tax (CR 601.2f). Decision 128 adds the flash cost after modifiers and tax, so the order is mode costs, optional costs, modifiers and tax, then the flash cost, whichever build lands first. The optional mana is concatenated once per count onto the resolved mana cost before `cost::apply_cost_modifiers` runs, so a reduction can reduce a kicker's generic mana. Buyback's mana is added after modifiers and tax today, which CR 601.2f does not allow, so folding buyback into the list moves it to the same place. Build 1 adds a test that a cost reduction lowers a buyback or kicker cost. Modifiers still skip an alternative-cost path, as they do today. `additional` costs are appended and go through the 123 payment-choice path, so a "you may sacrifice a creature" still picks the creature at payment. The mana cost and mana value do not change (CR 118.8d).

**The stack fact.** `StackObject.optional_paid: Vec<u32>` holds the announced counts. Helpers: `kick_count()` sums the Kicker entries, and `paid(index)` reads one entry, which serves "kicked with its A kicker" (CR 702.33f) and "if the additional cost was paid". `bought_back` becomes a query on the Buyback entry. A triggered or activated ability copies its source permanent's `GameObject.optional_paid` when it is put on the stack, or takes it from `triggers::find_departed_lki` if the source has left, as `source_lki` does. A delayed trigger gets none. A spell copy keeps the counts (CR 707.2, 707.10), and that is a TODO until spell copying exists.

**The permanent.** On battlefield entry, `optional_paid` moves from the stack object to `GameObject.optional_paid`, as `escaped` moves to `entered_via_escape`. It resets on a zone change (CR 400.7). A copy of a permanent never gets it, since CR 707.2 copies the fact only for an object on the stack. `Game.Conditions.Kicked(min?, which?)` is an intervening-if (CR 603.4). `ReplacementPayload::EntersWithCountersKicked { counter, per_kick, which }` follows the Escape and Sunburst payloads (CR 614.1c) and joins the no-ordering-pause lists in `replacement.rs` and `script/builders.rs`. "As this enters, mill three for each time it was kicked" (Urborg Lhurgoyf) is a TODO, because no hook exists for an "as enters" action other than counters or tapped (CR 614.12).

**Lua.** A script declares `s.optional_costs()`, which returns a list built by `Game.OptionalCosts.Kicker(...)`, `Multikicker(...)` and `Optional(...)` from mana strings and `Game.Costs.*` values. `s.buyback_cost()` keeps working and loads into the same list. Operation functions get a fourth `cast` argument, `function(source, controller, state, cast)`, with `cast.kicked`, `cast.paid` (a 1-based array) and `cast.bought_back`. `cast` is nil when nothing was recorded. Lua ignores an argument a function does not declare, so existing scripts keep working. `Game.Ops.IfPaid(op_if, op_else?, which?)` is a primitive branch, because pump effects are reachable only as primitives. `which` is a 1-based index, and nil means any optional cost was paid. Scaled amounts use the `cast` count in a custom function: `GainLife` (`i32`), `Mill`, `DrawCards` and `DealDamageTo` (`u32`), `CreateToken` (`Option<u32>`) and `Game.ChooseN` (a Lua number `n`) all take a plain number in `script/globals.rs`, and each must treat a computed 0 as doing nothing. The builds update `card-script-basics.md`, `game-constants.md`, `not-available-yet.md` and `docs/lua/stubs/game.lua`.

**Wire.** Every change is additive and defaulted, and `PROTOCOL_VERSION` stays 1. `WireAction::CastSpell` gains `optional_costs`, and the Entry gains the offers. The public stack object gains its paid counts. The wire keeps `pay_buyback` and the Entry's `buyback`. The host facade maps `pay_buyback` onto the buyback index and rejects a cast that sets both, with the existing illegal-action code (decision 001). Both legacy fields get a TODO for removal at the next protocol bump.

**Kicker-only targets and modes.** A target slot gets `requires_paid: Option<usize>`, and a modal card gets `paid_mode_range: Option<(usize, ModeRange)>` (CR 702.33g, 601.2c). This covers Inscription of Abundance and Pyrrhic Strike's blight. The Entry lists every slot with its tag, and one priority Answer carries the counts and the targets, as with modes and X (decision 003). The cast rejects a target for an unpaid slot and a missing target for a paid one. Comet Storm's "another target for each time it was kicked" is a TODO.

**Out of this ticket.** Chosen-count optional costs ("sacrifice one or more", "tap any number"), where the effect reads how many were paid, go to Not yet specified. So does a computed amount for `EntersWithCounters` in general. Entwine, gift, bargain, replicate, offspring, casualty, squad, conspire and sticker kicker are separate keywords that may reuse this path later.

**Builds.**
1. "Build optional costs and kicker": the card list, the announcement and Entry offers, the cost join, `StackObject.optional_paid`, buyback folded in, the `cast` argument for spells, `Game.Ops.IfPaid`, the wire, Lua docs and the stub. Blocked by "Build spell additional costs".
2. "Build kicked permanents": `GameObject.optional_paid`, ability capture at push, `Game.Conditions.Kicked`, `EntersWithCountersKicked`, and a test that a token copy of a kicked creature is unkicked. Blocked by build 1. "Build enter counters and keywords" does not block it, since that build adds fixed counters to reanimation ops (`EnterOptions`) and this payload joins the `ReplacementPayload::EntersWithCounters*` family that exists today.
3. "Build kicker-dependent targets and modes". Blocked by build 1 and "Build Lua modal spells and triggers".
"Build flash costs" does not block, because the flash cost goes after modifiers and tax and optional costs go before them. Either build may land first.
