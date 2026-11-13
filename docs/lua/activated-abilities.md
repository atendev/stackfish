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
[`Game.Ops.*`](game-constants.md#gameops) tag or a custom function. Required unless the ability
uses `:SetModes`.

**`:SetTarget(value)`** — gives the ability a real target. See [Targeting](#targeting) below.
Optional — omitting it keeps the ability targetless.

**`:SetText(text)`** — CR 405.4, the ability's rules text, such as `"{2}: Draw a card."`. A Seat
sees it with the ability on the stack. Optional. Empty text or a second call is a script error.

**`:SetFunctionsFrom(zoneName)`** — CR 113.6, for an ability that functions from a zone other than
the battlefield, e.g. "{2}{B}: Return this card from your graveyard to your hand." `zoneName` is
`"Hand"`, `"Graveyard"` or `"Exile"` (anything else is a script error; Stack, Library and Command have no
registration point yet). Hand abilities written without it, like Cycling, are still recognized by their
`DiscardSelf` cost. The ability
exists only while its source is in that zone: it registers when the card enters it, is dropped when
the card leaves, and can only be activated/triggers from there. Abilities that don't call this keep
the default — they never register for a card in the graveyard or exile, so an ETB trigger in the same
script stays dormant there. Statics, replacement effects and modifiers accept it too (see their own pages).

**`:SetModes(choice, modes)`** — CR 602.2b, 700.2a, a modal activated ability. See
[modal activated abilities](#modal-activated-abilities) below.

**`:Register()`** — finalizes and queues the ability. Requires exactly one of `:SetOperation` or
`:SetModes`.

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

Every target word is exactly one target, with no "up to N". `:SetTarget` also takes an array of filter
tables for CR 601.2c's multi-word form, one word per entry in order, for example
`a:SetTarget({ { type = "object", card_types = { "Creature" } }, { type = "player" } })`. Use
[`Game.Ops.ChainEach`](game-constants.md#gameops) to give each operation its own word.

## Modal activated abilities

"{1}, {T}: Choose one —" uses `:SetModes` in place of `:SetOperation`:

```lua
local e = Game.CreateActivatedAbility(c)
e:AddCost(Game.Costs.Mana("{1}"))
e:AddCost(Game.Costs.TapSelf)
e:SetModes(Game.ModeChoice.Exactly(1), {
    {
        target = { type = "object", card_types = { "Creature" }, count = 1 },
        operation = Game.Ops.Damage(1),
    },
    { operation = Game.Ops.GainLife(2) },
})
e:Register()
```

`choice` is a [`Game.ModeChoice.*`](game-constants.md#gamemodechoice) value, and each mode is a
`{ target = ..., operation = ... }` table, as on a [modal trigger](triggered-abilities.md#modal-triggered-abilities).
The player announces the modes in the activation itself, beside the targets of the chosen modes
(`ActivateAbility`'s `modes` field, CR 602.2b, 700.2a). There is no separate mode Decision. The
priority entry lists the mode range and each mode's target slots, as a modal spell's cast entry does.

`:SetTarget` beside `:SetModes` is an error, since each mode owns its targets. These are load errors
on this builder:

- `ExactlyChosenByOpponent` or `UpToChosenByOpponent`: another player choosing an activated
  ability's modes isn't built (CR 700.2e).
- A mode `cost` or `pawprint`, and `PawprintBudget`: those belong to a spell's `s.modes()`.
- A mode whose operation is `Game.Ops.AddMana` or `AddManaChoice`: a mana ability can't be modal
  (CR 605.1a, 605.3b).

A function operation, a chain and a `ForEachPlayer` are solo-only modes, as on a spell.

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
