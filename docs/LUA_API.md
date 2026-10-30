# Stackfish Lua API reference

This page lists every `Game.*` constructor, builder method, and table shape the Stackfish engine accepts for card scripts today. If a primitive is not listed here, it is not available yet. See [docs/lua/not-available-yet.md](lua/not-available-yet.md) for named gaps.

For how a card script is structured, read [docs/lua/card-script-basics.md](lua/card-script-basics.md). For the exact Magic rules, consult [docs/comp_rules.md](comp_rules.md). This page does not quote rules. It names the CR section that matters and the engine primitive that expresses it.

## Conventions on this page

- **Bare value**: use without calling, like `Game.Conditions.Enters`.
- **Factory**: call with arguments, like `Game.Ops.GainLife(2)`.
- **Tag**: a userdata value produced by a factory or bare value.
- A `?` after an argument means optional. Omitted arguments use the default shown.
- Tables are flat. Nested tables are rejected unless this page documents them.
- Unknown table keys, wrong-typed values, and unknown counter names are load-time errors.

## Shared schemas

### Object filter table

The same flat table is reused everywhere an object filter is needed. Only the keys an individual consumer can act on matter. The engine rejects keys it does not recognize.

```lua
{
    controller = "you",                   -- "you" | "opponent" | "any"
    card_types = { "Creature" },          -- list of card type names, matched as "or"
    excluded_card_types = { "Land" },   -- negated card_types
    subtypes = { "Human" },             -- list of subtype names, matched as "or"
    excluded_subtypes = { "Wall" },     -- negated subtypes
    supertypes = { "Legendary" },       -- list of supertype names
    excluded_supertypes = { ... },      -- negated supertypes
    zone = "Graveyard",                  -- see zone names below
    mana_value_equals = 3,               -- number or "X"
    mana_value_at_most = 3,              -- number or "X"
    mana_value_at_least = 3,             -- number or "X"
    random = true,                       -- affects which matching object is picked
}
```

Card type names: `Artifact`, `Creature`, `Enchantment`, `Instant`, `Land`, `Sorcery`, `Kindred`, `Planeswalker`, `Battle`.
Supertype names: `Basic`, `Legendary`, `Snow`, `World`.
Subtype names are free-form strings. The engine only checks the ones it knows about for keyword-granting purposes.
Zone names: `Library`, `Hand`, `Battlefield`, `Graveyard`, `Stack`, `Exile`, `Command`.

`Game.Filter({...})` wraps this table for `:SetAffected` and `:SetFilter` so the builder can tell it from a tag. Ops, costs, and condition constructors take the raw table directly.

// TODO: document unbacked filter keys once engine supports them: keywords, colors, name, other, power/toughness bounds, is_token, tapped, attacking, blocking.

### Mana strings

Mana-cost strings use curly-brace notation: `"{1}{R}"`, `"{W/U}{W/U}"`, `"{X}"`. `Game.Ops.AddMana` and `Game.Ops.AddManaChoice` use single-letter mana symbols `"W"`, `"U"`, `"B"`, `"R"`, `"G"`, `"C"` for colorless.

## `Game.Conditions.*`

Two kinds of values live here. Event tags go to `:SetCondition` on a triggered ability. State-check tags go to `:SetInterveningIf` on a triggered ability or `:SetCondition` on a static ability. Passing a value to the wrong slot is a load error.

### Event tags for `:SetCondition`

| Name | Meaning | CR anchor |
|---|---|---|
| `Enters` | When this enters the battlefield. | 603 |
| `Cast` | When you cast this spell. | 601.2i |
| `DealsDamage` | Whenever this deals damage. | 603.2 |
| `IsDealtDamage` | Whenever this is dealt damage. | 603.2 |
| `Upkeep` | At the beginning of your upkeep. | 502.1 |
| `EachUpkeep` | At the beginning of each upkeep. | 502.1 |
| `Untap` | At the beginning of your untap step. | 502.4 |
| `EachUntap` | At the beginning of each untap step. | 502.4 |
| `Dies` | When this dies. | 700.4, 603.10 |
| `LeavesBattlefield` | When this leaves the battlefield. | 603.10 |
| `DrawsCard` | Whenever you draw a card. | 121 |
| `EachDrawsCard` | Whenever a player draws a card. | 121 |
| `DiscardsCard` | Whenever you discard a card. | 603.2a |
| `EachDiscardsCard` | Whenever a player discards a card. | 603.2a |
| `Discarded` | When this card is discarded. | 603.2a |
| `Cycled` | When you cycle this card. | 702.29 |
| `MadnessExiled` | When this is exiled by madness. | 702.35 |
| `BecomesTargetOfOpponent` | When this becomes the target of an opponent's spell or ability. | 702.21a |
| `Scries` | Whenever you scry. | 701.22d |
| `EachScries` | Whenever a player scries. | 701.22d |
| `Surveils` | Whenever you surveil. | 701.25d |
| `EachSurveils` | Whenever a player surveils. | 701.25d |
| `BeginningOfCombat` | At the beginning of combat on your turn. | 507 |
| `EachBeginningOfCombat` | At the beginning of each combat. | 507 |
| `EndStep` | At the beginning of your end step. | 512/513 |
| `EachEndStep` | At the beginning of each end step. | 512/513 |

