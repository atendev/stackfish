# Targeting and mid-resolution choices

[← index](index.md)

## Filter table shape

Every filter table across this API — `:SetAffected`, `:SetTarget`, `Game.Costs.Sacrifice`/
`Discard`/`TapPermanents`, `Game.Ops.Search`, `Game.CreateCostModifier`'s `:SetFilter`,
`Game.Conditions.DiesMatching`, `Game.InterveningIf.ControlsAtLeast` — shares the same key
vocabulary. Every key is optional.

```lua
{
    controller = "you",              -- "you" | "opponent" | "any"
    card_types = { "Creature" },     -- list of card type names, matched as "or"
    excluded_card_types = { "Wall" },-- negated form of card_types
    subtypes = { "Human" },          -- list of subtype names (CR 205.4b), matched as "or"
    excluded_subtypes = { "Wall" },  -- negated form of subtypes
    supertypes = { "Legendary" },    -- list of supertype names (CR 205.4a)
    excluded_supertypes = { ... },   -- negated form of supertypes
    random = true,                   -- affects which matching object is picked, not membership
}
```

A positive key and its negated counterpart can be combined — `{ card_types = { "Creature" },
excluded_subtypes = { "Wall" } }` is "target creature that isn't a Wall." Not every consumer of
this shape reads every key (e.g. a cost filter has no reason to read `excluded_supertypes` if
nothing needs it yet) — check the specific method's own page if a key doesn't seem to be having an
effect.

`Game.SelectTargets`/`:SetTarget` additionally accept a `type` key (`"object"`, `"player"`, or
`"any"`, default `"object"`) to say what kind of thing is being targeted at all — see below.

## Scripted targeting predicates

For a legality check the plain filter table can't express (e.g. "target creature with power 4 or
greater"), pass a function instead of a table wherever `:SetTarget` is accepted:

```lua
e:SetTarget(function(object, source, controller, state) -> bool)
```

Called once per candidate object; must return a plain boolean. `state` is a snapshot of real,
public board state, so the predicate can inspect other objects' current characteristics.

The player-target equivalent, for "target player with 10 or less life" and similar:

```lua
e:SetTarget({ type = "player", filter = function(player, source, controller, state) -> bool end })
```

Both forms are checked once per candidate before the ability goes on the stack (and again at
resolution) — they're a pure legality check, not a place to make a choice or cause a side effect.

## Asking the player for a target mid-resolution

`Game.SelectTargets(filter)`, called from inside an operation function, suspends the operation and
asks the player to choose a target before continuing:

```lua
e:SetOperation(function(source, controller)
    local target = Game.SelectTargets({ card_types = { "Creature" } })
    -- target is available here exactly as if the call had returned
    -- synchronously
    return { gain_life = 1 }
end)
```

`filter` is the filter-table shape above, plus `type` (`"object"` | `"player"` | `"any"`, default
`"object"`). The result is a table: `{ kind = "object"|"player", id = n }` — check `target.kind` to
tell an object answer from a player one. Always exactly one target per call — no "up to N," no
picking from a specific zone.

## Asking the player a yes/no question mid-resolution

`Game.MayChoose()` (CR 608.2d) is the same idea with no filter to satisfy — a plain optional-action
choice:

```lua
e:SetOperation(function(source, controller)
    if Game.MayChoose() then
        return { gain_life = 4 }
    end
    return { gain_life = 1 }
end)
```

## What these can't do yet

- Only usable from inside an operation function — there's no equivalent for asking mid-condition
  or mid-cost.
- `Game.SelectTargets`/`Game.MayChoose` are for a choice made *during resolution*. Choosing a mode
  for a modal spell ("choose one —") happens before the spell even goes on the stack, as part of
  casting it — a different mechanism entirely, and not something a card script drives (see
  [not-available-yet.md](not-available-yet.md)).
