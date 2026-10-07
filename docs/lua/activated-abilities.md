# Activated abilities

[← index](index.md)

CR 602 — "Cost: Effect." Built with `Game.CreateActivatedAbility(c)`.

```lua
-- {T}: Gain 1 life.
local s = {}

function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end

return s
```

## Methods

**`:AddCost(value)`** — called once per cost component (an activation can need more than one at
once, e.g. `{T}, Sacrifice a creature:`):

```lua
e:AddCost(Game.Costs.TapSelf)
e:AddCost(Game.Costs.Sacrifice({ card_types = { "Creature" } }))
```

`value` is a [`Game.Costs.*`](game-constants.md#gamecosts) value. There's no `:SetCost` — every
`:AddCost` appends. Omitting `:AddCost` entirely means no cost at all (not an empty, always-free
cost — that distinction matters if you're later checking whether the ability has a cost).

**`:SetOperation(value)`** — what activating the ability does. Same as
[triggered abilities](triggered-abilities.md#methods) — a
[`Game.Ops.*`](game-constants.md#gameops) tag or a custom function. Required.

**`:SetTarget(value)`** — gives the ability a real target. See [Targeting](#targeting) below.
Optional — omitting it keeps the ability targetless.

**`:SetFunctionsFrom(zoneName)`** — CR 113.6, for an ability that functions from a zone other than
the battlefield, e.g. "{2}{B}: Return this card from your graveyard to your hand." `zoneName` is
`"Graveyard"` or `"Exile"` (anything else is a script error — hand abilities like Cycling are
recognized by their `DiscardSelf` cost, and no other zone has a registration point yet). The ability
exists only while its source is in that zone: it registers when the card enters it, is dropped when
the card leaves, and can only be activated/triggers from there. Abilities that don't call this keep
the default — they never register for a card in the graveyard or exile, so an ETB trigger in the same
script stays dormant there. Statics, replacement effects and modifiers can't be declared this way yet.

**`:Register()`** — finalizes and queues the ability. Requires `:SetOperation`.

## Targeting

`value` is one of:

1. **A plain filter table** — the shared filter-table shape (see
   [targeting-and-choices.md#filter-table-shape](targeting-and-choices.md#filter-table-shape)),
   plus `type` (defaults to `"object"` if omitted):

   ```lua
   e:SetTarget({ type = "object", card_types = { "Creature" }, controller = "you" })
   ```

2. **A custom predicate function** — `function(object, source, controller, state) -> bool`, for
   legality a plain filter table can't express (e.g. "target creature with power 4 or greater").
3. **A player-target predicate** — `{ type = "player", filter = function(player, source,
   controller, state) -> bool }`, the player-candidate version of (2). A bare `{ type = "player" }`
   with no `filter` key still works as an ordinary unfiltered player target.

Always exactly one target — there's no "up to N" or multi-target shape on an activated ability's
`:SetTarget` yet.

## Worked example: Equip

CR 702.6a's own worked example — "`[Cost]`: Attach this permanent to target creature you control.
Activate only as a sorcery":

```lua
-- Equip {1}
local s = {}

function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.Mana("{1}"))
    e:SetTarget({ type = "object", card_types = { "Creature" }, controller = "you" })
    e:SetOperation(Game.Ops.Attach)
    e:Register()
end

return s
```

`Game.Ops.Attach` automatically enforces "activate only as a sorcery" — you don't need to opt into
that restriction separately; every ability using it gets it for free.

## Paying the cost

Activating an ability with a cost pauses for a real payment step — even a bare `{T}` cost — before
anything happens. The actual tap/sacrifice/discard/life payment is committed at that step, not
when the ability is announced.

## Worked example: Cycling

CR 702.29a — "`[Cost]`, Discard this card: Draw a card," an ability that functions only while the
card sits in its controller's hand. `Game.Costs.DiscardSelf` (not the filter-driven `Discard`) is
what makes that possible — an ability whose cost includes it may be activated from `Hand` instead
of the usual `Battlefield` requirement (CR 602.1's own "unless the ability itself says otherwise"):

```lua
-- Cycling {2}
local s = {}

function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.Mana("{2}"))
    e:AddCost(Game.Costs.DiscardSelf)
    e:SetOperation(Game.Ops.DrawCards(1))
    e:Register()
end

return s
```

Typecycling (CR 702.29e) is the same shape with `Game.Ops.Search` instead of `DrawCards`:

```lua
-- Islandcycling {1}
e:AddCost(Game.Costs.Mana("{1}"))
e:AddCost(Game.Costs.DiscardSelf)
e:SetOperation(Game.Ops.Search({ subtypes = { "Island" } }, 1))
```

Not modeled: CR 702.29c/d's "when you cycle this card" / "cycles or discards" triggers — nothing in
this engine's trigger model yet distinguishes a cycling-cost discard from any other discard.
