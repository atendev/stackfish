# Replacement effects

[← index](index.md)

CR 614/615/616 — "If this would X, Y instead." Built with `Game.CreateReplacementEffect(c)`.

Omitting every settable field reproduces this builder's default shape — a repeatable regeneration
shield (CR 701.19b, "...each time this would be destroyed," not just the next time — contrast
[`Game.Ops.Regenerate`](game-constants.md#gameops), a resolving spell/ability's own one-shot
shield):

```lua
-- This creature regenerates.
local s = {}

function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetAffected(Game.Affected.Self)
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

**`:SetAffected(value)`** — which objects this effect watches for. `value` is
`Game.Affected.Self` (only this effect's own source) or a [`Game.Filter({...})`](static-abilities.md#gamefilter)
value, whose keys are all optional. Unlike static abilities, this one has no scripted predicate
form, and a raw table is a load error. Default (if never called): no filter, applies to any matching
event regardless of object.

**`:SetFilter(value)`** — which event shape this effect watches. `value` is a
[`Game.EventFilters.*`](game-constants.md#gameeventfilters) tag. Default: `WouldDie`.

**`:SetPayload(value)`** — the actual substitution. `value` is a
[`Game.ReplacementPayloads.*`](game-constants.md#gamereplacementpayloads) tag. Default:
`Regenerate`. A payload only pairs with certain filters (see the table in
[game-constants.md](game-constants.md#gamereplacementpayloads)) — registering a mismatched pair is
a real error.

**`:SetConsumption(value)`** — how many times it can fire. `value` is a
[`Game.Consumption.*`](game-constants.md#gameconsumption) value. Default: `Repeatable`.

**`:Register()`** — finalizes and queues the effect.

## Worked examples

**"Instead" substitution** — CR 614.13-style, works for any zone-move shape (dying, being
sacrificed, discarded, drawn, etc.), since they're all zone changes underneath:

```lua
-- If this creature would be exiled, put it into its owner's graveyard instead.
local e = Game.CreateReplacementEffect(c)
e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Exile"))
e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Graveyard"))
e:SetAffected(Game.Affected.Self)
e:Register()
```

```lua
-- If you would draw a card, exile it instead.
e:SetFilter(Game.EventFilters.WouldMoveZone("Library", "Hand"))
e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
e:SetAffected(Game.Filter({ controller = "you" }))
```

```lua
-- If a creature you control would be sacrificed, exile it instead.
-- (Also catches any other Battlefield -> Graveyard move on a matching
-- creature you control, not just a Sacrifice cost specifically -- they're
-- all the same underlying zone-change shape.)
e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Graveyard"))
e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
e:SetAffected(Game.Filter({ controller = "you", card_types = { "Creature" } }))
```

```lua
-- If a card would be discarded, exile it instead of putting it in a
-- graveyard -- whichever way it got discarded.
e:SetFilter(Game.EventFilters.WouldMoveZone("Hand", "Graveyard"))
e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
e:SetAffected(Game.Filter({ card_types = { "Creature" } }))
```

**Damage prevention/redirection:**

```lua
-- Fog: prevent all combat damage that would be dealt to you this turn.
local e = Game.CreateReplacementEffect(c)
e:SetFilter(Game.EventFilters.DamageDealt)
e:SetPayload(Game.ReplacementPayloads.PreventDamage)
e:SetConsumption(Game.Consumption.OneShot)
e:SetAffected(Game.Filter({ controller = "you" }))
e:Register()
```

```lua
-- Pariah: all damage that would be dealt to you is dealt to this creature
-- instead.
local e = Game.CreateReplacementEffect(c)
e:SetFilter(Game.EventFilters.DamageDealt)
e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
e:SetAffected(Game.Filter({ controller = "you" }))
e:Register()
```

Redirecting to some other chosen target (e.g. "to that source's controller," or "to target
creature") isn't available yet — only "to this permanent" (`RedirectDamageToSource`). CR 614.9's
own validity clause is rechecked live every time — if the redirect destination is no longer a
legal target (left the battlefield, changed type, or the relevant player left the game), the
redirect simply doesn't apply and the original damage proceeds normally.

**How this permanent enters the battlefield** (CR 603.6d/614.1c) — `EntersTapped` and
`EntersWithCounters` only pair with `WouldMoveZone(_, "Battlefield")`:

```lua
-- This creature enters tapped.
local e = Game.CreateReplacementEffect(c)
e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
e:SetPayload(Game.ReplacementPayloads.EntersTapped)
e:SetAffected(Game.Affected.Self)
e:Register()
```

```lua
-- This creature enters with two +1/+1 counters on it (e.g. Modular 2).
local e = Game.CreateReplacementEffect(c)
e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
e:SetPayload(Game.ReplacementPayloads.EntersWithCounters("PlusOnePlusOne", 2))
e:SetAffected(Game.Affected.Self)
e:Register()
```

Multiple `EntersTapped`/`EntersWithCounters` effects matching the same entry never need an
ordering choice (unlike a `ReplaceZoneDestination` substitution) — entering tapped is a one-time
flip, and counters just add up regardless of order.

Bloodthirst and Sunburst are the same shape with their own payloads (see
[game-constants.md](game-constants.md#gamereplacementpayloads)):

```lua
-- Bloodthirst 2
e:SetPayload(Game.ReplacementPayloads.EntersWithCountersBloodthirst("PlusOnePlusOne", 2))
```

```lua
-- Sunburst
e:SetPayload(Game.ReplacementPayloads.EntersWithCountersSunburst)
```

Devour and Amplify use the same filter/payload pairing, but involve a real choice as the permanent
enters (how many creatures to sacrifice; which cards to reveal) — the player is prompted
automatically, nothing extra to set up:

```lua
-- Devour 2
e:SetPayload(Game.ReplacementPayloads.EntersWithCountersDevour(2))
```

```lua
-- Amplify 1
e:SetPayload(Game.ReplacementPayloads.EntersWithCountersAmplify(1))
```

**Entering as a copy** (CR 707.6/614.12a, Clone-style):

```lua
-- This permanent enters the battlefield as a copy of any creature on the
-- battlefield.
local e = Game.CreateReplacementEffect(c)
e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
e:SetPayload(Game.ReplacementPayloads.EntersAsCopy())
e:SetAffected(Game.Affected.Self)
e:Register()
```

Pass the same `exceptions` table `Game.Ops.BecomeCopy` accepts (see
[game-constants.md](game-constants.md#gameops)) if the copy needs to keep some of its own
characteristics.

## CR 616.1 ordering choices

When two or more registered replacement effects (or a permanent's own protection) could both
apply to the same event, the affected player is automatically asked which one applies first — you
don't need to build anything for this, it's handled uniformly across every replacement effect in
the game.

## "Can't be regenerated" interaction

A `Regenerate` payload is rechecked live every time it's asked to apply — if the target has since
gained "can't be regenerated," the shield silently fails to protect it rather than needing to be
re-registered.
