# Cost modifiers

[← index](index.md)

CR 601.2f — "Spells you cast cost {N} more/less to cast," or similar. Built with
`Game.CreateCostModifier(c)`. The smallest builder in this API — just a filter and an amount.

```lua
-- Spells you cast cost {1} less to cast.
local s = {}

function s.register_abilities(c)
    local e = Game.CreateCostModifier(c)
    e:SetFilter(Game.Filter({ controller = "you" }))
    e:SetAmount(-1)
    e:Register()
end

return s
```

## Methods

**`:SetFunctionsFrom(zoneName)`** — CR 113.6, for an effect that functions from a zone other than the
battlefield (Anger: "as long as this card is in your graveyard..."). `zoneName` is `"Hand"`,
`"Graveyard"` or `"Exile"`. The effect exists only while its source is in that zone and is removed when the card leaves
it; without this call it only functions on the battlefield. See
[triggered-abilities.md](triggered-abilities.md#methods) for the shared behavior.

**`:SetFilter(value)`** — which spells this modifier applies to. `value` is a
[`Game.Filter({...})`](static-abilities.md#gamefilter) value over the shared filter keys — see
[targeting-and-choices.md#filter-table-shape](targeting-and-choices.md#filter-table-shape). A raw
table is a load error. Required.

**`:SetAmount(amount)`** — the flat change to the spell's generic mana component. A negative
number reduces the cost, a positive number increases it. Required.

**`:Register()`** — finalizes and queues the modifier.

## How multiple modifiers combine

If more than one registered modifier matches the same spell, every matching modifier's amount is
summed into a single net change before it's applied — the order the modifiers were registered in
never affects the result. A cost can never go below `{0}` in its generic component.

## Scope

Only applies to the generic-mana component of a spell's own printed mana cost, at the point it's
being cast with its normal cost (not an alternative cost like flashback). There's no colored-pip-
specific reduction yet (CR 118.7's "costs {1} less to cast, this reduction can't reduce the amount
of colored mana you pay" kind of nuance) — every reduction/increase here is generic-only.
