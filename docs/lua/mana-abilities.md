# Mana abilities

[← index](index.md)

CR 605 — `{T}: Add {C}`-style abilities that produce mana without going on the stack and without
being responded to. Built with `Game.CreateManaAbility(c)` — a dedicated builder, not a mode of
[activated abilities](activated-abilities.md): it never accepts a target at all (CR 605.1a), and
its operation can only be `Game.Ops.AddMana`/`Game.Ops.AddManaChoice` — nothing else produces a
real mana ability.

```lua
-- Wastes
-- {T}: Add {C}.
local s = {}

function s.register_abilities(c)
    local e = Game.CreateManaAbility(c)
    e:SetOperation(Game.Ops.AddMana("C"))
    e:Register()
end

return s
```

## Methods

**`:AddCost(value)`** — same [`Game.Costs.*`](game-constants.md#gamecosts) values an activated
ability's `:AddCost` accepts (`TapSelf`, `Sacrifice`, `Discard`, `TapPermanents`, `Life`, `Mana`).
Called once per cost component. **Whether you call this at all changes how the ability behaves —
see [No cost vs. a real cost](#no-cost-vs-a-real-cost) below.**

**`:SetOperation(value)`** — `value` must be a [`Game.Ops.AddMana`](game-constants.md#gameops) or
`Game.Ops.AddManaChoice` call. Anything else (including a custom function) is a real error at
`:Register()` time — nothing else actually produces correct mana-ability behavior, so this is
rejected rather than silently doing the wrong thing.

**`:Register()`** — finalizes and queues the ability. Requires `:SetOperation`.

There is no `:SetTarget` on this builder at all — CR 605.1a means a mana ability never has a
target, so the method simply doesn't exist here, rather than being accepted and ignored.

## No cost vs. a real cost

**Omit `:AddCost` entirely** for the common case — a plain `{T}: Add X` land or rock. The ability
resolves the instant it's activated: the source taps, the mana appears, and nothing pauses for a
payment step.

```lua
-- {T}: Add {G}.
local e = Game.CreateManaAbility(c)
e:SetOperation(Game.Ops.AddMana("G"))
e:Register()
```

**Call `:AddCost` at all** (even just `Game.Costs.TapSelf`) and the ability instead pauses for a
real payment step before resolving — same as every other ability's cost in this engine, including
a bare `{T}`. It still never goes on the stack and still can't be responded to; the only
difference is that paying the cost is now its own explicit step:

```lua
-- Same {T}: Add {G}, but written with an explicit cost instead of relying
-- on the no-cost shape above. Behaves the same to a player, but requires
-- confirming the payment as its own step rather than resolving instantly.
local e = Game.CreateManaAbility(c)
e:AddCost(Game.Costs.TapSelf)
e:SetOperation(Game.Ops.AddMana("G"))
e:Register()
```

A real cost is what makes a **compound-cost mana ability** possible at all — something no plain
`{T}: Add X` land needs, but a Treasure-token-style or Lotus-Petal-style card does:

```lua
-- Sacrifice this artifact: Add one mana of any color.
local e = Game.CreateManaAbility(c)
e:AddCost(Game.Costs.Sacrifice({ card_types = { "Artifact" } }))
e:SetOperation(Game.Ops.AddManaChoice(1, "WUBRG"))
e:Register()
```

Paying that cost drives the same sacrifice/discard/life/mana machinery any other ability's cost
uses — a `Sacrifice` component pauses to ask which matching permanent to sacrifice, same as it
would for a non-mana ability, then the mana is added once that sacrifice actually commits.

## Choosing a color with `AddManaChoice`

Same as any other `AddManaChoice` operation — the caller supplies the chosen color(s) when
activating the ability, not when building it. See [game-constants.md](game-constants.md#gameops)
for `Game.Ops.AddMana`/`AddManaChoice`'s exact argument shapes.

## Restricted mana

CR 106.11's "spend this mana only to cast [card type] spells" (Mishra's Workshop: "Add {C}{C}{C}.
Spend this mana only to cast artifact spells.") — use `Game.Ops.AddRestrictedMana(letter, cardType,
count?)` in place of `Game.Ops.AddMana`:

```lua
local e = Game.CreateManaAbility(c)
e:SetOperation(Game.Ops.AddRestrictedMana("C", "Artifact", 3))
e:Register()
```

The payment engine only accepts a restricted unit toward casting a spell of the named card type —
never toward an activated ability's cost, even one belonging to a permanent of that same card type.

## Snow mana

CR 106.3 and 107.4h. Mana a snow source produces is snow mana, and only snow mana pays an `{S}` cost
symbol. There is nothing to write in the script. When a permanent that has the Snow supertype
activates a mana ability, the engine marks each unit `snow_colored` or `snow_colorless`. A snow unit
is still its color for every other cost symbol. Restricted mana stays restricted and is not marked.

```lua
-- Snow-Covered Forest
-- Snow Basic Land - Forest
-- {T}: Add {G}.
```

The supertype comes from the card (or from `AddSupertype("Snow")`), so a script does not name it
again in `register_abilities`.

## What's not covered

- **Triggered mana abilities** are not built, so their mana is never marked snow.

- **Object-specific restrictions** ("only to cast the exiled card") aren't modeled — only a
  card-type filter.
- **Ability-activation restrictions** (vs. casting a spell) aren't modeled — restricted mana is
  spell-cast-only.
- **A mana ability that requires a target** doesn't exist in real Magic either (CR 605.1a), so
  this isn't a gap — it's the rule.
