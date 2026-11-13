# Not available yet

[← index](index.md)

Real, named gaps — don't spend time trying these, they aren't silently working under a different
name.

- **Custom operation functions don't support pump/type-changing effects.** A custom function can
  return `GainLife`, `DrawCards`, `DealDamageTo`, `AddCountersTo`, `SacrificeChosen`,
  `DiscardChosen`, `CastSpell`, or a `Chain` of them — see
  [targeting-and-choices.md](targeting-and-choices.md#drawing-dealing-damage-and-adding-counters).
  A P/T pump or a type/color-changing effect is still only reachable through the `Game.Ops.*`/
  `Game.Effects.*` tag form.
- **A custom operation's `Chain` holds at most one pending kind.** It may hold `GainLife` plus one
  of: zone moves (`SacrificeChosen` and `DiscardChosen` together are fine), `DrawCards`, events
  (`DealDamageTo` and `AddCountersTo` together are fine), or `CastSpell`. Mixing two pending kinds
  raises an error. TODO: let one operation outcome carry several requests.
- **Restricted mana beyond a card-type filter** — `Game.Ops.AddRestrictedMana` covers CR 106.11's
  "spend this mana only to cast [card type] spells" (Mishra's Workshop) only. Object-specific
  restrictions ("only to cast the exiled card") and ability-activation restrictions aren't modeled —
  see [mana-abilities.md](mana-abilities.md#restricted-mana).
- **No clock, random source or files.** Scripts get only the `table`, `string`, `utf8`, `math` and
  `coroutine` libraries. `os`, `io`, `require`, `dofile`, `loadfile`, `math.random` and
  `math.randomseed` are absent, because a replay reruns a game from its seed and must play out the
  same.
- **No lookup-by-key script resolution.** A card's script always has to be the literal Lua source,
  not a key into an external registry.
- **No script-backed costs.** `Game.Costs.*` covers every structured cost shape without needing a
  custom function; there's no way to write an arbitrary custom-function cost.
- **Alternative costs (flashback/escape-style) and Buyback have no TOML scenario-fixture field.**
  A card script *can* declare both now (`s.alternative_costs()`/`s.buyback_cost()` — see
  [card-script-basics.md](card-script-basics.md#writing-alternative_costs)), but the TOML scenario
  format itself still has no `[cards.alternative_costs]`/`buyback_cost` field, so a scenario
  fixture can't drive either without a script. Hybrid, Phyrexian, `{C}`, `{X}` and snow mana
  payment all work in the engine now (see [mana-abilities.md](mana-abilities.md#snow-mana)).
- **No tap or untap replacements.** There is no replacement hook for "would become tapped/untapped"
  and no stun counters (CR 122.1d), so an untap always succeeds.
- **No `Each*` versions of the newer event tags.** `BecomesTapped`, `BecomesUntapped`, `GainsLife`,
  `LosesLife`, `Attacks`, `Blocks` and `Mills` are about this permanent or you only. "Whenever a
  player loses life" or "whenever a creature attacks" needs a custom condition function reading
  the event table (see [card-script-basics.md](card-script-basics.md#writing-a-condition)), and
  there is no `DealsCombatDamage` tag. "Whenever this blocks" triggers once per blocked attacker,
  which only matters once a creature can block more than one.
- **Reanimation options are limited.** `Game.Ops.ReturnToBattlefield`/`ReturnAllToBattlefield` take
  only `control` (`"you"` or `"owner"`) and `tapped`. Not available: entering with an extra counter
  ("with a +1/+1 counter"), entering attacking or blocking (CR 508.4), "under target opponent's
  control", a "may" return, and Zombify-style riders such as haste or "exile it at end of turn". A
  returned Clone, Devour or Amplify creature enters as itself with no choice, because those choices
  need a pause that isn't tied to a stack object. An Aura with two or more legal hosts stays in the
  graveyard because the controller's host choice (CR 303.4f) has no pause yet. The all-form enters
  its cards one after another, not as one simultaneous event. Hand and library sources are not
  supported. Persist and Undying use `Game.Ops.ReturnSelfWithCounter`, which still skips
  replacement checks and entry modifiers.
- **Fight and bite have narrow target shapes.** `Game.Ops.Fight`/`Bite` work with a two-entry
  `s.target_requirement()` array, a `count = 2` entry (Fight only) or a single `count = 1` entry
  (the ability's source is the first creature). Not available: "fights up to one target creature"
  (`up_to`/`at_least` entries), "fights each creature", damage equal to something other than power,
  and, for a two-word fight on a triggered or activated ability, any test (`:SetTarget` takes an array of
  filters now, but only `Game.Ops.ChainEach` is exercised with it). A
  self-source fight or a bite whose creature left the battlefield does nothing instead of using
  last known information (CR 608.2h). Either op can be one mode of a modal spell or trigger, with
  the mode's own `target` in one of the shapes above. A
  scenario TOML `[cards.target_requirement]` cannot declare two words (use the script array form).
- **Modes have gaps.** A spell's `s.modes()` and the `:SetModes` of a trigger or activated ability
  work (see [card-script-basics.md](card-script-basics.md#writing-modes) and
  [activated-abilities.md](activated-abilities.md#modal-activated-abilities)). Not available:
  opponent-chosen modes on an activated ability (CR 700.2e); modal mana abilities (CR 605.3b);
  a mana cost on one mode (CR 700.2h, only `Game.Costs.*` additional costs work); and mode text,
  since a host labels modes from the card text. A function operation, a `Chain` or `ChainEach`
  and a `ForEachPlayer` are solo-only modes: each can be the one chosen mode, but not join another.
  With three or more players the controller can't yet pick which opponent chooses (CR 700.2e).
- **`Game.Costs.Other`** (an unstructured, free-text additional cost) doesn't exist — every cost
  has to be one of the structured shapes in [game-constants.md](game-constants.md#gamecosts).
- **Devour/Amplify quality restrictions.** "Devour [quality] N" (sacrificing only permanents of a
  named quality, not just any creature) and a single creature having multiple independent Amplify
  instances aren't modeled — see [game-constants.md](game-constants.md#gamereplacementpayloads).
- **Damage redirection only supports "to this permanent."** Redirecting to some other chosen
  target isn't available — see [replacement-effects.md](replacement-effects.md).
- **Hexproof and Shroud work only on permanents, in their plain forms.** `"Hexproof"` and
  `"Shroud"` are real keywords (see [game-constants.md](game-constants.md#supported-keyword-names-addkeywordremovekeywordignorekeyword)).
  Not available: hexproof/shroud on a player ("You have hexproof"), "hexproof from [quality]",
  and "can be the target as though it didn't have hexproof" (`IgnoreKeyword("Hexproof")` is
  accepted but nothing reads it). "Opponent" means any other player; there is no team model.
- **Ward is `Game.RegisterWard(source, cost)`, not a keyword name.** It takes a cost, so it can't go
  through `AddKeyword`. Only a mana cost works: no "Ward—Pay N life", discard or sacrifice cost, and no
  cost that depends on X. It can only be registered when the card's script runs, not granted to another
  permanent by a continuous effect. See [game-constants.md](game-constants.md#gameconditions).
- **"Counter unless that player pays {X}" only supports a fixed printed amount.** Mana Leak's own
  shape (`Game.Ops.CounterUnlessPaid("{3}")`) is real now — see
  [game-constants.md](game-constants.md#gameops). Condescend's own `{X}` (an amount announced when
  the *countering* spell itself was cast, not a fixed printed number) isn't representable yet.
  (`Game.Ops.Counter` against a spell, mana-value-restricted spell counters, e.g. Convolute's "mana
  value 3 or less" or "mana value X," *and* "counter target activated or triggered ability"
  (Stifle, Trickbind, via `{type = "ability"}` on `:SetTarget`/`target_requirement()`) are all real
  now too.)
- **Mill and life loss have no replacement hooks.** `Game.Conditions.Mills` and `LosesLife` trigger
  on them, but there is no "would lose life" replacement or "can't lose life" restriction
  (CR 119.8). An operation can't act on "the milled card" (CR 701.17c/d); only a condition sees
  the milled ids, and a card a replacement sent to a hand or library is left out of them. There is
  no mill as a cost, no dredge and no rad counters.
