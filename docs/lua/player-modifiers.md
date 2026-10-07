# Player modifiers

[← index](index.md)

Static effects that change a *player's* rules rather than an object's characteristics: hand size,
hand visibility, draw limits, and land plays. Each has its own constructor and enforcement point —
they are not CR 613 layer effects, so they don't go through
[`Game.CreateStaticAbility`](static-abilities.md). All four follow the same shape:

```lua
local m = Game.CreateXModifier(c)
m:SetAffected(Game.XAffected.You)   -- which player(s)
-- ...a magnitude setter, where the builder has one...
m:Register()
```

`:SetAffected` and (where present) the magnitude setter are **required** — calling `:Register()`
without them is a real error, not a default.

A modifier lasts as long as its source stays on the battlefield; when the source leaves, every
modifier it registered is removed.

## Functioning from another zone

Every builder here accepts `:SetFunctionsFrom("Hand"|"Graveyard"|"Exile")` (CR 113.6): the modifier exists only while
its source is in that zone and is removed when the card leaves it. Without it, only on the battlefield.

## Which player is affected

`:SetAffected` takes a tagged value, not a filter table:

| Value | Meaning |
|---|---|
| `Game.<Kind>Affected.You` | the source's controller |
| `Game.<Kind>Affected.Opponent` | each opponent of the source's controller |
| `Game.<Kind>Affected.Any` | every player |
| `Game.<Kind>Affected.EnchantedPlayer` | the player this Aura is attached to (CR 303.4m). **Hand size and hand reveal only.** |

`<Kind>` is `HandSize`, `HandReveal`, `DrawLimit` or `LandPlay`.

## `Game.CreateHandSizeModifier(c)` — CR 402.2

`:SetEffect(value)` is required. Values from `Game.HandSizeEffects.*`:

| Name | Meaning |
|---|---|
| `NoMaximum` | "You have no maximum hand size." |
| `SetTo(n)` | "Your maximum hand size is `n`." |

```lua
-- Enchanted player's maximum hand size is three.
local s = {}

function s.register_abilities(c)
    local m = Game.CreateHandSizeModifier(c)
    m:SetAffected(Game.HandSizeAffected.EnchantedPlayer)
    m:SetEffect(Game.HandSizeEffects.SetTo(3))
    m:Register()
end

return s
```

Enforced during the cleanup step's discard-to-hand-size. Modifiers do **not** stack: when several
apply to the same player, the one with the latest timestamp wins outright.

## `Game.CreateHandRevealModifier(c)` — CR 701.20a

"Plays with their hand revealed." Only `:SetAffected` and `:Register()`.

```lua
-- Enchanted player plays with their hand revealed.
local m = Game.CreateHandRevealModifier(c)
m:SetAffected(Game.HandRevealAffected.EnchantedPlayer)
m:Register()
```

> **Not enforced.** The modifier registers and is queryable, but nothing in the engine consumes
> it: there is no per-viewer visibility boundary yet, and card summaries never encode hand
> contents for anyone. Registering it today changes no behavior.

## `Game.CreateDrawLimitModifier(c)` — CR 121.2b

`:SetLimit(n)` is required — the most cards the player can draw each turn.

```lua
-- You can't draw more than one card each turn.
local m = Game.CreateDrawLimitModifier(c)
m:SetAffected(Game.DrawLimitAffected.You)
m:SetLimit(1)
m:Register()
```

Enforced at the single point every draw goes through, so both the draw step and a scripted
`Game.Ops.DrawCards` respect it. A multi-card draw is carried out as far as the limit allows
(CR 121.2b). Modifiers do not stack: the latest timestamp wins. `EnchantedPlayer` is not available.

## `Game.CreateLandPlayModifier(c)` — CR 305.2

`:SetAdditional(n)` is required — how many extra land plays per turn this grants.

```lua
-- You may play an additional land on each of your turns.
local m = Game.CreateLandPlayModifier(c)
m:SetAffected(Game.LandPlayAffected.You)
m:SetAdditional(1)
m:Register()
```

Unlike the other three, land-play modifiers **stack additively**: two registered `+1` modifiers
mean three land plays per turn (CR 305.2). `EnchantedPlayer` is not available.

## Not modeled

- Hand-reveal enforcement (above).
- CR 121.2b's "can't choose to draw multiple cards" and "can't pay a cost that includes drawing
  multiple cards" — no choice-based draw or draw-cost mechanism exists.
- `EnchantedPlayer` for draw limits and land plays — no real card needs it.
