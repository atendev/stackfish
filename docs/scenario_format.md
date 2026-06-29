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

The CLI's output describes each action/assertion next to its result (`ActionSpec::describe`/
`AssertionSpec::describe` in `crates/scenario/src/lib.rs`), e.g.:

```
-- actions --
  [4] ran - cast_spell(player=0, card="bear", targets=[], modes=[], x_value=None)
  [5] ran - submit_payment(player=0, units=["G", "C", "C"])

-- assertions --
  [3] PASS - phase == "main"
  [4] FAIL - step == "untap" (expected untap, got PreCombatMain)
```

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
mana_pool = { green = 1, colorless = 2 }   # optional, all six fields default 0
```

One zone each of Library/Hand/Graveyard/Command is created per player automatically. Battlefield
and Exile are shared (created once, not per player).

**`mana_pool` is fake, pre-seeded floating mana**, for cases where a scenario needs mana before its
action sequence could plausibly produce any (or doesn't want to bother with a `mana_ability` card
just to pay one cost). `[players.mana_pool]` can be omitted entirely (defaults to all-zero). CR 605
mana abilities now exist for the tap-a-permanent-for-mana case (see `mana_ability` under
`[[cards]]`) — a scenario can produce real mana via `activate_ability` instead of seeding the pool,
if that's what it's testing.

## `[[cards]]`

Ad-hoc test fixtures — not real card data, no connection to the external card database project.
Define just enough of a card to exercise what a scenario needs.

```toml
[[cards]]
key = "bear"                # string, referenced by [[setup]]/[[actions]]/[[assertions]] below
name = "Test Bear"
generic_cost = 1             # u32, default 0 - the whole generic component as one number
colors = ["G"]                # one entry PER PIP (repeats included), default []
types = ["Creature"]          # required - see Card types below
power = 2                     # optional i32
toughness = 2                  # optional i32
mana_ability = ["G"]           # optional, default [] - see Mana abilities below
```

**`mana_ability` (CR 605):** if non-empty, every instance of this card gets one
`RegisteredAbility { kind: Activated, effect: AddMana(...) }` set up automatically — activate it via
`activate_ability` with `ability_index = 0`. Each entry is one literal unit added (same letters as
`mana.units`, see below) — `["G"]` models `{T}: Add {G}`; `["G", "G"]` would model adding two green
in one activation. Only one mana ability per card this way, and the tap cost is implicit/hardcoded
(this is the only cost shape mana abilities support right now — see `commands.rs`'s
`ActivateAbility` arm). A card with no `mana_ability` entries can still be referenced by an
`activate_ability` action, but it has no registered abilities, so `ability_index = 0` will be
rejected ("no such ability on source").

A `{2}{G}{G}` cost is `generic_cost = 2, colors = ["G", "G"]` — two list entries for the two green
pips, not `["G"]` with an implied count. `submit_payment`'s `mana.units` field (see that action's
docs below) pays this with a *different* shape on purpose — a flat list of literal units, not an
echo of this cost shape, since which mana actually pays the generic component is the payer's
choice (CR 118.13a), not implied by the cost.

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

```toml
[[actions]]
type = "submit_payment"
player = 0
mana = { units = ["G", "C", "C"] }   # optional, default {} (an empty plan)
```

Answers a pending `ProcessState::AwaitingPayment` pause (CR 601.2f-h) — i.e. the action right
after a `cast_spell`/`activate_ability` that has a real cost. Deliberately **not** routed through
`submit_action` like every other action above — it answers `Engine::submit_payment` directly
(mirrors how a replacement-ordering or trigger-ordering choice would, if those ever got a TOML
shape too).

#### `mana.units`: the exact literal mana spent

`mana` is a `ManaPaymentSpec` with one field, `units` — a **flat list of literal mana units**, one
entry per unit actually spent, not a cost-shape echo:

- `"W"`/`"U"`/`"B"`/`"R"`/`"G"` — one colored unit of that color.
- `"C"` — one colorless unit.
- There's no `generic_cost`-style combined number here — every unit, whether it's covering a
  colored pip or the generic component, gets its own list entry.

This is CR 118.13a's actual payment choice: a colored pip **must** be paid with that exact color
(no substitution), but the generic component can be covered by **any** combination of leftover
units — extra of a color that also paid a pip, a completely different color, or colorless.
`PaymentPlan::validate` (`cost.rs`) works out which units satisfy which requirement from the flat
list itself; you just state what came out of the pool.

```toml
# Card costs generic_cost = 2, colors = ["G", "G"] ({2}{G}{G})
mana = { units = ["G", "G", "C", "C"] }        # 2 green pips, generic paid with colorless
mana = { units = ["G", "G", "W", "U"] }        # same pips, generic paid with 2 other colors instead - also valid
mana = { units = ["G", "G", "G", "C"] }        # an extra green covers part of the generic too - also valid
mana = { units = ["U", "U", "C", "C"] }        # WRONG color for the G pips -> rejected, no substitution
mana = { units = ["G", "G", "C"] }             # one short -> rejected
mana = { units = ["G", "G", "C", "C", "C"] }   # one too many -> rejected, even with a huge pool
```

The total list must sum to **exactly** the cost — every colored pip covered by its exact color,
and the remaining units summing to exactly the generic amount, no more, no less. Whether the
*pool* can actually cover a correctly-shaped list is checked separately (and does allow the pool
to hold more than the cost — only the units actually listed come out; everything else floating is
left untouched).

Omit `mana` entirely (or write `mana = {}`) to pay a `{0}` cost — CR 118.5a still requires this
explicit empty acknowledgment, casting never auto-pays. An empty `mana` against a *non-zero* cost
is also rejected (not a shortcut for "pay nothing and hope") — `units` must be empty only when the
spell's own cost has no mana component at all.

**Not yet supported in `mana.units`:** there's no way to express paying with Phyrexian mana (life
instead of a color), snow mana, or an X-cost payment — `[[cards]]` itself can't express those costs
either, so a card using any of those can't be paid via this action regardless. Also unsupported:
additional costs (sacrifice/discard/etc) and alternative costs (flashback/escape) —
`PaymentPlan::validate` rejects any `CostSpec` that has either set, and nothing in this slice
constructs one.

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
equals = "graveyard"   # any zone name from the Zones list above, "stack" included
```

