# Player modifiers

[← index](index.md)

Static effects that change a *player's* rules rather than an object's characteristics: hand size,
hand visibility, draw limits, land plays, and flash timing. Each has its own constructor and enforcement
point — they are not CR 613 layer effects, so they don't go through
[`Game.CreateStaticAbility`](static-abilities.md). All five follow the same shape:

```lua
local m = Game.CreateXModifier(c)
m:SetAffected(Game.Affected.You)   -- which player(s)
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

`:SetAffected` takes a player tag from [`Game.Affected`](game-constants.md#gameaffected), not a
filter table:

| Value | Meaning |
|---|---|
| `Game.Affected.You` | the source's controller |
| `Game.Affected.Opponent` | each opponent of the source's controller |
| `Game.Affected.Any` | every player |
| `Game.Affected.EnchantedPlayer` | the player this Aura is attached to (CR 303.4m). Hand size and hand reveal only; the draw limit and land play modifiers reject it at `:SetAffected`. |

An object tag (`Self`, `Enchanted`, `EnchantedPlayerControls`) given to a player modifier is a load error.

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
    m:SetAffected(Game.Affected.EnchantedPlayer)
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
m:SetAffected(Game.Affected.EnchantedPlayer)
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
m:SetAffected(Game.Affected.You)
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
m:SetAffected(Game.Affected.You)
m:SetAdditional(1)
m:Register()
```

Unlike the other three, land-play modifiers **stack additively**: two registered `+1` modifiers
mean three land plays per turn (CR 305.2). `EnchantedPlayer` is not available.

## `Game.CreateTimingPermission(c)` — CR 601.3b

Lets the affected player cast spells as though they had flash: with priority, on any turn, with a
non-empty stack. It is a permission, not the Flash keyword, so nothing that reads keywords sees it.
`:SetFilter(Game.Filter({...}))` is optional and limits which spells it covers; without it, every
spell. There is no magnitude setter.

```lua
-- Vedalken Orrery: you may cast spells as though they had flash.
local p = Game.CreateTimingPermission(c)
p:SetAffected(Game.Affected.You)
p:Register()
```

The permission never grants priority, and it covers only spells: land plays and Equip keep the
sorcery-speed rule. A cast from another zone through an alternative cost (flashback) is covered. A
filter with `extra` set never matches. `EnchantedPlayer` is not available.

## `Game.CreateZonePermission(c)` — CR 601.3, 305.1

Lets the affected player play lands or cast spells from a graveyard, from exile or from the top of a
library. `:SetZone` picks `"Graveyard"`, `"Exile"` or `"Library"`, and `:SetKind` picks `"PlayLand"` or
`"Cast"`. Both are required with
`:SetAffected`. `:SetOwner(Game.Affected.*)` says whose cards in that zone it covers and defaults to
`You`, so "your graveyard". `:SetFilter(Game.Filter({...}))` is optional and limits which cards it
covers.

```lua
-- Crucible of Worlds: you may play lands from your graveyard.
local p = Game.CreateZonePermission(c)
p:SetAffected(Game.Affected.You)
p:SetZone("Graveyard")
p:SetKind("PlayLand")
p:Register()
```

The permission says where, not when. A cast through it still needs the card's normal timing (or a
timing permission), and a land play still needs sorcery speed and an unused land play. A card cast or
played from another player's graveyard is controlled by the player who cast or played it. A card's
own flashback cost is separate: choosing it checks the flashback zone, and choosing the normal cost
checks the permission. `EnchantedPlayer` is not available for `:SetAffected` or `:SetOwner`.

`"Library"` needs `:SetFromLibraryTop()`, which opens only the top card of each covered library,
found again at every check (CR 401.5). Only the affected players may look at that card, unless
`:SetRevealTop()` is also called, in which case every player sees it. Once a card from the top is
cast or played, the next top card opens.

```lua
-- Future Sight: play with the top card of your library revealed. You may play lands and cast
-- spells from the top of your library.
for _, kind in ipairs({ "PlayLand", "Cast" }) do
    local p = Game.CreateZonePermission(c)
    p:SetAffected(Game.Affected.You)
    p:SetZone("Library")
    p:SetKind(kind)
    p:SetFromLibraryTop()
    p:SetRevealTop()
    p:Register()
end
```

