---
id: 128
title: Flash bought with an extra cost
type: grilling
mode: HITL
status: closed
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

Decided in auto mode over two grilling rounds, then approved by the user. A grilling subagent answered the questions and accepted every recommendation, and its notes are folded in below.

**Cards.** 10 printed cards buy flash with a cost (CR 601.3c). Nine pay "{2} more" (Asinine Antics, Breaking Wave, Ghitu Fire, Harbinger of the Tides, Mystical Tether, Oakshade Stalker, Rout, Saproling Symbiosis, Twilight's Call). Tegwyll's Scouring taps three untapped creatures with flying, which `TapPermanents` already expresses. Molten Exhale beholds a Dragon and waits on the behold fog item. The database has no rulings for any of them. Offering (CR 702.48a, 6 cards such as Blast-Furnace Hellkite) also buys flash, but its cost reduction comes from the sacrificed object, so it goes to Not yet specified.

**A cost on the cast, not a Timing permission.** A CR 601.3d condition reads state that exists before the cast, so a Timing permission holds it. A CR 601.3c cost is a choice the player makes inside the proposal (CR 601.2b), and "Timing permission follow-ups" left cast-choice-dependent flash as fog for that reason. So the flash cost is a per-card optional additional cost in buyback's shape. An alternative cost was ruled out, because it replaces the mana cost (CR 118.9) and this one adds to it.

**Types.** `FlashCost { mana: Option<ManaCost>, additional: Vec<AdditionalCost> }` in `types/cost.rs`, held as `CardDefinition.flash_cost: Option<FlashCost>` and announced by `CastSpell.pay_flash_cost: bool`. A `FlashCost` with no mana and no additional cost is invalid, because it would buy flash for nothing. `CostSpec` was not reused, since its `alternative` list would always be empty.

**Timing.** `can_cast_now` does not change. The player may announce the flash cost on any cast. When the actor holds priority and `can_cast_now` is false, the cast is legal only with the cost announced. Flash relaxes timing, never priority. A timing-bypassed cast never requires the cost. One shared helper computes this for the cast command and `cast_entries`, so the two cannot drift. Timing is checked once, at announcement, and an unpayable total unwinds through the existing "cannot pay" Answer (CR 601.2h, 601.6). Nothing records the payment on the stack, and the spell never gains the Flash keyword.

**Cost merge.** The cost goes on top of whatever cost applies: the mana cost, an alternative cost such as flashback, or a free cast (CR 601.2f, 118.9d). X is resolved first, the mana part is appended after cost modifiers and commander tax as buyback's is, and `additional` joins the mode costs in `cost.additional`. A TODO notes that CR 601.2f lets a reduction apply to the appended mana.

**Entry and wire.** `CastEntry.flash_cost: FlashCostOffer { None, Optional, Required }`. `Required` means the actor holds priority, the card fails `can_cast_now` and the flash cost alone lists it. A card is omitted when the actor lacks priority. The existing zone and cast-legality checks in `cast_entries` still run first, and the flash cost changes only the timing check. `cast_options` offers `Optional` at most. On the wire the offer is a snake_case string defaulting to `none`, and `pay_flash_cost` defaults to false. Both are additive, so `PROTOCOL_VERSION` stays 1.

**Lua.** `s.flash_cost()` returns a list of `Game.Costs.*` values, decoded by `decode_cost_component_value` as an alternative cost's `costs` list is, with `Game.Costs.Mana` at most once. Rout returns `{ Game.Costs.Mana("{2}") }`. A missing function means no flash cost, and an empty list is a load error. `card-script-basics.md` gains the function beside `buyback_cost()`, and `docs/lua/stubs/game.lua` gains its field.

**Bot and tests.** The bot announces the cost on `Required` and never on `Optional`, so seeded games and the replay pin do not move, and no proof card is added. Tests cover `cast_entries` for the three offers and for no priority, the cast command at sorcery and instant timing with and without the cost, no priority with the cost, Ghitu Fire with X, a flashback cast with the cost, the unpayable unwind, a Lua round trip with the empty-list error and Tegwyll's `TapPermanents`, a wire round trip, and a scenario that casts Breaking Wave on an opponent's turn.

**Glossary.** CONTEXT.md gains **Flash cost**: an optional additional cost printed on a card that lets that card be cast any time its controller could cast an instant, for the one cast that pays it (CR 601.3c). It is not a Timing permission. _Avoid_: flash grant, flash tax, extra cost. The Timing permission and Additional cost entries point to it.

**Relation to other tickets.** "Optional additional costs and kicker" decides whether buyback and the flash cost fold into one optional-cost list, and a TODO points there. The build "`new_game` loads alternative costs and buyback" also loads `flash_cost()`.

**Build.** One build ticket, "Build flash costs", with no blocker, since the `cost.additional` merge exists for mode costs. Its TODOs are behold for Molten Exhale and the CR 601.2f reduction on the appended mana.