### Event factories for `:SetCondition`

```lua
Game.Conditions.DiesMatching(filter)
Game.Conditions.LeavesBattlefieldMatching(filter)
Game.Conditions.OncePerTurn(condition)
```

`filter` is the shared object filter table. `OncePerTurn` wraps any other `Game.Conditions.*` event value.

```lua
-- "Whenever a creature you control dies"
e:SetCondition(Game.Conditions.DiesMatching({ controller = "you", card_types = { "Creature" } }))

-- "This ability triggers only once each turn."
e:SetCondition(Game.Conditions.OncePerTurn(Game.Conditions.DealsDamage))
```

### State-check tags for `:SetInterveningIf` or static `:SetCondition`

```lua
Game.Conditions.AtOrBelowLife(threshold)
Game.Conditions.ControlsAtLeast(filter, count)
Game.Conditions.Escaped
Game.Conditions.HadNoCounters(counter)
```

`counter` is a known counter kind string or a `Game.Counter.*` value.

## `Game.Affected.*`

Use these with `:SetAffected` on static abilities and replacement effects. Player tags go to player-modifier builders instead.

| Name | Accepted by | Meaning |
|---|---|---|
| `Self` | Static, replacement | Only the ability's own source. |
| `Enchanted` | Static | The object this Aura is attached to. |
| `EnchantedPlayerControls` | Static | Every permanent the enchanted player controls. |
| `You` | Player modifier | The controller. |
| `Opponent` | Player modifier | Each opponent. |
| `Any` | Player modifier | Every player. |
| `EnchantedPlayer` | Player modifier | The player this Aura is attached to. |

## `Game.Filter(table)`

Wraps the shared object filter table for `:SetAffected` and `:SetFilter`.

```lua
e:SetAffected(Game.Filter({ controller = "you", card_types = { "Creature" } }))
```

`self_only`, `enchanted_only`, and `enchanted_player_controls` are errors here. Use `Game.Affected.*` instead.

## `Game.Counter.*`

Common counter kinds and a custom constructor. Use these anywhere a counter is expected.

| Name | Meaning |
|---|---|
| `PlusOnePlusOne` | +1/+1 |
| `MinusOneMinusOne` | -1/-1 |
| `Loyalty` | Loyalty |
| `Poison` | Poison |
| `Energy` | Energy |
| `Experience` | Experience |
| `Charge` | Charge |

```lua
Game.Counter.Custom(name)
```

## `Game.Ops.*`

Resolution operations. Return these from `:SetOperation` or a spell's `s.operation()`.

### Mana and life

```lua
Game.Ops.AddMana(letter, count?)
Game.Ops.AddManaChoice(colors, count?)
Game.Ops.AddRestrictedMana(letter, cardType, count?)
Game.Ops.GainLife(amount)
Game.Ops.LoseLife(amount)
```

### Damage and destruction

```lua
Game.Ops.Damage(amount)
Game.Ops.Destroy
Game.Ops.DestroyAll(filter)
Game.Ops.BecomeBlocked
Game.Ops.Sacrifice
Game.Ops.SacrificeSelf
Game.Ops.Exile
Game.Ops.ExileSelf
Game.Ops.ExileAll(filter, zone?)
Game.Ops.ExileUntilSourceLeaves
Game.Ops.Regenerate
```

### Zone moves

```lua
Game.Ops.ReturnToHand
Game.Ops.ReturnSelfToHand
Game.Ops.ReturnAllToHand(filter, zone?)
Game.Ops.ReturnToBattlefield(opts?)
Game.Ops.ReturnAllToBattlefield(filter, zone?, opts?)
```

`opts` is `{ control = "you" | "owner", tapped = bool }`, default `{ control = "you", tapped = false }`.

### Tapping

```lua
Game.Ops.Tap
Game.Ops.Untap
Game.Ops.TapSelf
Game.Ops.UntapSelf
Game.Ops.TapAll(filter)
Game.Ops.UntapAll(filter)
```