`:SetLinked()` with `"Exile"` opens only cards exiled with the source through `Game.Ops.ExileLinked`
(CR 607.2a). The link ends when the card leaves exile or the source leaves the battlefield. A
face-down card is never offered (CR 601.3f). `:Register()` is an error when `"Library"` and
`:SetFromLibraryTop()` are not used together, when `:SetLinked()` is used with a zone other than
`"Exile"`, or when `:SetRevealTop()` is used without `:SetFromLibraryTop()`.

`:SetRider(name)` sets where a spell cast through the permission goes when it leaves the stack
(CR 608.2n). `"ExileInstead"` exiles it whether it resolves, is countered or fizzles, as flashback
does. `"ExileInsteadOfGraveyard"` exiles it only where it would go to a graveyard, so a permanent
spell still enters the battlefield, and a countered one is exiled. The rider is fixed when the spell
is cast, so it still applies after the source leaves. It is optional and needs `:SetKind("Cast")`.
When several permissions let a card be cast, one without a rider is used. A flashback cast uses the
flashback cost's own exile and ignores the permission. Buyback still returns the spell to hand.

```lua
-- Kess, Dissident Mage: during each of your turns, you may cast an instant or sorcery spell from
-- your graveyard. If a spell cast this way would be put into your graveyard, exile it instead.
local p = Game.CreateZonePermission(c)
p:SetAffected(Game.Affected.You)
p:SetZone("Graveyard")
p:SetKind("Cast")
p:SetFilter(Game.Filter({ card_types = { "Instant", "Sorcery" } }))
p:SetRider("ExileInsteadOfGraveyard")
p:Register()
```

## Timed grants — CR 611.2a

`:Register()` installs a static permission, which lasts while its source is on the battlefield. A
resolving spell or ability grants one for a stated duration instead: pass the finished builder,
without `:Register()`, to `Game.Ops.GrantTimingPermission(builder, duration)` or
`Game.Ops.GrantZonePermission(builder, duration)`. `duration` is `Game.Durations.UntilEndOfTurn`,
`UntilYourNextTurn` or `UntilEndOfCombat`, and any other value is an error (CR 611.2b). On
resolution the grant takes the resolving object as its source and that object's controller as
"you". It outlives its source and ends at cleanup, at the end of combat, or as its controller's next
turn begins (CR 514.2, 511.3).

```lua
-- {T}: You may play lands from your graveyard this turn.
function s.register_abilities(c)
    local p = Game.CreateZonePermission(c)
    p:SetAffected(Game.Affected.You)
    p:SetZone("Graveyard")
    p:SetKind("PlayLand")
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.GrantZonePermission(p, Game.Durations.UntilEndOfTurn))
    e:Register()
end
```

The builder's source id is replaced on resolution, so a spell's `s.operation()`, which gets no `c`,
may build it with any id, such as `Game.CreateTimingPermission(0)`.

A `:SetLinked()` grant still needs its source on the battlefield to keep its links, so one from a
spell matches nothing.

## Not modeled

- Hand-reveal enforcement (above).
- CR 121.2b's "can't choose to draw multiple cards" and "can't pay a cost that includes drawing
  multiple cards" — no choice-based draw or draw-cost mechanism exists.
- `EnchantedPlayer` for draw limits, land plays, timing permissions and zone permissions — no real
  card needs it.
- Conditional flash (CR 601.3d), "as though it were a main phase", and instant-speed equip.
- `Permanent`, `WhileConditionHolds` and "until the source leaves" durations for a grant (CR 611.2b).
- Impulse draw from a spell: a linked Timed grant matches nothing once its source leaves (CR 607.2a).
- Face-down linked cards (never offered, CR 601.3f), the
  stand-alone "play with the top card of your library revealed" effect without a permission
  (CR 401.5).
- Kess's "during each of your turns" and once-per-turn limits, a choice between several matching
  permissions (CR 601.2), "if it would die or leave exile this way" riders and battlefield-side exile
  such as unearth (CR 608.2n).
