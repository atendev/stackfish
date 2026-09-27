# Card script basics

[← index](index.md)

## Card script structure

A card's script is a Lua module that returns a table. `register_abilities` is the one every card
with any ability at all needs — see [Registration timing](#registration-timing) below for the two
optional siblings (`target_requirement`, `operation`) a card only needs if it's an Aura-style
targeted permanent or a plain instant/sorcery, respectively.

```lua
local s = {}

function s.register_abilities(c)
    -- c is this card's own object id
end

return s
```

`register_abilities(c)` is called with the card's own object id every time the card becomes a new
object in a zone where it can have abilities (see [Registration timing](#registration-timing) below). Inside it, you
build and register one or more abilities/effects using the `Game.Create*` constructors — see
[index.md](index.md#registering-an-ability-one-page-per-kind) for the full list. A single card can
register more than one ability by calling a constructor more than once:

```lua
function s.register_abilities(c)
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
it should run; that's the condition's job). Recognized keys on the returned table: `gain_life`,
`draw_cards`, `deal_damage`, `add_counters`, `sacrifice_chosen`/`discard_chosen` (see
[Choosing N objects/cards mid-resolution](targeting-and-choices.md#choosing-n-objectscards-mid-resolution)),
and `cast_spell` (see [Casting a spell from a script](targeting-and-choices.md)). `draw_cards`/
`deal_damage`/`add_counters` go through the exact same replacement-checked/prevention/lifelink
pipeline the `Game.Ops.*` primitive forms use — see
[Drawing, dealing damage, and adding counters](targeting-and-choices.md#drawing-dealing-damage-and-adding-counters)
for the full table shapes. Pump/type-changing effects are still reachable only via the `Game.Ops.*`
primitive form — see [not-available-yet.md](not-available-yet.md).

## Full worked example

An ETB "gain 1 life" creature:

```lua
-- test_bear.lua
--
-- Test Bear
-- Creature - Bear, 2/2
-- When Test Bear enters the battlefield, you gain 1 life.

local s = {}

function s.register_abilities(c)
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

function s.register_abilities(c)
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

A card's script re-runs — every time, unconditionally — whenever the card becomes a *new* object in
a zone where it can have abilities. This matches the rule that an object which changes zones becomes
a new object with no memory of its previous existence: whatever the previous object had registered is
torn down once that object is gone, so a card that returns later gets a genuinely fresh registration,
never a stale leftover or a second copy. `register_abilities(c)` is therefore called with a different
`c` each time, and **which registrations take effect depends on the zone `c` is in**:

| Zone `c` enters | What registers |
|---|---|
| **Hand** (draw, opening hand, tutor) | Triggered and activated abilities (this is how a hand-only ability like Cycling exists). Static abilities, replacement effects, cost modifiers and [player modifiers](player-modifiers.md) are ignored — they don't function from hand. |
| **Stack** (the moment a spell is cast, CR 601.2a) | Only triggers whose condition is [`Game.Conditions.Cast`](game-constants.md#gameconditions). Everything else is ignored until the permanent exists. |
| **Battlefield** (a land played, a spell resolving, a token created) | Everything except `Cast` triggers. |

All script-registered abilities are dropped again when the object leaves that zone — the script is
simply run again for the next one.

```lua
-- Cascade-style: "When you cast this spell, gain 3 life." Exists only while the spell is on the
-- stack; the "enters" trigger below only exists once it's a permanent.
function s.register_abilities(c)
    local cast = Game.CreateTriggeredAbility(c)
    cast:SetCondition(Game.Conditions.Cast)
    cast:SetOperation(Game.Ops.GainLife(3))
    cast:Register()

    local etb = Game.CreateTriggeredAbility(c)
    etb:SetCondition(Game.Conditions.Enters)
    etb:SetOperation(Game.Ops.GainLife(1))
    etb:Register()
end
```

Not covered: an ability that functions from the graveyard or exile (flashback-style), or a static
effect that must work off the battlefield (a characteristic-defining ability, CR 604.3, or a spell's
own cost reduction) — self-registration never runs for those zones or keeps those registrations, so
no such script-declared shape exists yet.

### The two functions that *aren't* battlefield-timed

`register_abilities(c)` can't help a card whose own base shape is needed *before* it's ever on the
battlefield — an Aura's target has to be legal at cast time, and a plain instant/sorcery that
resolves straight to the graveyard never becomes a battlefield object at all. Two optional sibling
functions cover those cases instead, each called once when the card's `CardDefinition` itself is
built rather than per battlefield entry — no `GameState`/`ObjectId` involved, since neither varies
by which specific object ends up carrying it:

| Function | For | Docs |
|---|---|---|
| `target_requirement()` | An Aura's "Enchant [quality]" line, or any other spell's own base targeting (CR 601.2c/702.5a) | [targeting-and-choices.md](targeting-and-choices.md#a-cards-own-base-targeting-enchant-etc) |
| `operation()` | A plain spell's own resolution effect (CR 608.2m) — Armageddon's "Destroy all lands," Shock's "deals 2 damage to any target" | below |
| `alternative_costs()` | Flashback/escape-style alternative costs (CR 601.2b) | below |
| `buyback_cost()` | Buyback's optional additional cost (CR 702.27a) | below |

Both are genuinely optional — the overwhelming majority of cards need neither (a creature with
only triggered/static abilities has no base target and no spell-level effect of its own to
declare).

#### Writing `operation()`

Same primitive-vs-custom-function shape [writing an operation](#writing-an-operation) above
already describes, just called with no arguments instead of resolved from a `:SetOperation(...)`
argument:

```lua
-- Armageddon: Destroy all lands.
local s = {}

function s.operation()
    return Game.Ops.DestroyAll({ card_types = { "Land" } })
end

return s
```

A targeted spell combines both new functions — `target_requirement()` for what's legal to target,
`operation()` for what happens to it:

```lua
-- Test Bolt: deals 2 damage to any target.
local s = {}

function s.target_requirement()
    return { type = "any", count = 1 }
end

function s.operation()
    return Game.Ops.Damage(2)
end

return s
```

A card with no `operation()` function at all keeps the pre-existing default: its effect can only
be a Rust-native primitive supplied some other way (direct construction today; TOML has no field
for this either — see [scenario_format.md](../scenario_format.md)).

#### Writing `alternative_costs()`

CR 601.2b's alternative costs — Flashback (CR 702.34a), Escape (CR 702.138a), and anything else
built the same way — as a card's own base data, same "called once, no `GameState`/`ObjectId`"
timing as `target_requirement()`/`operation()` above. Returns an **array**: a card can grant more
than one alternative cost (each selectable independently when casting — see
[targeting-and-choices.md](targeting-and-choices.md)'s cast-time announcement, same "announced at
cast time" pattern `{X}` uses).

```lua
-- Flashback {R}: cast from the graveyard, exiled instead of going anywhere else once it leaves the stack.
function s.alternative_costs()
    return {
        {
            description = "Flashback {R}",
            from_zone = "Graveyard",
            costs = { Game.Costs.Mana("{R}") },
            exile_on_leaving_stack = true,
        },
    }
end
```

Each entry is a table:

- `description` (required) — plain text, not mechanically enforced, but real (not silently
  defaulted) so an announcement/UI has something to show.
- `from_zone` (required) — a zone name string (`"Graveyard"`, `"Exile"`, ...) — the zone the card
  must actually be in for this alternative to apply. This is also the permission grant itself
  (CR 601.2b): declaring `from_zone = "Graveyard"` is what lets the card be cast from there at all.
- `costs` (required) — an array of [`Game.Costs.*`](game-constants.md#gamecosts) values, the exact
  same vocabulary [activated-abilities.md](activated-abilities.md#methods)'s `:AddCost` accepts.
  `Game.Costs.Mana(...)` may appear at most once per entry (same rule `:AddCost` already enforces
  for an activated ability's cost). Escape's own "exile N other cards from your graveyard" is
  `Game.Costs.ExileFromGraveyard({ ... }, n)` alongside a mana component:

  ```lua
  costs = { Game.Costs.Mana("{2}{B}"), Game.Costs.ExileFromGraveyard({}, 4) }
  ```
- `exile_on_leaving_stack` (optional, default `false`) — CR 702.34a: exiled instead of its normal
  destination the next time it leaves the stack, whether resolving or fizzling.
- `escape` (optional, default `false`) — `true` only for an Escape-shaped alternative cost; this is
  the only thing that structurally distinguishes "cast from a graveyard with an escape ability"
  from any other graveyard-cast alternative cost (both share `from_zone = "Graveyard"`).

A card with no `alternative_costs()` function at all keeps the pre-existing default: no alternative
costs at all, same as the overwhelming majority of real cards.

#### Writing `buyback_cost()`

CR 702.27a Buyback's optional additional cost — paid *in addition to* whatever cost already
applies (usually the printed mana cost), never a replacement for it, so it's a single flat
mana-cost string rather than the fuller `costs` array `alternative_costs()` uses:

```lua
function s.buyback_cost()
    return "{2}"
end
```

Parsed directly via the same mana-cost grammar [`Game.Costs.Mana(...)`](game-constants.md#gamecosts)
itself uses — a malformed string (e.g. `"not a cost"`) is a real load error, not a silently-ignored
buyback. A card with no `buyback_cost()` function at all keeps the pre-existing default: no
buyback cost, same as the overwhelming majority of real cards.

## Hidden information

A condition function only ever receives `source`/`controller`/`event` as plain values — there's no
way for a condition to read an opponent's hand or library contents, even by accident, since
nothing beyond those three arguments is ever passed to it.

Static-ability affects-predicates and effect-shape custom functions *do* receive a snapshot of
real, public board state as a trailing argument (see [static-abilities.md](static-abilities.md)) —
a deliberately wider boundary than the plain condition callback, since those need to inspect other
permanents to decide who/what an effect applies to.

An operation function (`:SetOperation(function(source, controller) ... end)`, or `s.operation()`'s
own custom-function shape) gets the same snapshot too, as a third `state` argument
(`phase5/mass-choice-effects`) — `function(source, controller, state) ... end`. Same public-zones-
only, hidden-zones-as-counts-only boundary as everywhere else this snapshot appears (CR 400.2/402.3):
`state.battlefield`/`state.exile` are full card-summary arrays (`card_types`, `controller`, ...);
`state.hand_count`/`state.library_count` are per-player counts only, never contents. This is what lets
an operation compute something like Balance's own "the fewest lands any player controls" before
making a choice — see [targeting-and-choices.md](targeting-and-choices.md#applying-an-effect-to-each-player).