### Counters and power/toughness changes

```lua
Game.Ops.Counters(counter, amount)
Game.Ops.ReturnSelfWithCounter(counter)
Game.Ops.Pump(power, toughness, duration)
Game.Ops.GainControl(duration)
```

`duration` is a `Game.Durations.*` value.

### Library and hand

```lua
Game.Ops.Mill(n)
Game.Ops.Discard(count)
Game.Ops.DiscardRandom(filter, count)
Game.Ops.DrawCards(amount)
Game.Ops.Search(filter, count, opts?)
Game.Ops.Shuffle
Game.Ops.Scry(n)
Game.Ops.Surveil(n)
```

`Search` `opts`: `{ destination = "Hand" | "Battlefield", tapped = bool, enters_attached = bool, shuffle = bool }`, default `{ destination = "Hand", shuffle = true }`.

### Countering and copying

```lua
Game.Ops.Counter
Game.Ops.CounterUnlessPaid(manaCost)
Game.Ops.BecomeCopy(exceptions?)
```

// TODO: document X-spell/cost syntax once 031 implements X targeting and payment.

### Combat and attachments

```lua
Game.Ops.Attach
Game.Ops.Fight
Game.Ops.Bite
Game.Ops.BecomeBlocked
Game.Ops.ExchangeTextBoxes
```

### Alternative costs and cast effects

```lua
Game.Ops.MadnessCastOrDiscard
Game.Ops.Cascade
```

### Chaining and player-wide effects

```lua
Game.Ops.Chain(a, b, ...)
Game.Ops.ChainEach({ op, words }, ...)
Game.Ops.ForEachPlayer(x)
```

`Chain` runs several operations in order. `ChainEach` assigns target words to entries. `ForEachPlayer` takes a function or a bare `SacrificeRandom`, `DiscardRandom`, `Mill`, or `LoseLife` value.

### Mid-resolution choices

```lua
Game.Ops.SacrificeChosen(list)
Game.Ops.DiscardChosen(list)
Game.Ops.CastSpell(card, opts?)
Game.Ops.DealDamageTo(pick, amount)
Game.Ops.AddCountersTo(pick, counter, amount)
```

`list` is the result of `Game.ChooseN`. `pick` is a `{ kind = "object" | "player" | "ability", id = ... }` table from `Game.SelectTargets` or `Game.ChooseN`.

### Tokens

```lua
Game.Ops.CreateToken(spec, count?)
```

`spec` is a `Game.Tokens.*` value. `count` defaults to 1.

## `Game.Costs.*`

Use these with `:AddCost` on activated abilities. Call once per component.

```lua
Game.Costs.TapSelf
Game.Costs.Sacrifice(filter)
Game.Costs.SacrificeSelf
Game.Costs.Discard(filter)
Game.Costs.DiscardSelf
Game.Costs.TapPermanents(filter, count)
Game.Costs.ExileFromGraveyard(filter, count)
Game.Costs.Life(amount)
Game.Costs.Mana(cost)
```

`Mana(cost)` takes a mana-cost string such as `"{1}{R}"`. An ability may have at most one mana component.

## `Game.Effects.*`

Use these with `:SetEffect` on static abilities. Each primitive only pairs with one layer. See `Game.Layers.*`.

```lua
Game.Effects.SetName(name)
Game.Effects.SetPT(power, toughness)
Game.Effects.ModifyPT(power, toughness)
Game.Effects.DoublePT(power, toughness)
Game.Effects.AddKeyword(name_or_list)
Game.Effects.RemoveKeyword(name)
Game.Effects.IgnoreKeyword(name)
Game.Effects.AddProtectionColor(letter)
Game.Effects.AddProtectionCardType(name)
Game.Effects.AddProtectionSubtype(name)
Game.Effects.AddProtectionName(name)
Game.Effects.AddProtectionPlayer(who)
Game.Effects.AddProtectionEverything
Game.Effects.AddLandwalkSubtype(name)
Game.Effects.AddLandwalkQuality(opts)
Game.Effects.AddBandsWithOther(subtype)
Game.Effects.AddBandsWithOtherSupertype(name)
Game.Effects.AddCardType(name)
Game.Effects.SetCardTypes(names)
Game.Effects.AddColor(letter)
Game.Effects.SetColors(letters)
Game.Effects.AddSupertype(name)
Game.Effects.AddSubtype(name)
Game.Effects.SwitchPT
Game.Effects.RemoveAllAbilities
```

