# Lua Scripting API

The canonical reference has moved to [docs/LUA_API.md](../LUA_API.md). This folder keeps topic-specific companions: card-script basics, per-ability-kind guides, targeting and mid-resolution choices, tokens, and the not-available-yet list.

- **[Card script basics](card-script-basics.md)** — how a card's Lua runs: `register_abilities(c)`, conditions, operations, primitives vs. custom functions, and registration timing.
- **[Game constants reference (legacy)](game-constants.md)** — older multi-page reference, superseded by [docs/LUA_API.md](../LUA_API.md).
- **[Triggered abilities](triggered-abilities.md)** — `Game.CreateTriggeredAbility` and `:SetCondition`/`:SetOperation`.
- **[Static abilities](static-abilities.md)** — `Game.CreateStaticAbility`, layers, effects, and `:SetAffected`.
- **[Activated abilities](activated-abilities.md)** — `Game.CreateActivatedAbility` and `:AddCost`.
- **[Mana abilities](mana-abilities.md)** — `Game.CreateManaAbility` and mana production.
- **[Replacement effects](replacement-effects.md)** — `Game.CreateReplacementEffect`, filters, payloads, and consumption.
- **[Cost modifiers](cost-modifiers.md)** — `Game.CreateCostModifier`.
- **[Player modifiers](player-modifiers.md)** — hand size, hand reveal, draw limit, and extra land plays.
- **[Targeting and mid-resolution choices](targeting-and-choices.md)** — filter tables, `Game.SelectTargets`, `Game.MayChoose`, `Game.ChooseN`, and scripted predicates.
- **[Tokens](tokens.md)** — `Game.Tokens.*` and `Game.Ops.CreateToken`.
- **[Not available yet](not-available-yet.md)** — named gaps rather than silently guessed features.
- **[VSCode intellisense](vscode-intellisense.md)** — EmmyLua stub and `.luarc.json` setup.
