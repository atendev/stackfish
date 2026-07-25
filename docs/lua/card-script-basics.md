# Card script basics

[← index](index.md)

## Card script structure

A card's script is a Lua module that returns a table with one function, `initial_effect`:

```lua
local s = {}

function s.initial_effect(c)
    -- c is this card's own object id
end

return s
```

`initial_effect(c)` is called with the card's own object id every time the card becomes a new
object on the battlefield (see [Registration timing](#registration-timing) below). Inside it, you
build and register one or more abilities/effects using the `Game.Create*` constructors — see
[index.md](index.md#registering-an-ability-one-page-per-kind) for the full list. A single card can
register more than one ability by calling a constructor more than once:

```lua
function s.initial_effect(c)
    local e1 = Game.CreateTriggeredAbility(c)
    e1:SetCondition(Game.Conditions.Enters)
    e1:SetOperation(Game.Ops.GainLife(1))
    e1:Register()

    local e2 = Game.CreateStaticAbility(c)
    e2:SetLayer(Game.Layers.PowerToughnessModifying)
    e2:SetEffect(Game.Effects.ModifyPT(1, 1))
    e2:SetAffected({ self_only = true })
    e2:Register()
end
```

Every `Game.Create*(...)` call returns a **builder** — a value with `:Set*` methods for filling in
its fields, and a `:Register()` method that finalizes it. Nothing takes effect until `:Register()`
is called. Calling `:Register()` with a required field unset (e.g. a triggered ability with no
operation) is a real error, not a silent no-op.

## The condition/operation split

A triggered ability separates "did this happen" from "what do we do about it":

- **condition** — did this ability's trigger event happen? Checked once per matching event, and
  must be a pure yes/no check with no side effects. This is the trigger's own event
  ("When/Whenever/At X").
- **intervening if** — a *separate* check, for abilities phrased "When/Whenever/At X, **if Y**,
  Z." `Y` here is a board-state condition, not an event — it's checked both when the trigger event
  happens and again right before the ability would resolve, so an ability whose intervening-if has
  since gone false simply fizzles instead of resolving. Set independently of `condition` via
  `:SetInterveningIf`.
- **operation** — what does the ability actually do? Runs once, when the ability resolves.

Activated abilities have the same **operation** concept, plus a **cost** (see
[activated-abilities.md](activated-abilities.md)) instead of a condition.

## Primitive vs. custom function

Anywhere this API asks for a condition, an operation, an effect, or an affects-predicate, you have
two choices:

1. **A `Game.*` tag or factory call** — a recognized primitive, e.g. `Game.Conditions.Enters` or
   `Game.Ops.GainLife(1)`. Fast, and covers the common cases — see
   [game-constants.md](game-constants.md) for the full list.
2. **A custom function** — `return function(...) ... end`. Slower, but expresses anything the
   primitive tables don't cover.

A value that doesn't resolve to one of these two shapes (e.g. a typo like `Game.Conditions.Typo`,
which silently indexes to `nil` in plain Lua) fails to load as a real error, rather than being
treated as "always true" or "do nothing."

### Writing a condition

**Primitive** (see [game-constants.md#gameconditions](game-constants.md#gameconditions) for the
full list):

```lua
return Game.Conditions.Enters       -- CR 603, "when this enters the battlefield"
return Game.Conditions.Always       -- no filter, matches every event
```

**Custom function** (general case):

```lua
return function(source, controller, event)
    return event ~= nil
       and event.kind == "zone_change"
       and event.object == source
       and event.to == "battlefield"
end
```

Called with `(source, controller, event)`:
- `source` — this ability's source object id (a plain integer).
- `controller` — the source's controller (a plain integer).
- `event` — the event being checked, as a table, or `nil`. **Only zone changes are currently
  bridged** — every other event kind (damage dealt, life changed, tapped, ...) arrives as `nil`. A
  zone-change table has:
  - `event.kind` — always the string `"zone_change"`.
  - `event.object` — the object id that moved (an integer).
  - `event.from` / `event.to` — the zone it moved out of/into, as a lowercase string
    (`"library"`, `"hand"`, `"battlefield"`, `"graveyard"`, `"stack"`, `"exile"`, `"command"`).

Must return a plain boolean, and must not attempt to read broader game state — a condition
function only ever sees `source`/`controller`/`event` (see
[Hidden information](#hidden-information) below).

### Writing an operation

**Primitive** (see [game-constants.md#gameops](game-constants.md#gameops) for the full list):

```lua
return Game.Ops.GainLife(1)   -- controller gains exactly 1 life
```

**Custom function** (general case) — a custom operation never mutates anything itself. It returns
a plain table describing *intent*, and the engine applies it:

```lua
return function(source, controller)
    return { gain_life = 1 }
end
```

Called with `(source, controller)` — no `event` parameter (an operation isn't re-checking whether
it should run; that's the condition's job). **`gain_life` is currently the only key recognized on
the returned table.** Every other effect shape (damage, counters, pump, drawing cards, ...) is
reachable only via the `Game.Ops.*` primitive form for now, not via a custom function's return
table — see [not-available-yet.md](not-available-yet.md).

## Full worked example

An ETB "gain 1 life" creature:

```lua
-- test_bear.lua
--
-- Test Bear
-- Creature - Bear, 2/2
-- When Test Bear enters the battlefield, you gain 1 life.

local s = {}

function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end

return s
```

The same ability, written the long way with custom functions instead of primitives — identical
observable behavior:

```lua
local s = {}

function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(function(source, controller, event)
        return event ~= nil and event.kind == "zone_change"
           and event.object == source and event.to == "battlefield"
    end)
    e:SetOperation(function(source, controller)
        return { gain_life = 1 }
    end)
    e:Register()
end

return s
```

## Registration timing

A card's script re-runs — every time, unconditionally — whenever it becomes a *new* object on the
battlefield: when it's played from hand as a land, and when a spell resolves and becomes a
permanent. This matches the rule that an object which changes zones becomes a new object with no
memory of its previous existence: whatever the previous object had registered (abilities,
continuous effects, replacement effects) is torn down once that object is gone, so a card that
returns to the battlefield later gets a genuinely fresh registration, not a stale leftover from
before.

Abilities keyed off a zone *other than* the battlefield (a cast trigger, a graveyard-based ability
like flashback) aren't covered by this — self-registration only runs at the two "entering the
battlefield" points above. This applies uniformly to every registration kind (triggered, static,
activated, replacement, cost modifier) — not just triggered abilities.

## Hidden information

A condition function only ever receives `source`/`controller`/`event` as plain values — there's no
way for a condition to read an opponent's hand or library contents, even by accident, since
nothing beyond those three arguments is ever passed to it.

Static-ability affects-predicates and effect-shape custom functions *do* receive a snapshot of
real, public board state as a trailing argument (see [static-abilities.md](static-abilities.md)) —
a deliberately wider boundary than the plain condition callback, since those need to inspect other
permanents to decide who/what an effect applies to.