`AddKeyword` accepts a single keyword name string or a list. Keyword names: `Flying`, `Reach`, `Menace`, `Vigilance`, `Haste`, `Defender`, `Flash`, `Deathtouch`, `Indestructible`, `Lifelink`, `Wither`, `Infect`, `FirstStrike`, `DoubleStrike`, `Trample`, `Fear`, `Intimidate`, `Shadow`, `Horsemanship`, `Hexproof`, `Shroud`, `AttacksEachCombat`, `CantBeRegenerated`.

## `Game.Layers.*`

Use these with `:SetLayer` on static abilities. These are bare values only.

| Name | Layer | Pairs with |
|---|---|---|
| `TextChanging` | 3 | `SetName` |
| `TypeChanging` | 4 | `AddCardType`, `SetCardTypes`, `AddSupertype`, `AddSubtype` |
| `ColorChanging` | 5 | `AddColor`, `SetColors` |
| `AbilityAdding` | 6 | `AddKeyword`, `RemoveKeyword`, `IgnoreKeyword`, `AddProtection*`, `AddLandwalk*`, `AddBandsWithOther*`, `RemoveAllAbilities` |
| `PowerToughnessSetting` | 7b | `SetPT` |
| `PowerToughnessModifying` | 7c | `ModifyPT`, `DoublePT` |
| `PowerToughnessSwitching` | 7e | `SwitchPT` |

## `Game.Durations.*`

Use these with `:SetDuration` on static abilities or as the `duration` argument to `Game.Ops.Pump`/`GainControl`.

| Name | Meaning |
|---|---|
| `UntilEndOfTurn` | Until end of current turn. |
| `UntilYourNextTurn` | Until your next turn begins. |
| `UntilEndOfCombat` | Until end of current combat. |
| `Permanent` | Never expires. |

## `Game.EventFilters.*`

Use these with `:SetFilter` on replacement effects.

```lua
Game.EventFilters.DamageDealt
Game.EventFilters.WouldDie
Game.EventFilters.WouldMoveZone(from?, to?)
Game.EventFilters.WouldDieEnchanted
Game.EventFilters.WouldEnterEnchantedPlayerGraveyard
```

Either `from` or `to` may be `nil` for "any zone."

## `Game.ReplacementPayloads.*`

Use these with `:SetPayload` on replacement effects.

```lua
Game.ReplacementPayloads.PreventDamage
Game.ReplacementPayloads.Regenerate
Game.ReplacementPayloads.UmbraArmor
Game.ReplacementPayloads.RedirectDamageToSource
Game.ReplacementPayloads.ReplaceZoneDestination(zone)
Game.ReplacementPayloads.EntersTapped
Game.ReplacementPayloads.EntersWithCounters(counter, amount)
Game.ReplacementPayloads.EntersWithCountersBloodthirst(counter, amount)
Game.ReplacementPayloads.EntersWithCountersEscape(counter, amount)
Game.ReplacementPayloads.EntersWithCountersSunburst
Game.ReplacementPayloads.EntersWithCountersDevour(perCreature, quality?)
Game.ReplacementPayloads.EntersWithCountersAmplify(perCard)
Game.ReplacementPayloads.EntersAsCopy(exceptions?)
```

## `Game.Consumption.*`

Use these with `:SetConsumption` on replacement effects.

```lua
Game.Consumption.OneShot
Game.Consumption.Repeatable
Game.Consumption.LimitedUses(n)
```

## `Game.Tokens.*`

Token specs, passed to `Game.Ops.CreateToken`.

```lua
Game.Tokens.Clue
Game.Tokens.Food
Game.Tokens.Treasure
Game.Tokens.Creature(subtypes, colors, power, toughness, opts?)
Game.Tokens.CopyOf(objectId, opts?)
```

`Creature` `subtypes` may be a single string or a list. `colors` is a WUBRG/C string. `opts` is `{ keywords = {...}, name = "...", additional_types = {...}, additional_supertypes = {...} }`.

## Ability builders

All builders take the source object id and register with `:Register()`.

```lua
Game.CreateTriggeredAbility(source)
Game.CreateStaticAbility(source)
Game.CreateActivatedAbility(source)
Game.CreateManaAbility(source)
Game.CreateReplacementEffect(source)
Game.CreateCostModifier(source)
Game.CreateHandSizeModifier(source)
Game.CreateHandRevealModifier(source)
Game.CreateDrawLimitModifier(source)
Game.CreateLandPlayModifier(source)
```

### `Game.CreateTriggeredAbility`

