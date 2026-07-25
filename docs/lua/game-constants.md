# Game constants reference

[← index](index.md)

Every `Game.*` tag and factory function, organized by table. See
[card-script-basics.md](card-script-basics.md#primitive-vs-custom-function) for the general
primitive-vs-custom-function pattern these all follow, and the per-kind pages
([triggered](triggered-abilities.md), [static](static-abilities.md),
[activated](activated-abilities.md), [replacement](replacement-effects.md),
[cost modifier](cost-modifiers.md)) for how each table is actually consumed by a builder.

A **bare value** is used directly, no call: `Game.Conditions.Always`. A **factory** is called with
arguments to produce a value: `Game.Ops.GainLife(1)`.

## `Game.Conditions.*`

Used with `:SetCondition` on a [triggered ability](triggered-abilities.md). Answers "did this
ability's triggering event just happen" — CR 603's trigger-event condition, checked once per
matching event.

### Bare tags

| Name | Meaning |
|---|---|
| `Enters` | CR 603, "when this enters the battlefield." |
| `Always` | No filter — matches every event. Write this explicitly for an unconditional trigger; don't rely on a typo/`nil` meaning the same thing. |
| `DealsDamage` | CR 603.2, "whenever this deals damage" — matches any damage this object deals, combat or otherwise. |
| `IsDealtDamage` | "Whenever this is dealt damage." Object targets only — a player being dealt damage never satisfies this. |
| `Upkeep` | CR 502.1, "at the beginning of your upkeep." |
| `EachUpkeep` | CR 502.1, "at the beginning of each upkeep" (either player's). |
| `Untap` | CR 502.4, "at the beginning of your untap step." Note: the untap step grants no priority, so an ability that triggers here just waits for the next priority window before it can go on the stack. |
| `EachUntap` | "At the beginning of each untap step." |
| `Dies` | CR 700.4/603.10, "when this dies" (goes from the battlefield to a graveyard). |
| `LeavesBattlefield` | "When this leaves the battlefield," any destination (not just the graveyard). |
| `DrawsCard` | CR 121, "whenever you draw a card." |
| `EachDrawsCard` | "Whenever a player draws a card," any player. |
| `DiscardsCard` | "Whenever you discard a card." |
| `EachDiscardsCard` | "Whenever a player discards a card," any player. |
| `Discarded` | "When this card is discarded" (self-referential — contrast `DiscardsCard`, which is about *you* discarding *any* card). |
| `Scries` | CR 701.22d, "whenever you scry." |
| `EachScries` | "Whenever a player scries," any player. |
| `Surveils` | CR 701.25d, "whenever you surveil." |
| `EachSurveils` | "Whenever a player surveils," any player. |
| `BeginningOfCombat` | CR 507, "at the beginning of combat on your turn." |
| `EachBeginningOfCombat` | "At the beginning of combat," any player's turn. |
| `EndStep` | CR 512/513, "at the beginning of your end step." |
| `EachEndStep` | "At the beginning of each end step," any player's turn. |

### Factories

**`Game.Conditions.DiesMatching(filter)`** — CR 700.4/603.10, "whenever a creature you control
dies" (or any other filtered death). `filter` is the shared filter-table shape — see
[targeting-and-choices.md#filter-table-shape](targeting-and-choices.md#filter-table-shape) for its
full key list (`controller`, `card_types`, `supertypes`, `subtypes`, `random`). Unlike the bare
`Dies` tag (which only matches this ability's own source), matches any qualifying death on the
battlefield — `{ controller = "you", card_types = { "Creature" } }` is Blood Artist's own trigger.

**`Game.Conditions.LeavesBattlefieldMatching(filter)`** — filtered sibling of `LeavesBattlefield`,
same filter-table shape as `DiesMatching`, matches any zone change off the battlefield (not just
to a graveyard) for a matching object.

**`Game.Conditions.OncePerTurn(condition)`** — CR 603.2h, "this ability triggers only once each
turn." Wraps any other `Game.Conditions.*` value (bare tag or a `DiesMatching`/
`LeavesBattlefieldMatching` result):

```lua
e:SetCondition(Game.Conditions.OncePerTurn(Game.Conditions.DealsDamage))
```

The wrapped condition still has to genuinely match — `OncePerTurn` just additionally requires that
this exact ability hasn't already triggered once already this turn. Resets at the start of each
new turn. Not available for a custom-function condition — only wraps a `Game.Conditions.*` value.

## `Game.InterveningIf.*`

Used with `:SetInterveningIf` on a [triggered ability](triggered-abilities.md), or `:SetCondition`
on a [static ability](static-abilities.md) (CR 611.2c, "this effect applies only as long as X is
true"). A pure board-state predicate — no triggering event involved, checked fresh whenever it's
asked.

**`Game.InterveningIf.AtOrBelowLife(threshold)`** — true iff the ability's controller's current
life total is at or below `threshold`. E.g. "at the beginning of your upkeep, if you have 10 or
less life..." is `Game.InterveningIf.AtOrBelowLife(10)`.

**`Game.InterveningIf.ControlsAtLeast(filter, count)`** — CR 611.2c, "as long as you control
`count` or more permanents matching `filter`." `filter` is the same filter-table shape as
`DiesMatching` above.

A custom function is also accepted anywhere a `Game.InterveningIf.*` value is — see
[triggered-abilities.md](triggered-abilities.md) and
[static-abilities.md](static-abilities.md) for the call signature.

## `Game.Ops.*`

Used with `:SetOperation` on a [triggered](triggered-abilities.md) or
[activated](activated-abilities.md) ability. Describes what the ability actually does when it
resolves.

| Name | Args | Meaning |
|---|---|---|
| `AddMana(letter, count?)` | `letter`: a single mana-color letter (`"W"`/`"U"`/`"B"`/`"R"`/`"G"`) or `"C"` for colorless. `count`: how many, default 1. | CR 605, adds exactly this fixed mana to the activator's pool. No mixed-color-in-one-call shape yet ("add one white and one blue" needs two calls or `AddManaChoice`). |
| `AddManaChoice(count, colors)` | `count`: how many mana. `colors`: a string of allowed letters, e.g. `"WU"`. | CR 605, "add `count` mana of any of these colors, your choice" — the player picks the actual color(s) when activating. |
| `GainLife(amount)` | integer | CR 119, controller gains exactly `amount` life. Also the one key a custom operation function's returned table can use (`{ gain_life = amount }`) — see [card-script-basics.md](card-script-basics.md#writing-an-operation). |
| `Damage(amount)` | integer (unsigned) | CR 608.2m, deals exactly `amount` damage to the ability's target. Goes through the normal damage pipeline, so prevention/replacement effects and Lifelink/Wither/Infect/Deathtouch all apply normally. Single fixed target only — no "divide among any number of targets" or "each opponent" shape yet. |
| `Destroy` | — | CR 701.8, destroys the ability's target. Indestructible targets are unaffected. |
| `Sacrifice` | — | CR 701.17, sacrifices the ability's target (must be something the caster/controller controls). Unlike `Destroy`, indestructibility doesn't stop this. Not to be confused with `Game.Costs.Sacrifice` — that's a *cost* (pick any matching permanent you control, no targeting), this is a resolution effect against a chosen target. |
| `Exile` | — | CR 701.18, exiles the ability's target. Ignores indestructibility. Battlefield targets only for now (exiling a card from a graveyard isn't wired up yet). |
| `Counters(kind, amount)` | `kind`: counter kind name, e.g. `"PlusOnePlusOne"`, `"MinusOneMinusOne"`, `"Loyalty"`, `"Poison"`, `"Energy"`, `"Experience"`, `"Charge"`; any other string becomes a custom counter kind with that name. `amount`: integer. | CR 122.1a, puts `amount` counters of `kind` on the ability's target. Object targets only — no player-facing counters (Energy/Experience on a player) yet. |
| `Pump(power, toughness, duration)` | `power`/`toughness`: integers (can be negative). `duration`: a `Game.Durations.*` value. | "Target creature gets +X/+Y until `duration`." Every legal target gets its own independent effect. |
| `GainControl(duration)` | `duration`: a `Game.Durations.*` value. | CR 108.4/613.1, "gain control of target permanent until `duration`" (Threaten-style). The new controller is always whoever controls the resolving spell/ability — there's no separate "give control to a specific player" argument. Only covers a resolving spell/ability's own one-shot control change; a static "you control enchanted creature" aura effect isn't built yet. |
| `Regenerate` | — | CR 701.19a, gives the ability's target a one-shot regeneration shield. A target that can't be regenerated gets no shield (silently) rather than erroring. |
| `DrawCards(amount)` | integer | CR 120.3/121, the ability's controller draws exactly `amount` cards. Always the controller — no "target player draws" shape yet. Drawing from an empty library currently just stops, rather than being tracked as a loss condition. |
| `CreateToken(spec, count?)` | `spec`: a `Game.Tokens.*` value — see [tokens.md](tokens.md). `count`: how many copies, default 1. | CR 111, creates one or more tokens under the controller's control. |
| `Shuffle` | — | CR 701.19b (mechanic), the resolving controller shuffles their own library. No "target player shuffles" shape yet. |
| `Scry(n)` | integer | CR 701.22a, look at the top `n` cards of your library, then put any number on the bottom (any order) and the rest back on top (any order). `n = 0` (or an empty library) is a true no-op. |
| `Surveil(n)` | integer | CR 701.25a, look at the top `n` cards of your library, then put any number into your graveyard and the rest back on top (any order). `n = 0` is a no-op. |
| `Search(filter, count, opts?)` | `filter`: the shared filter-table shape (see [targeting-and-choices.md](targeting-and-choices.md#filter-table-shape)). `count`: max cards to find (never mandatory — finding fewer, including zero, is legal). `opts`: `{ destination = "hand"\|"battlefield", tapped = bool, enters_attached = bool, shuffle = bool }`, all optional (`destination` defaults `"hand"`, `shuffle` defaults **true**). | CR 701.23a, search your library for up to `count` cards matching `filter`, put them in `destination`, then shuffle unless `shuffle = false`. `tapped`/`enters_attached` only matter when `destination = "battlefield"`. Revealing the found card, redirecting the search to only the top N cards, and simultaneous multiplayer search aren't modeled yet. |
| `BecomeCopy(exceptions?)` | `exceptions`: optional table, see below | CR 707.2/707.3, the ability's source becomes a copy of its target's current copiable values (e.g. Vesuvan Doppelganger). Never changes zones, so no ETB/leaves-battlefield triggers fire from becoming a copy. |
| `Attach` | — | CR 702.6a (Equip), attaches the ability's source to its target. "Activate only as a sorcery" is automatically enforced for every `Attach`-using ability — you don't need to opt in separately. |
| `ExchangeTextBoxes` | — | CR 612.5, exchanges the "text box" (rules text, keywords, and script — not name/mana cost/color/power/toughness/types) between the ability's first two targets. If either target becomes illegal by resolution, the whole effect fizzles for both. |

`BecomeCopy`'s `exceptions` table (also used by `Game.Tokens.CopyOf` — see
[tokens.md](tokens.md)):

```lua
Game.Ops.BecomeCopy({
    power = 4, toughness = 4,        -- CR 707.9b: keep these instead of copying them (must set both together)
    keywords = { "Flying" },         -- CR 707.9a: keep these keywords in addition to the copied ones
    retain_name = true,              -- CR 707.9c: keep this object's own name instead of the copied one
    retain_color = true,             -- CR 707.9d: keep this object's own color instead of the copied one
})
```

Omit the table entirely (or pass nothing) for a bare, unmodified copy.

## `Game.Costs.*`

Used with `:AddCost` on an [activated ability](activated-abilities.md) — call it once per cost
component, since one activation can need several at once (`{T}, Sacrifice a creature:` is two
calls).

| Name | Args | Meaning |
|---|---|---|
| `TapSelf` | — | CR 602.5a, the `{T}` symbol — taps whichever permanent is activating the ability. Always the source, never a choice. |
| `Sacrifice(filter)` | `filter`: shared filter-table shape | CR 701.20a, sacrifice a permanent you currently control matching `filter`. |
| `Discard(filter)` | `filter`: shared filter-table shape | Discard a card from your hand matching `filter`. |
| `TapPermanents(filter, count)` | `filter`: shared filter-table shape. `count`: how many. | Tap `count` *other* permanents (not the source) matching `filter` — distinct from `TapSelf`. |
| `Life(amount)` | integer | CR 119.4, pay exactly `amount` life. You can't pay more life than you have. |
| `Mana(cost)` | a mana-cost string, e.g. `"{1}{R}"` | The mana-cost component of the activation cost. Full hybrid/Phyrexian/generic parsing per the normal mana-cost grammar. At most one `Mana(...)` per ability. |

There's no `Game.Costs.Other` for an unstructured, free-text cost — every cost component has to be
one of the structured shapes above.

## `Game.Layers.*`

Used with `:SetLayer` on a [static ability](static-abilities.md) — which CR 613.1 layer this
effect applies in. Bare values only:

| Name | CR layer | Pairs with (`Game.Effects.*`) |
|---|---|---|
| `TextChanging` | 3 | `SetName` |
| `TypeChanging` | 4 | `AddCardType` |
| `ColorChanging` | 5 | `AddColor` |
| `AbilityAdding` | 6 | `AddKeyword`, every `AddProtection*`, `AddLandwalk*`, `AddBandsWithOther`, `RemoveAllAbilities` |
| `PowerToughnessSetting` | 7b | `SetPT` |
| `PowerToughnessModifying` | 7c | `ModifyPT` |
| `PowerToughnessSwitching` | 7e | `SwitchPT` |

Only these 7 layers are currently supported (copy effects, control-changing, and the two
characteristic-defining/counter sub-layers of layer 7 aren't available as a static-ability layer —
`GainControl`/`BecomeCopy` are resolution-time [operations](#gameops), not static effects, and
counters are handled separately from layers entirely).

## `Game.Effects.*`

Used with `:SetEffect` on a [static ability](static-abilities.md) — what the effect actually does.
Each one only pairs with the layer listed in the table above; registering a mismatched
layer/effect pair is a real error.

| Name | Args | Meaning |
|---|---|---|
| `SetName(name)` | string | CR 612.8, overrides the name outright. |
| `SetPT(power, toughness)` | integers | Overrides power/toughness outright. |
| `ModifyPT(power, toughness)` | integers (can be negative) | Adds this delta to power/toughness. |
| `AddKeyword(name)` | keyword name string | Grants a keyword ability. |
| `AddProtectionColor(letter)` | mana-color letter | CR 702.16, protection from that color. |
| `AddProtectionCardType(name)` | card type name | Protection from that card type (e.g. "protection from artifacts"). |
| `AddProtectionSubtype(name)` | subtype name | CR 702.16g, protection from that creature type. One call per subtype (write "protection from A and from B" as two separate registered effects). |
| `AddProtectionName(name)` | a card name | CR 702.16a, protection from a specific named card. Matched against the object's current (possibly changed) name. |
| `AddProtectionPlayer(who)` | `"you"` or `"opponent"` | CR 702.16k, protection from a player, relative to whichever player controls the *protected* object. `"any"` isn't valid here — use `AddProtectionEverything` for "protection from everything." |
| `AddProtectionEverything` | — | CR 702.16j, protection regardless of any characteristic — no quality to name. |
| `AddLandwalkSubtype(name)` | land subtype, e.g. `"Island"` | CR 702.14, plain landwalk (covers the 5 basic landwalks). |
| `AddLandwalkQuality(opts)` | table — see [static-abilities.md](static-abilities.md) | CR 702.14a's combination forms ("nonbasic landwalk," "snow swampwalk," etc.) |
| `AddBandsWithOther(subtype)` | subtype name | CR 702.22b/c, "bands with other [subtype]." |
| `AddCardType(name)` | card type name | Adds a card type (additive only — can't remove one). |
| `AddColor(letter)` | mana-color letter | Adds a color (additive only). |
| `SwitchPT` | — | Swaps power and toughness. |
| `RemoveAllAbilities` | — | CR 613.1f, strips every keyword and protection quality accumulated so far. A later effect with a higher timestamp that re-adds something still applies on top. |

A custom function is also accepted in place of any `Game.Effects.*` value — see
[static-abilities.md](static-abilities.md) for its call signature (it returns a table describing
the effect shape instead of using a tag).

## `Game.Durations.*`

Used with `:SetDuration` on a [static ability](static-abilities.md), or as the `duration` argument
to `Game.Ops.Pump`/`Game.Ops.GainControl`. Bare values only:

| Name | Meaning |
|---|---|
| `UntilEndOfTurn` | Cleans up at the end of the current turn. |
| `UntilYourNextTurn` | Cleans up right before your next turn begins. |
| `UntilEndOfCombat` | Cleans up at the end of the current combat. |
| `Permanent` | Never expires on its own (the default if `:SetDuration` is never called). |

## `Game.EventFilters.*`

Used with `:SetFilter` on a [replacement effect](replacement-effects.md) — which event shape this
effect watches for.

| Name | Args | Meaning |
|---|---|---|
| `DamageDealt` | — | Matches damage being dealt. Pairs with `PreventDamage`/`RedirectDamageToSource`. |
| `WouldDie` | — | CR 700.4, matches "this would die" (battlefield → graveyard specifically, any cause). Pairs with `Regenerate` or `ReplaceZoneDestination`. |
| `WouldMoveZone(from?, to?)` | zone names, either or both may be omitted/`nil` | The general zone-move filter. Either side omitted means "any zone" on that side — `WouldMoveZone("Battlefield", nil)` matches any move off the battlefield, `WouldMoveZone(nil, "Exile")` matches any move into exile regardless of source. Zone names: `"Library"`, `"Hand"`, `"Battlefield"`, `"Graveyard"`, `"Stack"`, `"Exile"`, `"Command"`. |

## `Game.ReplacementPayloads.*`

Used with `:SetPayload` on a [replacement effect](replacement-effects.md) — the actual
substitution. Each only pairs with the filter(s) noted; see
[replacement-effects.md](replacement-effects.md) for full worked examples of each.

| Name | Args | Pairs with | Meaning |
|---|---|---|---|
| `PreventDamage` | — | `DamageDealt` | CR 615, the matched damage doesn't happen. |
| `RedirectDamageToSource` | — | `DamageDealt` | CR 614.9, the matched damage is dealt to this effect's own source instead (Pariah-style). Redirecting to some other chosen target isn't supported yet — only "to this permanent" is. |
| `Regenerate` | — | `WouldDie` | CR 701.19b, a repeatable regeneration shield ("each time this would die," not just the next time — contrast the one-shot `Game.Ops.Regenerate` operation). |
| `ReplaceZoneDestination(zone)` | a zone name string | `WouldDie` or `WouldMoveZone` | CR 614.13-style "instead" substitution — the object goes to `zone` instead of its original destination. |
| `EntersTapped` | — | `WouldMoveZone(_, "Battlefield")` | CR 603.6d, the permanent enters tapped. |
| `EntersWithCounters(kind, amount)` | counter kind name, integer | `WouldMoveZone(_, "Battlefield")` | The permanent enters with this many counters already on it (e.g. Modular). |
| `EntersWithCountersBloodthirst(kind, amount)` | counter kind name, integer | `WouldMoveZone(_, "Battlefield")` | CR 702.54a Bloodthirst — the counters are only added if an opponent was dealt damage this turn (checked automatically, no separate condition needed). |
| `EntersWithCountersSunburst` | — | `WouldMoveZone(_, "Battlefield")` | CR 702.44 Sunburst — kind and amount are derived automatically from the mana spent to cast this permanent. |
| `EntersWithCountersDevour(perCreature)` | integer | `WouldMoveZone(_, "Battlefield")` | CR 702.82a Devour — this permanent's controller is asked, as it enters, how many creatures to sacrifice; it enters with `perCreature` × (creatures sacrificed) counters. "Devour [quality] N" (sacrificing only permanents of a named quality, not just any creature) isn't modeled yet. |
| `EntersWithCountersAmplify(perCard)` | integer | `WouldMoveZone(_, "Battlefield")` | CR 702.38a Amplify — the controller is asked to reveal matching cards from hand as it enters; it enters with `perCard` × (cards revealed) counters. A single creature having multiple independent Amplify instances isn't modeled yet. |
| `EntersAsCopy(exceptions?)` | same `exceptions` table as `Game.Ops.BecomeCopy` | `WouldMoveZone(_, "Battlefield")` | CR 707.6/614.12a, the permanent enters the battlefield as a copy of another permanent (Clone-style) rather than as itself. |

Multiple matching replacement effects that both rewrite the same event (e.g. two different "goes
to exile instead" effects on the same death) pause and ask the affected player which one to apply
first, per CR 616.1 — this is automatic, nothing to opt into.

## `Game.Consumption.*`

Used with `:SetConsumption` on a [replacement effect](replacement-effects.md) — how many times it
can fire. Bare values, except `LimitedUses`:

| Name | Args | Meaning |
|---|---|---|
| `OneShot` | — | Fires once, ever, then is spent. |
| `Repeatable` | — | Fires every time it matches, no limit. The default if `:SetConsumption` is never called. |
| `LimitedUses(n)` | integer | Fires up to `n` times, then is spent. |

## `Game.Tokens.*`

Used as the `spec` argument to `Game.Ops.CreateToken`. See [tokens.md](tokens.md) for the full
reference and worked examples.
