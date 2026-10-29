# Targeting and mid-resolution choices

[← index](index.md)

## Filter table shape

Every object filter across this API shares one key vocabulary: `:SetTarget`,
`Game.Costs.Sacrifice`/`Discard`/`TapPermanents`, `Game.Ops.Search`, `Game.Ops.DestroyAll` and the
other mass ops, `Game.Conditions.DiesMatching`, `Game.Conditions.ControlsAtLeast`, and the
[`Game.Filter({...})`](static-abilities.md#gamefilter) value that `:SetAffected` and a cost modifier's
`:SetFilter` take. Every key is optional.

```lua
{
    controller = "you",              -- "you" | "opponent" | "any"
    card_types = { "Creature" },     -- list of card type names, matched as "or"
    excluded_card_types = { "Wall" },-- negated form of card_types
    subtypes = { "Human" },          -- list of subtype names (CR 205.4b), matched as "or"
    excluded_subtypes = { "Wall" },  -- negated form of subtypes
    supertypes = { "Legendary" },    -- list of supertype names (CR 205.4a)
    excluded_supertypes = { ... },   -- negated form of supertypes
    zone = "Hand",                   -- zone name, see below
    mana_value_equals = 3,           -- number or "X", see below
    random = true,                   -- affects which matching object is picked, not membership
}
```

A positive key and its negated counterpart can be combined — `{ card_types = { "Creature" },
excluded_subtypes = { "Wall" } }` is "target creature that isn't a Wall." Each consumer matches on
the keys it can, and ignores the rest silently for now (strict key rejection comes later):

| Consumer | Keys it matches on |
|---|---|
| `Game.Filter` in `:SetAffected` and `:SetFilter`, and the mass ops such as `Game.Ops.DestroyAll` | `controller`, `card_types`, `subtypes`, `excluded_subtypes`, `supertypes`, `excluded_supertypes` |
| `Game.Ops.Search`, `Game.Ops.DiscardRandom`, `Game.Costs.Sacrifice`/`Discard`/`TapPermanents` | `controller`, `card_types`, `supertypes`, `subtypes`, `random` |
| Target tables (`:SetTarget`, `s.target_requirement()`, `Game.SelectTargets`) | `controller`, `card_types`, `excluded_card_types`, `subtypes`, `excluded_subtypes`, `supertypes`, `excluded_supertypes`, `zone`, `mana_value_*` |

Ops, costs and condition constructors take the raw flat table. Only `:SetAffected` and a cost
modifier's `:SetFilter` take it wrapped in `Game.Filter`.

`Game.SelectTargets`/`:SetTarget` additionally accept a `type` key (`"object"`, `"player"`, or
`"any"`, default `"object"`) to say what kind of thing is being targeted at all — see below.
`type = "player"` honors the shared `controller` key too (e.g. `{ type = "player", controller =
"opponent" }` for "target opponent") — omit it for a plain "target player" that accepts anyone.

`:SetTarget` has two more values: `{ type = "ability" }` ("counter target activated or triggered
ability") and `{ type = "triggering" }`. `"triggering"` is only meaningful on a triggered ability whose
condition names a spell or ability, which today is `Game.Conditions.BecomesTargetOfOpponent`. The target
is the spell or ability that caused the trigger, filled in automatically with no choice for the
player, and the trigger is dropped if that spell or ability has already left the stack (CR 603.3d).
`Game.RegisterWard` uses it.

```lua
-- "Target opponent" - no script needed for this common case.
function s.target_requirement()
    return { type = "player", controller = "opponent", count = 1 }
end
```

Every filter table — `Game.SelectTargets`/`Game.ChooseN`'s own, and `:SetTarget`/
`s.target_requirement()`'s — additionally accepts a `zone` key: a zone name string (`"Hand"`,
`"Battlefield"`, `"Stack"`, ...), same names/casing the TOML fixture format uses. Defaults to
`"Battlefield"` if omitted, same as before. This is how a mid-resolution ask reaches into a player's
own hand (CR 701.9's discard, e.g. `Game.ChooseN({ zone = "Hand", controller = "you" }, n)`), and how
a card's own base "target" reaches onto the stack (CR 701.6a's "counter target spell" — see
`s.target_requirement()` below and `Game.Ops.Counter` in
[game-constants.md](game-constants.md#gameops)). A graveyard or exile `zone` is also how a card
aims at a card there, for example Reanimate's "target creature card in a graveyard":

```lua
function s.target_requirement()
    return { zone = "Graveyard", card_types = { "Creature" }, count = 1 }
end
function s.operation()
    return Game.Ops.ReturnToBattlefield()   -- the call parentheses are required
end
```

An object filter also accepts up to one of `mana_value_equals`/`mana_value_at_most`/
`mana_value_at_least` (CR 202.3, e.g. Convolute's "mana value 3 or less" or "counter target spell
with mana value X") — each takes either a plain number or the literal string `"X"`, meaning this
casting's own announced X (CR 107.3a; only meaningful on a card whose own mana cost has an `{X}` to
announce a value for):

```lua
function s.target_requirement()
    return { zone = "Stack", mana_value_equals = "X", count = 1 }
end
```

`"X"` resolves against whatever `x_value` was announced for *this* cast/activation, re-checked fresh
at resolution the same way every other filter key already is (CR 608.2b) — a target counted illegal
because its mana value stopped matching X is treated exactly like any other now-illegal target.

## A card's own base targeting (Enchant, etc.)

The filter table above is also how a card declares *its own* target requirement — the thing that
determines what's a legal target when the card itself is cast, before any ability is even in play.
This is CR 601.2c's "target" on the spell itself: an Aura's "Enchant [quality]" line (CR 702.5a) is
the most common case, but any targeted spell works the same way ("target creature" on a removal
spell, etc.).

Add an optional `s.target_requirement()` function to the card's script — a sibling to
`s.register_abilities(c)`, not a replacement for it:

```lua
-- Enchant Wall
local s = {}

function s.target_requirement()
    return { subtypes = { "Wall" }, count = 1 }
end

function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(Game.Affected.Enchanted)
    e:Register()
end

return s
```

Unlike `register_abilities(c)`, `target_requirement()` takes no arguments and runs once — when the
card's own definition is built, not per copy as it enters play — since the restriction is fixed
text that doesn't depend on game state or which object ends up carrying it. It returns the same
flat filter table as everywhere else on this page, plus these siblings:

```lua
{
    type = "object",        -- "object" | "player" | "any", default "object"
    count = 1,               -- required
    up_to = false,            -- CR 601.2c/115.6 "up to N"
    at_least = false,          -- CR 601.2c/115.6 "N or more" - mutually exclusive with up_to
    exclude_source = false,     -- CR 115.4 "another target"
    -- ...plus card_types/subtypes/supertypes/controller/etc from the filter table above
}
```

A card with no `target_requirement()` function needs no targets to cast — the same default as
omitting `[cards.target_requirement]` in a TOML fixture.

For a legality check the flat filter shape can't express, set `filter` to a function. The `filter`
key takes only a function, never a nested table (a nested `filter = { ... }` is a load error), and
the filter keys sit beside it as siblings. With `type = "player"` the function is a player predicate;
otherwise it is an object predicate. Same scripted-predicate shapes `:SetTarget` accepts, see
[Scripted targeting predicates](#scripted-targeting-predicates) below:

```lua
-- Enchant creature with power 4 or greater
function s.target_requirement()
    return { count = 1, filter = function(object, source, controller, state)
        for _, card in ipairs(state.battlefield) do
            if card.id == object then
                return card.power ~= nil and card.power >= 4
            end
        end
        return false
    end }
end
```

`count`/`up_to`/`at_least`/`exclude_source` still sit alongside `filter` as sibling keys either way.

### Two-target spells (fight and bite)

A spell with two distinct "target" words returns an array of requirement tables, one per word, in
card order. Each entry is the same table shape as the single form above. The engine builds one
`TargetSpec::Multiple` from it, and the caster submits the targets in the same order (the first
target for the first entry, the second for the second). Each entry is checked on its own, so the same
creature can be picked for two entries when both filters allow it.

```lua
-- Prey Upon: "Target creature you control fights target creature you don't control."
function s.target_requirement()
    return {
        { card_types = { "Creature" }, controller = "you", count = 1 },
        { card_types = { "Creature" }, controller = "opponent", count = 1 },
    }
end
function s.operation()
    return Game.Ops.Fight
end
```

```lua
-- Bite: "Target creature you control deals damage equal to its power to target creature you don't control."
function s.operation()
    return Game.Ops.Bite   -- same two-entry target_requirement as above
end
```

The return value must be either one requirement table (anything with no array part, as before) or a
plain non-empty array of requirement tables. An empty table, a table that mixes array entries with
keys such as `count`, an array with gaps, and an array entry that is not a table are all load
errors. A one-entry array is the same single word as a bare table.

An ability (not a spell) can also use `Fight` or `Bite` with a normal single `:SetTarget`. The
ability's own source is then the first creature, for example "When this creature enters, it fights
target creature you don't control" (a triggered ability) or "{1}: This creature fights target
creature you don't control" (an activated one). `:SetTarget` also takes an array of filters, one
target word each (see [triggered-abilities.md](triggered-abilities.md)). A two-word `Fight` on an ability
is not tested yet.

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
    return Game.Ops.GainLife(1)
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
        return Game.Ops.GainLife(4)
    end
    return Game.Ops.GainLife(1)
end)
```

## Choosing N objects/cards mid-resolution

`Game.ChooseN(filter, n, opts)` (`phase5/mass-choice-effects`) asks the player to choose `n` objects
or cards matching `filter`, one at a time, without repeats — built entirely on top of
`Game.SelectTargets`/`Game.MayChoose` above, not a new request shape:

```lua
e:SetOperation(function(source, controller)
    local chosen = Game.ChooseN({ card_types = { "Creature" }, controller = "you" }, 2)
    -- chosen is a list of { kind = "object"|"player", id = n } tables, same
    -- shape Game.SelectTargets itself returns, one entry per pick
    return Game.Ops.SacrificeChosen(chosen)
end)
```

`opts.up_to`/`opts.at_least` mirror `target_requirement()`'s own CR 601.2c/115.6 count modes — at most
one should be `true` (`at_least` wins if both are set, same "no validation, just two flags" convention
the underlying `TargetRequirement` decode already takes):

- neither (default): exactly `n`, no `Game.MayChoose`-shaped ask at all.
- `up_to`: anywhere from 0 to `n` — before every pick (including the first), the player is asked "one
  more?"; answering no stops the loop early with however many have been chosen so far.
- `at_least`: `n` or more, no upper bound of its own (capped only by however many legal candidates
  actually exist — the caller's own responsibility to leave a real choice, same stance every other
  count shape in this engine takes). The first `n` picks are mandatory, no ask; every pick past that
  asks "one more?" first, indefinitely, until declined.

Each pick's filter automatically excludes every object already chosen this call, so the same creature
can't be picked twice. `filter` accepts the same scripted-predicate shape `:SetTarget`/
`target_requirement()` do (see [Scripted targeting predicates](#scripted-targeting-predicates) above)
— a bare `function(object, source, controller, state) -> bool`, or `{ type = "player", filter = fn }`
for a player-candidate predicate — not just the flat option-bag table, for a check the table shape
can't express (e.g. "creature with power 4 or greater," which has no `power` key):

```lua
local chosen = Game.ChooseN(function(object, source, controller, state)
    for _, card in ipairs(state.battlefield) do
        if card.id == object then
            return card.power ~= nil and card.power >= 4
        end
    end
    return false
end, 1)
```

`Game.Ops.SacrificeChosen(list)`/`Game.Ops.DiscardChosen(list)` take a `Game.ChooseN` result and
sacrifice/discard exactly that list (CR 701.21/701.9) — the operation's own return value, same as any
other `Game.Ops.*` result:

```lua
e:SetOperation(function(source, controller)
    local chosen = Game.ChooseN({ zone = "Hand", controller = "you" }, 1)
    return Game.Ops.DiscardChosen(chosen)
end)
```

To do both in the same turn, return `Game.Ops.Chain(Game.Ops.SacrificeChosen(a), Game.Ops.DiscardChosen(b))`
(see `Game.Ops.ForEachPlayer`'s own example below). A raw-key table such as `{ sacrifice_chosen = ... }`
raises an error.

## Drawing, dealing damage, and adding counters

A custom operation function returns a `Game.Ops.*` value, or `nil` for no further effect — the same
"describe what happened, the engine applies it for real" split every other value on this page uses:

```lua
e:SetOperation(function(source, controller)
    return Game.Ops.DrawCards(2)
end)
```

`Game.Ops.DrawCards(n)` goes through the exact same replaceable draw pipeline as its primitive form
(CR 121/616.1) — a redirect effect ("draw a card instead") still applies, and two or more competing
replacement effects on the same draw still pause for a real ordering choice. Always draws for
`controller` — there's no way for an operation to make a *different* player draw.

```lua
e:SetOperation(function(source, controller)
    local chosen = Game.SelectTargets({ card_types = { "Creature" } })
    return Game.Ops.DealDamageTo(chosen, 3)
end)
```

`Game.Ops.DealDamageTo(pick, amount)` takes a `{ kind = "object"|"player", id = n }` pick, the exact
shape `Game.SelectTargets`/`Game.ChooseN` return, so a picked target passes straight through. It
becomes a real `DamageDealt` event, going through the normal replacement/prevention pipeline
(CR 615/616) and applying Deathtouch/Wither/Infect/Lifelink exactly like combat damage or
`Game.Ops.Damage`. CR 601.2d "divide as you choose" still isn't modeled — each `DealDamageTo` deals its
full `amount` independently.

```lua
e:SetOperation(function(source, controller)
    return Game.Ops.AddCountersTo({ kind = "object", id = some_id }, "PlusOnePlusOne", 1)
end)
```

`Game.Ops.AddCountersTo(pick, counter, amount)` — `counter` is a counter kind name
(`"PlusOnePlusOne"`, `"MinusOneMinusOne"`, `"Loyalty"`, `"Poison"`, `"Energy"`, `"Experience"`,
`"Charge"`, or any other string for a custom counter kind). Object targets only — a player pick is
silently skipped, same as every other CR 613 counter-adding shape in this engine.

To return several of these, use `Game.Ops.Chain(a, b, ...)`. A chain inside a custom function may hold
`GainLife` (applied directly) plus at most one pending kind: zone moves (`SacrificeChosen` and
`DiscardChosen` together are fine), `DrawCards`, events (`DealDamageTo` and `AddCountersTo` together are
fine), or `CastSpell`. Mixing two pending kinds raises an error. TODO: carry several requests in one
operation outcome. Other declared ops (`Destroy`, `Damage`, ...) can't be returned from a custom function.

## Sacrificing/discarding at random

`Game.Ops.SacrificeRandom(filter, count)`/`Game.Ops.DiscardRandom(filter, count)` (CR 701.9b/CR "at
random") pick `count` matching permanents/cards themselves — no player choice at all, unlike
`Game.ChooseN`+`SacrificeChosen`/`DiscardChosen` above. Static values, not custom functions — set
`e:SetOperation(Game.Ops.SacrificeRandom({ card_types = { "Creature" } }, 2))` directly, same as
`Game.Ops.Destroy`/`Sacrifice`. Fewer than `count` are affected if fewer than `count` candidates exist
(CR 701.17b's mill precedent — not an error). `DiscardRandom`'s `filter` is the same shared filter
table (e.g. `Game.Ops.DiscardRandom({ card_types = { "Land" } }, 1)` for "discard a random land") — an
empty table (`{}`) matches every card in hand, same as omitting a restriction anywhere else.

## Applying an effect to each player

`Game.Ops.ForEachPlayer(fn)` (CR 101.4) invokes `fn` once per player, active player first, then each
other player in turn order — the rules' own worked example is literally "each player sacrifices a
creature":

```lua
function s.operation()
    return Game.Ops.ForEachPlayer(function(source, controller, state)
        -- controller is *this* player, not the spell's original caster -
        -- "you" inside Game.ChooseN's own filter resolves against controller
        local chosen = Game.ChooseN({ card_types = { "Creature" }, controller = "you" }, 1)
        return Game.Ops.SacrificeChosen(chosen)
    end)
end
```

Every operation function (whether wrapped in `ForEachPlayer` or not) now receives a third `state`
argument — a read-only snapshot of public game state (same shape condition/CDA scripts already get:
`state.battlefield`/`state.exile` are full card-summary arrays including a `card_types` list and
`controller`; `state.hand_count`/`state.library_count` are per-player counts, hidden zones staying
count-only per CR 400.2/402.3). This is how a `ForEachPlayer` body computes something like Balance's
own "the fewest lands any player controls" before ever asking a question — see
[card-script-basics.md](card-script-basics.md) for the full snapshot shape.

CR 101.4's "then the actions happen simultaneously": every player's own `SacrificeChosen`/
`DiscardChosen` answer is accumulated, not applied immediately — the real zone moves happen only
once every player (including one whose own body never asked anything, because they were already at
the minimum) has been invoked. A replacement effect reading the battlefield mid-loop still sees every
permanent that's *about to* leave, not one that already has.

`Game.Ops.ForEachPlayer` also accepts a bare `Game.Ops.SacrificeRandom`/`DiscardRandom`/`Mill`/
`LoseLife` value directly, with no wrapper function at all — CR 701.9b/701.21's "at random" shape has no player choice
to make, so there's no need for a script to run per player:

```lua
function s.operation()
    -- "Each player sacrifices a creature at random."
    return Game.Ops.ForEachPlayer(Game.Ops.SacrificeRandom({ card_types = { "Creature" }, controller = "you" }, 1))
end
```

Every player's own random picks are drawn first and merged into one batch before any of them actually
move, same simultaneity guarantee as the function shape above — with no coroutine involved at all,
this resolves in a single synchronous step (no `AwaitingScriptChoice` pause ever happens). Handing
`Game.Ops.ForEachPlayer` any other `Game.Ops.*` value directly (anything other than
`SacrificeRandom`/`DiscardRandom`/`Mill`/`LoseLife`) is a load-time error, not a silent no-op — wrap it in a function
instead if it needs to run per player (e.g. `Game.Ops.ForEachPlayer(function(s, c) return
Game.Ops.GainLife(1) end)`).

`Game.Ops.ForEachPlayer(Game.Ops.Mill(2))` ("each player mills two cards") and
`Game.Ops.ForEachPlayer(Game.Ops.LoseLife(1))` ("each player loses 1 life") work the same way. Every
player's top cards are merged into one mill batch, and life loss is applied to each player in APNAP
order with one `LifeChanged` event per player. Inside this wrapper they always hit every player and
ignore the ability's own target.

## What these can't do yet

- Only usable from inside an operation function — there's no equivalent for asking mid-condition
  or mid-cost.
- `Game.SelectTargets`/`Game.MayChoose`/`Game.ChooseN` are for a choice made *during resolution*.
  Choosing a mode for a modal spell ("choose one —") happens before the spell even goes on the
  stack, as part of casting it — a different mechanism entirely, and not something a card script
  drives (see [not-available-yet.md](not-available-yet.md)).
- `Game.Ops.ForEachPlayer`'s bare-tag shape only accepts `SacrificeRandom`/`DiscardRandom`/`Mill`/`LoseLife` — no
  other `Game.Ops.*` primitive can be applied per-player without a wrapping function.