`equals = "stack"` works too, despite the stack not being one of `state.zones` — the assertion
falls back to checking `state.stack` directly (a card on the stack has a `StackObject`, not a slot
in any `Zone.contents`). Useful for checking a spell mid-cast, before it resolves off the stack —
e.g. right after a `cast_spell` action and before any `pass_priority` that would resolve it.

```toml
[[assertions]]
type = "mana_pool"
player = 0
color = "green"            # one of white/blue/black/red/green/colorless
equals = 0
```

```toml
[[assertions]]
type = "tapped"
card = "forest"
equals = true
```

Checks `GameObject.tapped` — the way to confirm an `activate_ability` action (e.g. CR 605's
tap-for-mana case) actually tapped its source.

```toml
[[assertions]]
type = "phase"
equals = "main"     # pre_game / beginning / main / combat / ending
```

```toml
[[assertions]]
type = "step"
equals = "pre_combat_main"
# one of: untap / upkeep / draw / pre_combat_main / beginning_of_combat /
# declare_attackers / declare_blockers / combat_damage / end_of_combat /
# post_combat_main / end / cleanup
```

`pre_combat_main`/`post_combat_main` aren't real CR 500.4 steps (Main has none) — they exist so
`step` always holds a meaningful, non-stale value while `phase == "main"` instead of leaking
whatever the previous real step happened to be (CR 505.1's "Main happens twice per turn").

```toml
[[assertions]]
type = "active_player"
equals = 0    # u32, matches a [[players]] id
```

```toml
[[assertions]]
type = "priority_player"
equals = 0    # u32, matches a [[players]] id
```

These read `GameState` fields directly (not through the engine's own accessor methods), so they
work regardless of which inherent methods are implemented yet.

**A card's key keeps resolving correctly across zone moves.** `play_land`/`cast_spell`/resolution
all mint a *new* `ObjectId` for the moved object (CR 400.7) instead of reusing the old one — every
`[[setup]]` card key is tracked and automatically re-pointed at its new id after each action that
moves it (`remap_moved_card_ids` in `crates/scenario/src/lib.rs`), so `object_zone`/later actions
referencing `"bear"` keep working after it's been cast, resolved, etc. You don't need to do anything
for this — just keep using the same key from `[[setup]]` throughout the file.

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

## Example: casting and paying for a spell

A solo player's first `pass_priority` always counts as "everyone passed" (1 player ≥
`turn_order.len() == 1`), so each one advances exactly one turn/phase/step (CR 500-514) —
`Untap → Upkeep → Draw → Main`. `cast_spell`'s non-instant branch needs `Phase::Main` (sorcery
speed), so reaching it from the game's `PreGame` start takes 4 passes; one more pass after paying
resolves the spell off the stack (CR 608.2m). See
`crates/engine/tests/scenarios/phase5_cast_spell_payment.toml` for this exact file, runnable as-is.

```toml
seed = 1

[[players]]
id = 0
life = 20
mana_pool = { green = 1, colorless = 2 }

[[cards]]
key = "bear"
name = "Grizzly Bears"
generic_cost = 2
colors = ["G"]
types = ["Creature"]
power = 2
toughness = 2

[[setup]]
player = 0
zone = "hand"
card = "bear"

[[actions]]
type = "pass_priority"
player = 0
# (x4 total - PreGame -> Untap -> Upkeep -> Draw -> Main)

[[actions]]
type = "cast_spell"
player = 0
card = "bear"

[[actions]]
type = "submit_payment"
player = 0
mana = { units = ["G", "C", "C"] }

[[actions]]
type = "pass_priority"
player = 0
# resolves the stack now that everyone's passed again

[[assertions]]
type = "mana_pool"
player = 0
color = "green"
equals = 0

[[assertions]]
type = "object_zone"
card = "bear"
equals = "battlefield"

[[assertions]]
type = "phase"
equals = "main"

[[assertions]]
type = "step"
equals = "pre_combat_main"
```

## Extending the format

The schema lives in `crates/scenario/src/lib.rs` (`ScenarioFile` and friends — `PlayerSpec`,
`CardSpec`, `SetupEntry`, `ActionSpec`, `AssertionSpec`). Adding a new action or assertion variant
is: add the enum variant + its TOML-facing fields, add the matching arm in `to_player_action` or
`check_assertion`, update this doc. Update this doc in the same change as the code — don't let it
drift.
