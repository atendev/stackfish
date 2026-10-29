# Static abilities

[← index](index.md)

CR 604/611/613 — an always-on continuous effect, like "Creatures you control get +1/+1" or "This
creature has protection from red." Built with `Game.CreateStaticAbility(c)`.

```lua
-- Anthem: creatures you control get +1/+1.
local s = {}

function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Filter({ controller = "you", card_types = { "Creature" } }))
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

**`:SetLayer(value)`** — which of CR 613.1's layers this effect applies in. `value` is a
[`Game.Layers.*`](game-constants.md#gamelayers) tag. Required.

**`:SetEffect(value)`** — what the effect does. `value` is a
[`Game.Effects.*`](game-constants.md#gameeffects) tag, or a custom function (see
[Script-backed effects](#script-backed-setaffectedseteffect) below). Required. Registering a layer and
effect that don't belong together (e.g. `SetLayer(TypeChanging)` with
`SetEffect(Game.Effects.ModifyPT(...))`) is a real error — see the pairing table in
[game-constants.md](game-constants.md#gamelayers).

**`:SetAffected(value)`** — which objects this effect applies to. `value` is a
[`Game.Affected`](#gameaffected-object-tags) object tag, a [`Game.Filter({...})`](#gamefilter) value,
or a custom function — see [Script-backed effects](#script-backed-setaffectedseteffect) below. A
raw table is a load error. Omitting `:SetAffected` entirely means the effect applies to every object
(a real, if unusual, shape).

**`:SetCondition(value)`** — CR 611.2c, "this effect applies only as long as X is true." `value`
is a state tag from [`Game.Conditions.*`](game-constants.md#state-tags) (`AtOrBelowLife`,
`ControlsAtLeast`, `Escaped`, `HadNoCounters`) or a custom function
`function(source, controller) -> bool`. An event tag such as `Dies` is a load error, because those
belong to a triggered ability's `:SetCondition`. Optional — omitting it means the effect is
unconditionally active whenever it's registered. `:SetCondition(nil)` and an unknown tag are load errors.

**`:SetDuration(value)`** — makes the effect temporary instead of permanent. `value` is a
[`Game.Durations.*`](game-constants.md#gamedurations) tag. Omitting it defaults to `Permanent`.

```lua
-- Artifacts you control gain flying until end of turn.
e:SetDuration(Game.Durations.UntilEndOfTurn)
```

**`:Register()`** — finalizes and queues the effect. Requires `:SetLayer` and `:SetEffect`.

## Characteristic-defining abilities

**`:SetCharacteristicDefining()`** — CR 604.3: marks the static as a characteristic-defining ability, so
it functions in every zone instead of only the battlefield (a Mistform-style "this card is all colors"
applies in hand, on the stack, in the graveyard, ...). `:Register()` checks CR 604.3a and errors unless
the effect is `Game.Effects.AddSubtype` in `Game.Layers.TypeChanging` or `AddColor`/`SetColors` in
`Game.Layers.ColorChanging`, it uses `:SetAffected(Game.Affected.Self)`, and it has no `:SetCondition`,
`:SetDuration` or `:SetFunctionsFrom`. A power/toughness CDA is not written this way — it's the card
definition's `CharacteristicDefining` P/T value. The effect registers whenever the card's script runs
for a zone (hand, library, cast, battlefield entry, graveyard/exile) — an engine sweep runs the script for
any card found in hand, library, graveyard or exile, so it registers however the card got there. Not the
command zone.

## `Game.Filter`

`Game.Filter({...})` wraps a flat object filter table for `:SetAffected`. Every key is optional:

```lua
e:SetAffected(Game.Filter({
    controller = "you",                 -- "you" | "opponent" | "any", default: no filter
    card_types = { "Creature" },        -- default: no filter
    subtypes = { "Zombie" },
    excluded_subtypes = { "Vampire" },
    supertypes = { "Legendary" },
    excluded_supertypes = { "Basic" },
}))
```

A static ability's `:SetAffected` matches on `controller`, `card_types`, `subtypes`,
`excluded_subtypes`, `supertypes` and `excluded_supertypes`. The other shared filter keys (see
[targeting-and-choices.md](targeting-and-choices.md#filter-table-shape)) are ignored silently for
now; strict key rejection comes later. `self_only`, `enchanted_only` and
`enchanted_player_controls` are errors that point to `Game.Affected`.

## `Game.Affected` object tags

Tags for the cases a filter table can't express, passed straight to `:SetAffected`. A tag can't be
combined with filter keys.

| Tag | Meaning |
|---|---|
| `Game.Affected.Self` | Only this effect's own source. |
| `Game.Affected.Enchanted` | CR 303.4m, "Enchanted creature gets +1/+1": whatever this effect's source is currently attached to. Matches permanents only. |
| `Game.Affected.EnchantedPlayerControls` | CR 303.4m, "Creatures enchanted player controls have lifelink" (Curse of Vitality): objects controlled by the player this effect's source is attached to. Matches nothing if the source isn't attached to a player. |

The player tags (`You`, `Opponent`, `Any`, `EnchantedPlayer`) belong to the
[player modifiers](player-modifiers.md), and passing one here is a load error.

## Script-backed `:SetAffected`/`:SetEffect`

Both also accept a custom function instead of a tag or `Game.Filter` value — the same primitive-vs-custom-function
pattern every other slot in this API uses:

```lua
-- "Other creatures you control get +1/+1" via a predicate instead of a
-- Game.Filter({ controller = .., card_types = .. }) value.
e:SetAffected(function(object, source, controller, state, enchanted)
    return object ~= source
end)

