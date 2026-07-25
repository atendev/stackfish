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

## What's not covered

- **"Spend this mana only to..." restricted mana** (e.g. Chrome Mox) isn't modeled — mana added
  this way is ordinary, unrestricted mana once it's in the pool.
- **A mana ability that requires a target** doesn't exist in real Magic either (CR 605.1a), so
  this isn't a gap — it's the rule.
