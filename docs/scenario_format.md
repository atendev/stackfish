# Scenario File Format

> Living doc — update when `crates/scenario/src/lib.rs`'s schema changes (new action/assertion
> variants, fewer simplifications, etc). If this doc and the code disagree, the code wins; fix
> the doc.

A scenario is a TOML file describing a game setup and a sequence of actions to run against it,
plus assertions to check afterward. Used by:

- `cargo test --workspace` — runs every `.toml` file in `crates/engine/tests/scenarios/`
  automatically (see `crates/engine/tests/scenarios.rs`).
- `cargo run -p stackfish-cli -- path/to/scenario.toml` — runs one scenario file and prints a
  report. Use this for one-off manual or AI-driven testing.

Actions that hit not-yet-implemented engine code report `not yet implemented` rather than
crashing the run — that's expected pre-Phase 7, not a failure. Only an assertion that actually ran
and got the wrong answer counts as a hard failure.

## Top-level fields

```toml
seed = 1          # u64, required - seeds GameState.rng

[[players]]        # zero or more
[[cards]]           # zero or more - ad-hoc test card definitions
[[setup]]           # zero or more - places a card into a zone at game start
[[actions]]         # zero or more - PlayerActions run in order
[[assertions]]      # zero or more - checked after all actions run
```

## `[[players]]`

```toml
[[players]]
id = 0       # u32, becomes PlayerId(id)
life = 20    # i32, starting life
```

One zone each of Library/Hand/Graveyard/Command is created per player automatically. Battlefield
and Exile are shared (created once, not per player).

## `[[cards]]`

Ad-hoc test fixtures — not real card data, no connection to the external card database project.
Define just enough of a card to exercise what a scenario needs.

```toml
[[cards]]
key = "bear"                # string, referenced by [[setup]]/[[actions]]/[[assertions]] below
name = "Test Bear"
generic_cost = 1             # u32, default 0
colors = ["G"]                # list of "W"/"U"/"B"/"R"/"G", default []
types = ["Creature"]          # required - see Card types below
power = 2                     # optional i32
toughness = 2                  # optional i32
```

**Simplification:** mana cost is `generic_cost` + `colors`, not a parsed cost string like
`"{2}{R}"` — no mana-cost-string parser exists anywhere yet. Can't currently express hybrid,
Phyrexian, or X costs on a test fixture. Extend `CardSpec` in `crates/scenario/src/lib.rs` if a
scenario needs one of those.

**Card types** (`types`, at least one required): `Artifact`, `Creature`, `Enchantment`,
`Instant`, `Land`, `Sorcery`, `Kindred`, `Planeswalker`, `Battle`. Supertypes (`Legendary` etc) and
subtypes (`Human`, `Aura`, etc) aren't settable from a scenario file yet — add fields to
`CardSpec` if a test needs them.

## `[[setup]]`

Places one card instance into one zone for one player at game start.

```toml
[[setup]]
player = 0          # u32, matches a [[players]] id
zone = "battlefield" # see Zones below
card = "bear"         # matches a [[cards]] key
```

Each `[[setup]]` entry creates a new object instance — list the same card key twice to get two
copies on the battlefield, each with its own `ObjectId`. Visibility defaults to CR's normal zone
defaults (library/hand hidden, everything else public) — no way to override per-object visibility
from a scenario yet.

**A card's key only becomes resolvable in `[[actions]]`/`[[assertions]]` once it has a
`[[setup]]` entry.** Defining a card in `[[cards]]` alone doesn't create an instance of it — if an
action references a card key with no matching `[[setup]]` entry, the scenario panics immediately
with "unknown card key" (not caught/reported as `NotImplemented`, since this is a scenario-file
mistake, not an engine gap). A spell meant to be cast from hand still needs a `[[setup]]` entry
placing it in `"hand"` first.

**Zones**: `"library"`, `"hand"`, `"battlefield"`, `"graveyard"`, `"exile"`, `"command"`.
`"stack"` is intentionally not settable here — the stack needs `StackObject` metadata
(targets/modes/costs), not just a card sitting in a zone; use `[[actions]]` to get something onto
the stack once casting is implemented.

## `[[actions]]`

Each entry becomes one `PlayerAction`, run in file order via `Engine::submit_action` + `step()`.
Tagged by `type`.

```toml
[[actions]]
type = "pass_priority"
player = 0
```

```toml
[[actions]]
type = "play_land"
player = 0
card = "forest"   # matches a [[cards]] key
```

```toml
[[actions]]
type = "cast_spell"
player = 0
card = "shock"
targets = [0]            # see Targets below
modes = []                # list of u32, default []
x_value = 0               # optional u32
```

```toml
[[actions]]
type = "activate_ability"
player = 0
source = "bear"
ability_index = 0
targets = []
```

```toml
[[actions]]
type = "declare_attackers"
player = 0
attackers = [
  { attacker = "bear", target = 1 },
]
```

```toml
[[actions]]
type = "declare_blockers"
player = 1
blocks = [
  { blocker = "wall", attacker = "bear" },
]
```

```toml
[[actions]]
type = "make_choice"
player = 0
description = "choose a player"
selection = [1]
```

**Not yet supported:** `SubmitPayment`. `PaymentPlan`'s `CostPayment` shapes are more involved and
the cost system itself isn't implemented yet either — add an `ActionSpec::SubmitPayment` variant
once that's worth testing.

### Targets

Anywhere a target is expected (`targets`, `selection`, attacker `target`), write either:

- a player id as a bare number: `0` (becomes `Target::Player(PlayerId(0))`)
- a card key as a string: `"bear"` (becomes `Target::Object(<that card's id>)`)

```toml
targets = [0, "bear"]   # a player and a card, mixed, both valid
```

## `[[assertions]]`

Checked once, after every action has run. Tagged by `type`.

```toml
[[assertions]]
type = "life"
player = 0
equals = 18
```

```toml
[[assertions]]
type = "ledger_len"
equals = 3
```

```toml
[[assertions]]
type = "object_zone"
card = "bear"
equals = "graveyard"   # any zone name from the Zones list above
```

These read `GameState` fields directly (not through the engine's own accessor methods), so they
work regardless of which inherent methods are implemented yet.

## Example: full file

```toml
seed = 1

[[players]]
id = 0
life = 20

[[players]]
id = 1
life = 20

[[cards]]
key = "bear"
name = "Test Bear"
generic_cost = 1
colors = ["G"]
types = ["Creature"]
power = 2
toughness = 2

[[setup]]
player = 0
zone = "battlefield"
card = "bear"

[[assertions]]
type = "life"
player = 0
equals = 20

[[assertions]]
type = "object_zone"
card = "bear"
equals = "battlefield"
```

## Extending the format

The schema lives in `crates/scenario/src/lib.rs` (`ScenarioFile` and friends — `PlayerSpec`,
`CardSpec`, `SetupEntry`, `ActionSpec`, `AssertionSpec`). Adding a new action or assertion variant
is: add the enum variant + its TOML-facing fields, add the matching arm in `to_player_action` or
`check_assertion`, update this doc. Update this doc in the same change as the code — don't let it
drift.