```lua
e:SetFunctionsFrom(zone)
e:SetCondition(game_conditions_value)
e:SetInterveningIf(state_condition_value)
e:SetOperation(game_ops_value)
e:SetTarget(target_spec)
e:Register()
```

### `Game.HandSizeEffects.*`

Used with `:SetEffect` on `Game.CreateHandSizeModifier`.

```lua
Game.HandSizeEffects.NoMaximum
Game.HandSizeEffects.SetTo(n)
```

### `Game.CreateStaticAbility`

```lua
e:SetFunctionsFrom(zone)
e:SetLayer(game_layers_value)
e:SetEffect(game_effects_value)
e:SetAffected(game_affected_value | Game.Filter(...))
e:SetCondition(state_condition_value)
e:SetDuration(game_durations_value)
e:SetCharacteristicDefining()
e:Register()
```

### `Game.CreateActivatedAbility`

```lua
e:SetFunctionsFrom(zone)
e:AddCost(game_costs_value)
e:SetOperation(game_ops_value)
e:SetTarget(target_spec)
e:Register()
```

### `Game.CreateReplacementEffect`

```lua
e:SetFilter(game_event_filters_value)
e:SetPayload(game_replacement_payloads_value)
e:SetAffected(Game.Affected.Self | Game.Filter(...))
e:SetConsumption(game_consumption_value)
e:SetFunctionsFrom(zone)
e:Register()
```

## Target specification

A spell declares its targets with `s.target_requirement()`:

```lua
function s.target_requirement()
    return {
        type = "object",            -- "object" | "player" | "any" | "ability" | "triggering"
        count = 1,
        up_to = false,
        at_least = false,
        exclude_source = false,
        -- plus any object filter keys
    }
end
```

Two target words return an array of requirement tables. A `filter` key may hold a function for scripted predicates.

## Mid-resolution choices

These only work inside a custom operation function because they suspend the Lua coroutine.

```lua
Game.SelectTargets(filter)          -- returns one { kind, id } pick
Game.MayChoose()                    -- returns true or false
Game.ChooseN(filter, n, opts?)       -- returns an array of picks
```

`ChooseN` `opts`: `{ up_to = bool, at_least = bool }`.

## Sugar

```lua
Game.RegisterKeyword(source, name)
Game.RegisterWard(source, cost)
```

`RegisterKeyword` grants the source a printed keyword as a layer-6 self-only static ability. `RegisterWard` registers one Ward instance. `cost` is a mana-cost string.

## Strict validation rules

- Unknown keys in any engine-decoded table raise a load error.
- Wrong-typed values raise a load error.
- Unknown counter kind strings raise a load error. Use `Game.Counter.Custom(name)` for flavor counters.
- A bare `Game.Ops.*` factory passed uncalled is detected and rejected.
- A raw table in `:SetAffected` is rejected. Wrap it in `Game.Filter(...)`.
- Operations must return a `Game.Ops.*` or `Game.Effects.*` value, or `nil` to mean "do nothing." Raw-key tables are rejected.
- Multi-op results in callbacks must use `Game.Ops.Chain` or `Game.Ops.ChainEach`.

## Migration from the old API

Forms removed or renamed in the 052/061/062/063 cleanup:

- Raw-key result tables are rejected. Use `Game.Ops.*` and `Game.Effects.*` constructors.
- `Game.InterveningIf` is gone. Intervening-if values live in `Game.Conditions`.
- The four `*Affected` tables are collapsed into `Game.Affected.*`.
- `Conditions.Always` is removed. Omit `:SetCondition` for an unconditional trigger.
- `Tokens.ArtifactCreature` is removed. Use `Game.Tokens.Creature(...)` with `additional_types`.
- Nested `filter = { ... }` in target tables is rejected. Put filter keys flat beside `count`/`up_to`/etc.
- The `AddMana`/`AddManaChoice` argument order is `(letter(s), count?)`.
- Parameter names are standardized: `count`, `amount`, `counter`.

## Common mistakes

```lua
-- Wrong: uncalled factory
return Game.Ops.Mill
-- Right:
return Game.Ops.Mill(2)

-- Wrong: raw table in SetAffected
e:SetAffected({ card_types = { "Creature" } })
-- Right:
e:SetAffected(Game.Filter({ card_types = { "Creature" } }))

-- Wrong: nested filter table
return { count = 1, filter = { card_types = { "Creature" } } }
-- Right:
return { count = 1, card_types = { "Creature" } }

-- Wrong: unknown counter string
return Game.Ops.Counters("Flood", 1)
-- Right:
return Game.Ops.Counters(Game.Counter.Custom("Flood"), 1)
```