-- An effect whose shape is decided by a function instead of a Game.Effects.*
-- tag.
e:SetEffect(function(source, controller, state, enchanted)
    return Game.Effects.ModifyPT(1, 1)
end)
```

An affects-predicate is invoked once per candidate object and must return a plain boolean. An
effect function is invoked once per query and must return a single `Game.Effects.*` value, such as
`SetPT`, `ModifyPT`, `DoublePT(power, toughness)`, `AddKeyword` (one keyword string or a Lua list),
`RemoveKeyword`, `IgnoreKeyword`, `SwitchPT`, `RemoveAllAbilities`, or any of the type, color,
supertype and subtype constructors. Return `nil` for no effect this call. A raw-key table such as
`{ modify_pt = ... }` or `{ switch_pt = true }` raises an error, and a function that errors is
treated as "no effect this call" rather than a hard failure.

Both functions receive a **state snapshot** as a trailing argument — real, public board state, not
just object identity — plus `enchanted` (CR 303.4m): whatever object `source` is currently
attached to, or `nil` if nothing. `object == enchanted` is the function-form equivalent of
`Game.Affected.Enchanted` for a predicate that needs more than plain identity matching (e.g.
"enchanted creature, but only if it's a Human").

Note: because a function-backed effect's real shape isn't known until it actually runs, a
layer/effect mismatch here (e.g. `SetLayer(PowerToughnessModifying)` whose function returns
`Game.Effects.AddKeyword(...)`) is silently skipped rather than raised as a `:Register()`-time error — a real, but
narrower, safety net than the tag form gets.

## Protection worked examples

The complete CR 702.16 quality list — color, card type, subtype, name, "everything," and player —
all pair with `Game.Layers.AbilityAdding`:

```lua
-- This creature has protection from red.
e:SetLayer(Game.Layers.AbilityAdding)
e:SetEffect(Game.Effects.AddProtectionColor("R"))
e:SetAffected(Game.Affected.Self)
```

```lua
-- This creature has protection from artifacts.
e:SetEffect(Game.Effects.AddProtectionCardType("Artifact"))
```

```lua
-- This creature has protection from Zombies.
e:SetEffect(Game.Effects.AddProtectionSubtype("Zombie"))
```

```lua
-- This creature has protection from Progenitus.
e:SetEffect(Game.Effects.AddProtectionName("Progenitus"))
```

```lua
-- This creature has protection from everything.
e:SetEffect(Game.Effects.AddProtectionEverything)
```

```lua
-- This creature has protection from an opponent.
e:SetEffect(Game.Effects.AddProtectionPlayer("opponent"))
```

Each pairs with `:SetAffected(Game.Affected.Self)` in the common case (a creature granting
itself protection). Protection is enforced automatically everywhere it matters — damage
prevention, illegal targeting, and illegal blocking — you don't need to do anything else once it's
registered. "Protection from any player" isn't a real quality on its own — use
`AddProtectionEverything` for "protection from everything," which covers that and more.

Not yet modeled: protection granted by an Aura/Equipment attachment (CR 702.16b — attach machinery
exists but doesn't hook into protection yet).

## Registering more than one static ability on a card

Call `Game.CreateStaticAbility(c)` again — each `:Register()` is independent, same as triggered
abilities.
