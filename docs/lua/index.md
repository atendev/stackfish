# Lua Scripting API

This is the reference for writing a card's abilities in Lua: how a card script is structured, the
full `Game.*` API surface, and the mid-resolution player-choice mechanisms available to you.

**Status:** this documents what actually works today, not an aspirational API. If something isn't
listed here, assume it doesn't exist yet — see [not-available-yet.md](not-available-yet.md) before
assuming a signature you don't see documented is just "missing from the docs."

## Start here

- **[Card script basics](card-script-basics.md)** — how a card's Lua actually runs: the
  `register_abilities(c)` structure, the condition/operation split, primitives vs. custom functions,
  and how a card's registrations reset when it changes zones. Read this first.
- **[Game constants reference](game-constants.md)** — every `Game.Conditions.*`, `Game.InterveningIf.*`,
  `Game.Ops.*`, `Game.Costs.*`, `Game.Layers.*`, `Game.Effects.*`, `Game.Durations.*`,
  `Game.EventFilters.*`, `Game.ReplacementPayloads.*`, `Game.Consumption.*`, and `Game.Tokens.*`
  entry, in one page, organized by table. The page to search when you know roughly what primitive
  you want and need its exact Lua signature.

## Registering an ability (one page per kind)

A card's `register_abilities(c)` can call any of the `Game.Create*` constructors below any number of
times — each ability/effect you build and `:Register()` is independent, so one card can register
several abilities.

| Page | Constructor | Registers | Rules anchor |
|---|---|---|---|
| [Triggered abilities](triggered-abilities.md) | `Game.CreateTriggeredAbility(c)` | "When/Whenever/At..." abilities | CR 603 |
| [Static abilities](static-abilities.md) | `Game.CreateStaticAbility(c)` | Always-on continuous effects | CR 604/611/613 |
| [Activated abilities](activated-abilities.md) | `Game.CreateActivatedAbility(c)` | "Cost: Effect" abilities | CR 602 |
| [Mana abilities](mana-abilities.md) | `Game.CreateManaAbility(c)` | "{T}: Add {C}"-style abilities | CR 605 |
| [Replacement effects](replacement-effects.md) | `Game.CreateReplacementEffect(c)` | "If this would X, Y instead" effects | CR 614/615/616 |
| [Cost modifiers](cost-modifiers.md) | `Game.CreateCostModifier(c)` | "Spells you cast cost {N} more/less" | CR 601.2f |

## Cross-cutting mechanisms

- **[Targeting and mid-resolution choices](targeting-and-choices.md)** — the shared filter-table
  shape (`card_types`/`controller`/`subtypes`/...) used across `:SetAffected`/`:SetTarget`/cost
  filters; scripted targeting predicates for cases a plain filter table can't express; and the two
  mechanisms for asking the player something in the middle of resolving an effect,
  `Game.SelectTargets`/`Game.MayChoose`.
- **[Tokens](tokens.md)** — `Game.Tokens.*` token specs and `Game.Ops.CreateToken`.
- **[Not available yet](not-available-yet.md)** — real gaps, named rather than silently guessed
  at, plus a short how-to for adding a new primitive if you're extending the engine itself.

## A few things worth knowing before writing a card

- **A custom function never mutates anything itself.** Whether you're writing an operation, an
  effect, or an affects-predicate, a custom function *describes* what should happen (usually by
  returning a plain table) — the engine is what actually applies it. See
  [card-script-basics.md](card-script-basics.md#primitive-vs-custom-function).
- **Typos are load-time errors, not silent no-ops.** A condition/operation/effect that doesn't
  resolve to a recognized shape (e.g. `Game.Conditions.Typo`, which indexes to `nil`) fails to
  load rather than quietly doing nothing. If a card doesn't load, that's the engine telling you
  something's wrong — not something to work around.
- **A card only ever sees what it's allowed to see.** A condition callback gets the triggering
  event and nothing else; it can't peek at an opponent's hand or library. See
  [card-script-basics.md](card-script-basics.md#hidden-information).
- **A permanent that changes zones and comes back is a brand-new object.** Its abilities and
  continuous effects are re-registered from scratch every time it re-enters the battlefield — see
  [card-script-basics.md](card-script-basics.md#registration-timing).
