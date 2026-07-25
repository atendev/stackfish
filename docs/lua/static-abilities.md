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
    e:SetAffected({ controller = "you", card_types = { "Creature" } })
    e:Register()
end

return s
```

## Methods

**`:SetLayer(value)`** — which of CR 613.1's layers this effect applies in. `value` is a
[`Game.Layers.*`](game-constants.md#gamelayers) tag. Required.

**`:SetEffect(value)`** — what the effect does. `value` is a
[`Game.Effects.*`](game-constants.md#gameeffects) tag, or a custom function (see
[Script-backed effects](#script-backed-setaffectedseteffect) below). Required. Registering a layer and
effect that don't belong together (e.g. `SetLayer(TypeChanging)` with
`SetEffect(Game.Effects.ModifyPT(...))`) is a real error — see the pairing table in
[game-constants.md](game-constants.md#gamelayers).

**`:SetAffected(value)`** — which objects this effect applies to. `value` is a plain table (every
key optional), or a custom function — see [Filter table](#filter-table) and
[Script-backed effects](#script-backed-setaffectedseteffect) below. Omitting `:SetAffected` entirely
means the effect applies to every object (a real, if unusual, shape).

**`:SetCondition(value)`** — CR 611.2c, "this effect applies only as long as X is true." `value`
is a [`Game.InterveningIf.*`](game-constants.md#gameinterveningif) tag or a custom function
`function(source, controller) -> bool`. Optional — omitting it means the effect is unconditionally
active whenever it's registered.

**`:SetDuration(value)`** — makes the effect temporary instead of permanent. `value` is a
[`Game.Durations.*`](game-constants.md#gamedurations) tag. Omitting it defaults to `Permanent`.

```lua
-- Artifacts you control gain flying until end of turn.
e:SetDuration(Game.Durations.UntilEndOfTurn)
```

**`:Register()`** — finalizes and queues the effect. Requires `:SetLayer` and `:SetEffect`.

## Filter table

The table passed to `:SetAffected` — every key optional:

```lua
e:SetAffected({
    controller = "you",           -- "you" | "opponent" | "any", default: no filter
    card_types = { "Creature" },  -- default: no filter
    self_only = true,             -- default false — "just this effect's own source"
    enchanted_only = true,        -- default false — CR 303.4m, "whatever this is attached to"
})
```

`enchanted_only = true` (CR 303.4m, e.g. "Enchanted creature gets +1/+1") can't be combined with
the other keys — it short-circuits to "whatever this effect's own source (the Aura, or any other
permanent with something attached to it) is currently attached to." Matches only permanents —
enchanting/equipping a player isn't modeled. Combined with the parent effect's own `self_only`,
note this is about the *target of attachment*, not the effect's own source — don't set both
`self_only` and `enchanted_only`.

## Script-backed `:SetAffected`/`:SetEffect`

Both also accept a custom function instead of a table/tag — the same primitive-vs-custom-function
pattern every other slot in this API uses:

```lua
-- "Other creatures you control get +1/+1" via a predicate instead of the
-- {controller=.., card_types=..} table shape.
e:SetAffected(function(object, source, controller, state, enchanted)
    return object ~= source
end)

-- An effect whose shape is decided by a function instead of a Game.Effects.*
-- tag.
e:SetEffect(function(source, controller, state, enchanted)
    return { modify_pt = { power = 1, toughness = 1 } }
end)
```

An affects-predicate is invoked once per candidate object and must return a plain boolean. An
effect-shape function is invoked once per query and must return a table with exactly one of these
keys:

| Key | Shape | Same as |
|---|---|---|
| `set_pt` | `{ power = N, toughness = N }` | `Game.Effects.SetPT` |
| `modify_pt` | `{ power = N, toughness = N }` | `Game.Effects.ModifyPT` |
| `add_keyword` | `{ "Flying", ... }` | `Game.Effects.AddKeyword` |
| `remove_keyword` | `{ "Defender", ... }` | `Game.Effects.RemoveKeyword` |
| `add_card_type` | `{ "Creature", ... }` | `Game.Effects.AddCardType` |
| `add_color` | a color-letter string, e.g. `"WU"` | `Game.Effects.AddColor` |
| `switch_pt` | `true` | `Game.Effects.SwitchPT` |
| `remove_all_abilities` | `true` | `Game.Effects.RemoveAllAbilities` |
| `double_pt` | `{ power = true/false, toughness = true/false }` | no `Game.Effects.*` tag exists for this yet — the function form is currently the only way to double power and/or toughness |

An empty or unrecognized table, or a function that errors, is treated as "no effect this call"
rather than a hard failure.

Both functions receive a **state snapshot** as a trailing argument — real, public board state, not
just object identity — plus `enchanted` (CR 303.4m): whatever object `source` is currently
attached to, or `nil` if nothing. `object == enchanted` is the function-form equivalent of
`{ enchanted_only = true }` for a predicate that needs more than plain identity matching (e.g.
"enchanted creature, but only if it's a Human").

Note: because a function-backed effect's real shape isn't known until it actually runs, a
layer/effect mismatch here (e.g. `SetLayer(PowerToughnessModifying)` whose function returns
`add_keyword`) is silently skipped rather than raised as a `:Register()`-time error — a real, but
narrower, safety net than the tag form gets.

## Protection worked examples

The complete CR 702.16 quality list — color, card type, subtype, name, "everything," and player —
all pair with `Game.Layers.AbilityAdding`:

```lua
-- This creature has protection from red.
e:SetLayer(Game.Layers.AbilityAdding)
e:SetEffect(Game.Effects.AddProtectionColor("R"))
e:SetAffected({ self_only = true })
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

Each pairs with `:SetAffected({ self_only = true })` in the common case (a creature granting
itself protection). Protection is enforced automatically everywhere it matters — damage
prevention, illegal targeting, and illegal blocking — you don't need to do anything else once it's
registered. "Protection from any player" isn't a real quality on its own — use
`AddProtectionEverything` for "protection from everything," which covers that and more.

Not yet modeled: protection granted by an Aura/Equipment attachment (CR 702.16b — attach machinery
exists but doesn't hook into protection yet).

## Registering more than one static ability on a card

Call `Game.CreateStaticAbility(c)` again — each `:Register()` is independent, same as triggered
abilities.
