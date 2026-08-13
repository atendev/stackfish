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
[activated](activated-abilities.md) ability, or with a plain spell's own
[`s.operation()`](card-script-basics.md#the-two-functions-that-arent-battlefield-timed). Describes
what the ability/spell actually does when it resolves.

| Name | Args | Meaning |
|---|---|---|
| `AddMana(letter, count?)` | `letter`: a single mana-color letter (`"W"`/`"U"`/`"B"`/`"R"`/`"G"`) or `"C"` for colorless. `count`: how many, default 1. | CR 605, adds exactly this fixed mana to the activator's pool. No mixed-color-in-one-call shape yet ("add one white and one blue" needs two calls or `AddManaChoice`). |
| `AddManaChoice(count, colors)` | `count`: how many mana. `colors`: a string of allowed letters, e.g. `"WU"`. | CR 605, "add `count` mana of any of these colors, your choice" — the player picks the actual color(s) when activating. |
| `AddRestrictedMana(letter, cardType, count?)` | `letter`/`count`: same as `AddMana`. `cardType`: a card type name (e.g. `"Artifact"`). | CR 106.11, same fixed-mana shape as `AddMana`, but the payment engine only accepts it toward casting a spell of `cardType` (Mishra's Workshop: `Game.Ops.AddRestrictedMana("C", "Artifact", 3)`). Never usable for an activated ability's cost. See [mana-abilities.md](mana-abilities.md#restricted-mana). |
| `GainLife(amount)` | integer | CR 119, controller gains exactly `amount` life. Also the one key a custom operation function's returned table can use (`{ gain_life = amount }`) — see [card-script-basics.md](card-script-basics.md#writing-an-operation). |
| `Damage(amount)` | integer (unsigned) | CR 608.2m, deals exactly `amount` damage to the ability's target. Goes through the normal damage pipeline, so prevention/replacement effects and Lifelink/Wither/Infect/Deathtouch all apply normally. Single fixed target only — no "divide among any number of targets" or "each opponent" shape yet. |
| `Destroy` | — | CR 701.8, destroys the ability's target. Indestructible targets are unaffected. |
| `DestroyAll(filter)` | `filter`: a table with `card_types`/`controller`/`self_only` — the same shape [`:SetAffected`](static-abilities.md#filter-table) uses. | CR 701.8/601.2c, destroys every battlefield permanent matching `filter`, unconditionally — no targets at all (e.g. Armageddon: `Game.Ops.DestroyAll({ card_types = { "Land" } })`). Indestructible permanents are unaffected, same as `Destroy`. `subtypes`/exclusion keys aren't supported in `filter` yet — a plain type/controller restriction is all this covers so far. |
| `Sacrifice` | — | CR 701.17, sacrifices the ability's target (must be something the caster/controller controls). Unlike `Destroy`, indestructibility doesn't stop this. Not to be confused with `Game.Costs.Sacrifice` — that's a *cost* (pick any matching permanent you control, no targeting), this is a resolution effect against a chosen target. |
| `Exile` | — | CR 701.18, exiles the ability's target. Ignores indestructibility. Battlefield targets only for now (exiling a card from a graveyard isn't wired up yet). |
| `Counter` | — | CR 701.6a, "counter target spell." The target is another spell on the stack — declare it via `s.target_requirement()` with `zone = "Stack"` (e.g. `{ zone = "Stack", count = 1 }` for a plain Counterspell, `excluded_card_types = { "Creature" }` for a Negate-style "target noncreature spell," or `mana_value_at_most = 3` / `mana_value_equals = "X"` for a mana-value-restricted counter like Convolute or "counter target spell with mana value X" — see the filter table's own `mana_value_*` keys below). Removed from the stack without resolving; lands in its owner's graveyard. Doesn't reach "counter target activated or triggered ability" (abilities aren't targetable objects yet) or "counter unless that player pays" (no mid-resolution payment ask yet). |
| `SacrificeChosen(list)` | `list`: a `Game.ChooseN` result | CR 701.21, sacrifices exactly the objects in `list` (a Lua-side helper, not a coroutine ask itself) — the intended pairing is `Game.ChooseN` then `Game.Ops.SacrificeChosen(chosen)` as an operation's return value. See [targeting-and-choices.md](targeting-and-choices.md#choosing-n-objectscards-mid-resolution). |
| `DiscardChosen(list)` | `list`: a `Game.ChooseN` result | CR 701.9, discards exactly the cards in `list`, same pairing as `SacrificeChosen` above (typically with `Game.ChooseN({ zone = "Hand", ... }, n)`). |
| `SacrificeRandom(filter, count)` | `filter`: shared filter-table shape; `count`: number | CR 701.21 + "at random" — sacrifices `count` matching permanents chosen by the engine, no player input. Fewer than `count` if fewer candidates exist. A static value like `Destroy`/`DestroyAll`, not a custom function. |
| `DiscardRandom(filter, count)` | `filter`: shared filter-table shape; `count`: number | CR 701.9b's "require a random discard" — discards `count` matching cards from the resolving ability's own controller's hand at random. Fewer than `count` if fewer match. An empty `filter` table matches every card. |
| `ForEachPlayer(x)` | `x`: `function(source, controller, state)`, or a bare `SacrificeRandom`/`DiscardRandom` value | CR 101.4 (APNAP) — invokes `x` once per player, active player first (or, for the bare-tag shape, draws each player's own random picks with no script involved at all). See [targeting-and-choices.md](targeting-and-choices.md#applying-an-effect-to-each-player) for the full shape, including the CR 101.4 simultaneity guarantee. |
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
    counter_kind = "PlusOnePlusOne", -- CR 707.9e: an "additional effect" exception - a real counter,
    counter_amount = 1,              --   not a characteristic (must set both together, e.g. Spark Double)
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
| `DiscardSelf` | — | CR 702.29a Cycling's "Discard this card" — discards the ability's own source. Always the source, never a choice (mirrors `TapSelf`), unlike `Discard(filter)` above. An activated ability whose cost includes this may be activated from `Hand` instead of `Battlefield` (CR 602.1's "unless the ability itself says otherwise" carve-out) — see [activated-abilities.md](activated-abilities.md). |
| `TapPermanents(filter, count)` | `filter`: shared filter-table shape. `count`: how many. | Tap `count` *other* permanents (not the source) matching `filter` — distinct from `TapSelf`. |
| `ExileFromGraveyard(filter, count)` | `filter`: shared filter-table shape. `count`: how many. | CR 702.138a Escape's "exile N other cards from your graveyard" — same shape as `TapPermanents`, checked against your graveyard instead of the battlefield. No self-exclusion needed: an escaping card has already left the graveyard (moved to the stack) by the time this cost is paid, so it can never be one of its own `count` candidates. |
| `Life(amount)` | integer | CR 119.4, pay exactly `amount` life. You can't pay more life than you have. |
| `Mana(cost)` | a mana-cost string, e.g. `"{1}{R}"` | The mana-cost component of the activation cost. Full hybrid/Phyrexian/generic parsing per the normal mana-cost grammar. At most one `Mana(...)` per ability. `{X}` is allowed (CR 107.3a/107.3k/602.2b) - the activating player announces a value as part of activation, independent of any other X on the same object or other instance of this ability, resolved into a generic amount before payment the same way a spell's own `{X}` is. |

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
| `AbilityAdding` | 6 | `AddKeyword`, `RemoveKeyword`, `IgnoreKeyword`, every `AddProtection*`, `AddLandwalk*`, `AddBandsWithOther`, `AddBandsWithOtherSupertype`, `RemoveAllAbilities` |
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
| `RemoveKeyword(name)` | keyword name string | Removes just this keyword outright — the object no longer has it at all, for any purpose. Doesn't touch protection or any other keyword, unlike `RemoveAllAbilities` below. |
| `IgnoreKeyword(name)` | keyword name string | The object keeps this keyword (anything checking "has [keyword]" still sees it), but the restriction it imposes is voided — e.g. "Walls you control can attack as though they didn't have defender" (Assault Formation) is `IgnoreKeyword("Defender")`. Today only `"Defender"` has a real enforcement call site (the attack-declaration check) — `KeywordSet`'s other bits don't restrict the object's own actions the same way. |
| `AddProtectionColor(letter)` | mana-color letter | CR 702.16, protection from that color. |
| `AddProtectionCardType(name)` | card type name | Protection from that card type (e.g. "protection from artifacts"). |
| `AddProtectionSubtype(name)` | subtype name | CR 702.16g, protection from that creature type. One call per subtype (write "protection from A and from B" as two separate registered effects). |
| `AddProtectionName(name)` | a card name | CR 702.16a, protection from a specific named card. Matched against the object's current (possibly changed) name. |
| `AddProtectionPlayer(who)` | `"you"` or `"opponent"` | CR 702.16k, protection from a player, relative to whichever player controls the *protected* object. `"any"` isn't valid here — use `AddProtectionEverything` for "protection from everything." |
| `AddProtectionEverything` | — | CR 702.16j, protection regardless of any characteristic — no quality to name. |
| `AddLandwalkSubtype(name)` | land subtype, e.g. `"Island"` | CR 702.14, plain landwalk (covers the 5 basic landwalks). |
| `AddLandwalkQuality(opts)` | table — see [static-abilities.md](static-abilities.md) | CR 702.14a's combination forms ("nonbasic landwalk," "snow swampwalk," etc.) |
| `AddBandsWithOther(subtype)` | subtype name | CR 702.22b/c, "bands with other [subtype]." |
| `AddBandsWithOtherSupertype(name)` | supertype name (`"Basic"`/`"Legendary"`/`"Snow"`/`"World"`) | CR 702.22c/m, the historic "bands with other Legends" wording — supertype form of the above. |
| `AddCardType(name)` | card type name | Adds a card type (additive only — can't remove one). |
| `AddColor(letter)` | mana-color letter | Adds a color (additive only). |
| `SwitchPT` | — | Swaps power and toughness. |
| `RemoveAllAbilities` | — | CR 613.1f, strips every keyword and protection quality accumulated so far. A later effect with a higher timestamp that re-adds something still applies on top. |

Every keyword granted or removed this way is actually respected wherever it matters — including Haste (bypasses summoning sickness), Defender (can't attack), and Vigilance (doesn't tap to attack).

A custom function is also accepted in place of any `Game.Effects.*` value — see
[static-abilities.md](static-abilities.md) for its call signature (it returns a table describing
the effect shape instead of using a tag).

`Game.RegisterKeyword(source, name)` is a standalone helper (not part of the `Game.Effects.*`
table) that wraps the `CreateStaticAbility`/`SetLayer(AbilityAdding)`/`SetEffect(AddKeyword(name))`/
`SetAffected({ self_only = true })`/`Register()` boilerplate for the common case of a card granting
itself a printed keyword — see the
[cookbook](../wiki/Card-Script-Cookbook.md#grant-itself-a-printed-keyword-ability) for the full
comparison. Fall back to the longhand `Game.Effects.AddKeyword` form when the target isn't
self-only or the duration isn't permanent.

### Supported keyword names (`AddKeyword`/`RemoveKeyword`/`IgnoreKeyword`)

The complete list `Game.Effects.AddKeyword(name)`, `Game.Effects.RemoveKeyword(name)`, and
`Game.Effects.IgnoreKeyword(name)` all recognize — anything else is a load-time error,
not a silent no-op:

| Name | CR | Meaning |
|---|---|---|
| `"Flying"` | 702.9 | Evasion — blockable only by Flying/Reach. |
| `"Reach"` | 702.17 | Can block a Flying creature (no evasion of its own). |
| `"Menace"` | 702.111 | Can't be blocked except by two or more creatures. |
| `"Vigilance"` | 702.20 | Doesn't tap to attack. |
| `"Haste"` | 702.10 | Ignores summoning sickness (attacking and `{T}` activated abilities). |
| `"Defender"` | 702.3 | Can't attack. |
| `"Flash"` | 702.8 | Castable any time its controller could cast an instant. |
| `"Deathtouch"` | 702.2 | Any nonzero damage from this source is lethal, regardless of amount. |
| `"Indestructible"` | 702.12 | Ignores the lethal-damage state-based action (still affected by 0 toughness). |
| `"Lifelink"` | 702.15 | Damage this source deals also gains its controller that much life. |
| `"Wither"` | 702.80 | Damage this source deals to a creature is `-1/-1` counters instead of marked damage. |
| `"Infect"` | 702.90 | Implies Wither's creature-damage behavior; damage to a player becomes poison counters instead of life loss. |
| `"FirstStrike"` | 702.7 | Deals combat damage in the first-strike damage step. |
| `"DoubleStrike"` | 702.7 | Deals combat damage in both the first-strike and regular damage steps. |
| `"Trample"` | 702.19 | Excess combat damage beyond lethal can spill to the defending player. Trample-over-planeswalkers/battles (702.19c/e/f/g) isn't modeled — excess only ever spills to a player. |
| `"Fear"` | 702.36 | Evasion — can't be blocked except by artifact and/or black creatures. |
| `"Intimidate"` | 702.13 | Evasion — can't be blocked except by artifact creatures and/or creatures sharing a color with it. |
| `"Shadow"` | 702.28 | Can only block/be blocked by other Shadow creatures. |
| `"Horsemanship"` | 702.31 | Can't be blocked by creatures without Horsemanship. |
| `"CantBeRegenerated"` | 701.19c | Not a true CR 702 keyword — an internal status flag ("this permanent can't have a regeneration shield created for it"), granted the same way as any other keyword. |

**Not grantable via `AddKeyword`:** Banding (CR 702.22) exists as a real `KeywordSet` bit
(`effective_keywords(...).contains(BANDING)` is checked at every real combat-declaration site), but
nothing in this scripting API currently sets it — only `AddBandsWithOther`/`AddBandsWithOtherSupertype`
(the "bands with other [quality]" variants, listed in the table above) are reachable from Lua. Plain
Banding is a real, named gap, not silently guessed at.

Every keyword in the table above is fully enforced, not just stored — First Strike/Double Strike
gate which combat damage step a creature deals damage in, Trample lets excess lethal damage spill
to the defending player, and Fear/Intimidate/Shadow/Horsemanship are all checked as real blocking
restrictions.

`Hexproof`/`Shroud`/`Ward` aren't in the list at all — no `KeywordSet` bit exists for any of them
yet.

**Combat damage timing (CR 510.1/510.2):** a creature's combat damage is assigned and dealt
automatically the instant its damage step opens whenever there's exactly one legal outcome
(unblocked, or blocked by a single non-trampling creature) — no card script or player action is
needed to make it happen. A real choice only pauses the game when one actually exists: 2+ blockers
on one attacker, a trampling attacker with at least one blocker, or a blocker forced onto 2+
attackers by banding. Either way, a `Game.Conditions.DealsDamage` triggered ability (above) fires
the same regardless of whether the assignment was automatic or chosen — nothing about authoring a
card script changes here.

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
