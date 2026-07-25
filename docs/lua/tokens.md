# Tokens

[← index](index.md)

CR 111 — creating token permanents. A token's shape is built with a `Game.Tokens.*` value, then
created with [`Game.Ops.CreateToken(spec, count?)`](game-constants.md#gameops).

## `Game.Tokens.*`

| Name | Args | Produces |
|---|---|---|
| `Clue` | — | A Clue token ("{2}, Sacrifice this artifact: Draw a card."). |
| `Food` | — | A Food token ("{2}, {T}, Sacrifice this artifact: You gain 3 life."). |
| `Creature(subtypes, colors, power, toughness, opts?)` | see below | A creature token. |
| `ArtifactCreature(subtypes, colors, power, toughness, opts?)` | same as `Creature` | A creature token that's also an artifact. |
| `CopyOf(objectId, exceptions?)` | `objectId`: the object to copy. `exceptions`: same table `Game.Ops.BecomeCopy` accepts. | A token that enters as a copy of an existing object (CR 707.10f). |

`Game.Tokens.Treasure` isn't available yet — it needs a mana-color choice, which token creation
doesn't support yet either.

## `Creature`/`ArtifactCreature` arguments

- `subtypes` — a subtype name, or a list of subtype names, e.g. `"Soldier"` or `{ "Elf",
  "Warrior" }`.
- `colors` — a string of color letters, e.g. `"W"` or `"WU"`; empty string for colorless.
- `power`, `toughness` — integers.
- `opts` (optional table):
  - `keywords = { "Flying", ... }` — keyword abilities to grant.
  - `name = "..."` — overrides the default name (which is otherwise built from the subtypes, e.g.
    "Soldier" or "Elf Warrior").
  - `additional_types = { "Artifact" }` — extra card types beyond Creature (used internally by
    `ArtifactCreature`, but available directly too, e.g. for an enchantment creature token).
  - `additional_supertypes = { "Legendary" }` — e.g. a legendary creature token.

## Worked examples

```lua
-- Create a 1/1 white Soldier creature token.
Game.Ops.CreateToken(Game.Tokens.Creature("Soldier", "W", 1, 1))
```

```lua
-- Create two 2/2 black Zombie tokens.
Game.Ops.CreateToken(Game.Tokens.Creature("Zombie", "B", 2, 2), 2)
```

```lua
-- Create a 0/0 green Elemental token with trample and haste.
Game.Ops.CreateToken(Game.Tokens.Creature("Elemental", "G", 0, 0, {
    keywords = { "Trample", "Haste" },
}))
```

```lua
-- Create a legendary 4/4 colorless Golem artifact creature token named
-- "Karnstruct".
Game.Ops.CreateToken(Game.Tokens.ArtifactCreature("Golem", "", 4, 4, {
    name = "Karnstruct",
    additional_supertypes = { "Legendary" },
}))
```

```lua
-- Create a Clue token.
Game.Ops.CreateToken(Game.Tokens.Clue)
```

```lua
-- Create a token that's a copy of target creature.
e:SetOperation(function(source, controller)
    local target = Game.SelectTargets({ card_types = { "Creature" } })
    Game.Ops.CreateToken(Game.Tokens.CopyOf(target))
    return {}
end)
```

## Notes

- Tokens created this way get the same `EntersTapped`/`EntersWithCounters`/Bloodthirst/Sunburst
  replacement-effect handling as any other permanent entering the battlefield — see
  [replacement-effects.md](replacement-effects.md). Devour and Amplify don't offer their choice on
  token creation yet.
- CR 111.7 (a token ceasing to exist once it leaves the battlefield) isn't enforced yet — a token
  currently behaves like a normal card once it moves to another zone.
