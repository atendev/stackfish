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

## Two setup paths

Scenarios use one of two construction paths — pick exactly one per file:

| Path | Key section | Engine entry point |
|---|---|---|
| **Manual** | `[[players]]` + `[[setup]]` | `Engine::new()` — no pregame automation |
| **New game** | `[game]` | `Engine::new_game()` — real shuffle, draw, mulligan |

If `[game]` is present, `[[players]]` and `[[setup]]` are ignored entirely.

## Top-level fields

```toml
seed = 1          # u64, optional - seeds GameState.rng; required on manual path, optional when [game].rng_seed is set

[[players]]        # zero or more (manual path only — ignored when [game] is set)
[[cards]]           # zero or more - ad-hoc test card definitions (used by both paths)
[[setup]]           # zero or more - places a card into a zone (manual path only)
[game]             # optional - triggers new_game path; see below
[[actions]]         # zero or more - PlayerActions run in order         \
[[assertions]]      # zero or more - checked after all actions run      / use one style
[[steps]]           # zero or more - actions + assertions interleaved  /  not both
```

`[[steps]]` and the `[[actions]]`/`[[assertions]]` pair are mutually exclusive — if `[[steps]]` is
non-empty it takes precedence and the other two are ignored.

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

**Manual path only.** `[[players]]` is ignored when `[game]` is present.

## `[game]` — new game path

Present the `[game]` section to trigger `Engine::new_game`, which runs real pregame automation:
shuffle, opening hand draw (CR 103.3–103.5), and London Mulligan. The `[[players]]` and `[[setup]]`
sections are ignored when `[game]` is set.

```toml
[game]
turn_order = [0, 1]      # u32 list, player ids in turn order (first = starting player CR 103.1)
                          # defaults to [[game.players]] order if omitted
rng_seed = 42             # u64, RNG seed for shuffles; defaults to top-level `seed` if omitted
```

### `[game.settings]`

All fields are optional — each overrides the corresponding default individually.

| Field | Type | Default | Description |
|---|---|---|---|
| `shuffle_libraries` | bool | `true` | Shuffle each library before drawing (CR 103.3). Set `false` for deterministic scenarios. |
| `starting_hand_size` | u8 | `7` | Cards drawn for opening hand and every redraw. |
| `skip_mulligan` | bool | `false` | Auto-keep all hands; no mulligan decisions needed. |
| `free_mulligans` | u32 | `0` | Mulligans that cost no card on the bottom (CR 103.5c grants 1 in Commander). |
| `max_mulligans` | u32 | (unlimited) | Hard cap on mulligans per player; omit for standard unlimited. |
| `starting_life` | u32 | `20` | Life total for all players (CR 103.4). |
| `poison_loss_threshold` | u32 | `10` | Poison counters required to lose (CR 704.5c). |
| `max_hand_size` | u32 | `7` | Cards allowed in hand at cleanup (CR 514.1). |

```toml
[game.settings]
shuffle_libraries = false
starting_hand_size = 7
skip_mulligan = false
free_mulligans = 1
max_mulligans = 3
starting_life = 40
poison_loss_threshold = 10
max_hand_size = 7
```

### `[[game.players]]`

Each entry defines one player's library. All fields except `id` are optional.

```toml
[[game.players]]
id = 0                                              # u32, required - becomes PlayerId(id)
library = ["bear", "bear", "forest", "forest"]      # card keys from [[cards]], top to bottom
commanders = ["general"]                            # optional, default [] - CR 903.6, see below
life_override = 40                                  # optional u32, overrides starting_life for this player only
```

`library` is a list of `[[cards]]` keys in top-to-bottom order. Keys may repeat for multiple
copies. If `shuffle_libraries = true` (default), the engine shuffles before drawing so the order
only matters for deterministic (`shuffle_libraries = false`) runs.

`commanders` (CR 903.6) is a list of `[[cards]]` keys placed in the command zone instead of the
library — a list, not a single key, since Partner commanders mean a player can legally have more
than one. `Engine::new_game` mints these with `is_commander: true` right after that player's
library (matching id-allocation order exactly), so card keys resolve the same way library keys do
(see below) once pregame completes.

**Card keys work on the new-game path.** The scenario crate reconstructs the object-id sequence
that `Engine::new_game` assigns (sequential, one per library card, in player/library order), then
remaps through the pregame ledger events — each `draw_opening_hand`/`return_hand_to_library`
logs a `ZoneChange` with the old id, so keys follow cards into their post-pregame zones. Duplicate
keys in a library track the last instance (same semantics as `[[setup]]`). This means
`assert_object_zone`, `activate_ability`, `play_land`, etc. all work with library card keys after
pregame completes.

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
power_script = "..."           # optional Lua source, default none - see Characteristic-defining below
toughness_script = "..."       # optional Lua source, default none - see Characteristic-defining below
mana_ability = ["G"]           # optional, default [] - see Mana abilities below
keywords = ["Flying", "Deathtouch"]  # optional, default [] - see Keywords below
trigger_condition = "..."      # optional Lua source, default none - see Triggered abilities below
trigger_operation = "..."      # optional Lua source, default none - see Triggered abilities below
script = "..."                 # optional full self-registering Lua source, default none - see below
```

**`mana_ability` (CR 605):** if non-empty, every instance of this card gets one
`RegisteredAbility { kind: Activated, effect: AddMana(...) }` set up automatically — activate it via
`activate_ability` with `ability_index = 0`. Each entry is one literal unit added (same letters as
`mana.units`, see below) — `["G"]` models `{T}: Add {G}`; `["G", "G"]` would model adding two green
in one activation. Only one mana ability per card this way, and the tap cost is implicit/hardcoded
— this TOML field can never express any other cost shape. A card with no `mana_ability` entries can
still be referenced by an `activate_ability` action, but it has no registered abilities, so
`ability_index = 0` will be rejected ("no such ability on source"). A card's own Lua `script` can
self-register a real mana ability too, including one with a cost beyond a bare tap (a Treasure/
Lotus-Petal-style sacrifice, say) — see [docs/lua/mana-abilities.md](lua/mana-abilities.md).

A `{2}{G}{G}` cost is `generic_cost = 2, colors = ["G", "G"]` — two list entries for the two green
pips, not `["G"]` with an implied count. `submit_payment`'s `mana.units` field (see that action's
docs below) pays this with a *different* shape on purpose — a flat list of literal units, not an
echo of this cost shape, since which mana actually pays the generic component is the payer's
choice (CR 118.13a), not implied by the cost.

**Simplification:** mana cost is `generic_cost` + `colors`, not a parsed cost string like
`"{2}{R}"` — no mana-cost-string parser exists anywhere yet. Can't currently express hybrid,
Phyrexian, or X costs on a test fixture. Extend `CardSpec` in `crates/scenario/src/lib.rs` if a
scenario needs one of those.

**`power_script`/`toughness_script` (CR 604.3/208.2a, characteristic-defining values):** if set,
`power`/`toughness` above is ignored for that characteristic — instead `CardDefinition.power`/
`.toughness` becomes `PTValue::CharacteristicDefining { script: Some(ScriptReference::Inline(...)) }`,
resolved at query time via `ScriptRuntime::invoke_cda` (CR 613 layer 7a, `layers.rs`). Raw Lua
source evaluating to `function(source, controller) -> integer`, same convention
`trigger_condition`/`trigger_operation` already use for a raw-source-string field. No `Game.*`
state-query bindings exist yet, so a real "count cards in your graveyard"-style CDA isn't
expressible in a scenario file yet — only a constant or something derived from `source`/`controller`
alone. A card with neither field set keeps the plain `power = N` shape as before. See
`crates/engine/tests/scenarios/phase5_layer_engine_cda.toml` for a worked example (a creature with
scripted toughness dying to CR 704.5g's lethal-damage SBA).

**`keywords` (CR 702):** printed keyword abilities, by name, parsed into a `KeywordSet` bitmask —
only the primitives that change how an existing engine checkpoint behaves, not the full ~140-entry
CR 702 catalog. Supported: `Flying`, `Reach`, `Menace`, `Vigilance`, `Haste`, `Defender`, `Flash`,
`Deathtouch`, `Indestructible`, `Lifelink`, `Wither`, `Infect`. Trample, First/Double Strike,
Hexproof/Shroud/Protection/Ward aren't modeled yet (see `docs/implementation.md`'s
keyword-primitives slice notes for why). Keywords that are really sugar for an activated/triggered
ability (Cycling, Persist, ...) don't belong in this list at all — use `mana_ability` above for a
mana ability, or extend `CardSpec` for anything else, since no keyword-name -> ability mapping
exists yet.

**`trigger_condition`/`trigger_operation` (CR 603, narrow script bridge):** if `trigger_operation` is
set, every instance of this card gets one `RegisteredAbility { kind: Triggered, .. }` set up
automatically, with both fields loaded via `Engine::load_condition`/`Engine::load_operation` (see
`script.rs`'s `ScriptRuntime`). `trigger_operation` alone is what registers the ability —
`trigger_condition` is optional and means "no filter, always triggers" when omitted (per
`RegisteredAbility::condition`'s doc). Only one triggered ability per card this way, same
single-ability-per-card simplification `mana_ability` uses.

Each field's source string can be either a `Game.Conditions.*`/`Game.Ops.*` one-liner (resolved
once at load time, no Lua VM calls afterward) or a full custom Lua function, invoked for real
every time. **See [docs/lua/index.md](lua/index.md) for the full authoring reference** — recognized
primitives, the custom-function calling convention, what event data a condition can see, and
what isn't wired up yet. Short example (an ETB "gain 1 life" trigger, primitive form):

```toml
trigger_condition = "return Game.Conditions.Enters"
trigger_operation = "return Game.Ops.GainLife(1)"
```

This shape is deliberately narrow — no targeting, no continuous effects, and registration is done
by the harness rather than the card's own script. See `script` below for real self-registration.

**`script` (CR 603, real self-registration):** a full card script — the `s.initial_effect(c)`
module convention, calling `Game.CreateTriggeredAbility(c)` itself — instead of the harness
building a `RegisteredAbility` by hand. Sets `CardDefinition::script_ref` directly; no
post-construction `Engine` step needed, since the real engine pipeline (`PlayLand`/
`ResolveStackTop`) runs it automatically once the card actually reaches the battlefield. **See
[docs/lua/index.md](lua/index.md) for the full authoring reference.** Example (same ETB "gain 1 life" ability
as above, self-registered instead of harness-wired):

```toml
script = """
local s = {}
function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"""
```

`Game.CreateStaticAbility(c)` self-registers a real `ContinuousEffect` the same way (CR 604/611)
— see [docs/lua/static-abilities.md](lua/static-abilities.md) and
`crates/engine/tests/scenarios/phase5_static_ability_self_registration.toml` for a worked example
(an anthem land whose own script buffs "creatures you control" the moment it's played).
`Game.CreateActivatedAbility(c)`/`Game.CreateReplacementEffect(c)`/`Game.CreateCostModifier(c)` are
real too — see [docs/lua/index.md](lua/index.md) for the full set. Nothing stops a card from
setting both `script` and `trigger_condition`/`trigger_operation` at once, but that's not a
supported combination — pick one per card.

**`continuous_effect` (CR 613, layer engine):** if set, every instance of this card gets one static
`ContinuousEffect` set up automatically, built directly as a Rust struct — a convenience field for
fixtures that don't need real script self-registration (`Game.CreateStaticAbility`, see `script`
above, now real). Only one continuous effect per card this way, same single-per-card
simplification `mana_ability`/`trigger_operation` use.

```toml
[cards.continuous_effect]
layer = "power_toughness_modifying"  # "type_changing" | "color_changing" | "ability_adding" |
                                      # "power_toughness_setting" | "power_toughness_modifying"
power = 1                            # i32, default 0 - power_toughness_* layers only
toughness = 1                        # i32, default 0 - power_toughness_* layers only
keywords = ["Indestructible"]        # ability_adding only, default [] - see Keywords above for names
types = ["Creature"]                 # type_changing only, default [] - same names as Card types below,
                                      # additive only (grants in addition to existing types)
colors = ["R"]                       # color_changing only, default [] - same letters as CardSpec.colors,
                                      # additive only (grants in addition to existing colors)
filter_controller = "you"            # "you" | "opponent" | "any", optional, default: no controller filter
filter_card_types = ["Creature"]     # optional, default [] (no card-type filter)
filter_self_only = false             # optional, default false - "just this effect's own source"
```

`layer` picks which of `layers.rs`'s five built primitives this is — only these five layers have
a real query path so far (CR 613's other layers — copy/control/text, plus 7a's
characteristic-defining values and 7d/7e — aren't wired to the layer engine yet; see `layers.rs`'s
own doc comment). Ordering *within* a layer is by registration timestamp, which a scenario file
can't control (always `0`) — write a direct Rust integration test
(`crates/engine/tests/phase5_layer_engine_basics.rs`) instead if a fixture needs to exercise
timestamp ordering between two effects in the same layer.

Example — an anthem enchantment giving `+1/+1` to creatures its controller controls:

```toml
[[cards]]
key = "anthem"
name = "Glorious Anthem"
types = ["Enchantment"]

[cards.continuous_effect]
layer = "power_toughness_modifying"
power = 1
toughness = 1
filter_controller = "you"
filter_card_types = ["Creature"]
```

See `crates/engine/tests/scenarios/phase5_layer_engine_basics.toml` for a full worked example
(the anthem above keeping a lethally-damaged bear alive through CR 704.5g's state-based action).
`phase5_layer_engine_consumers.toml` shows the same `power_toughness_modifying` primitive changing
real combat damage dealt (`assign_combat_damage`), not just an SBA — see
`docs/implementation.md`'s layer-engine-consumers slice notes.
`phase5_layer_engine_types_colors.toml` shows `type_changing` (CR 613 layer 4, additive only) —
pairing an `AddCardType(Creature)` effect with a `power_toughness_setting` effect animates a Land
into a creature that's then actually subject to CR 704.5g's lethal-damage SBA (two separate
`[[cards]]` sources, since one `ContinuousEffect` is one layer — see `docs/implementation.md`'s
layer-engine-types-colors slice notes).

**`target_requirement` (CR 115/601.2c-e, targeting system):** if set, casting this card as a
spell requires targets matching `filter` — `CastSpell` validates count/filter/dedup before the
spell is allowed onto the stack (CR 601.2e), and `ResolveStackTop` rechecks legality against
current game state at resolution (CR 608.2b) — if every target is now illegal, the spell fizzles
(doesn't resolve, removed from the stack, owner's graveyard) instead of running its effect.
`count`'s meaning depends on `up_to`/`at_least` (mutually exclusive, see `TargetRequirement`'s own
doc comment for the full three-way shape): exact by default, 0-to-`count` if `up_to` is set (CR
601.2c/115.6, "up to N"), `count`-or-more with no upper bound if `at_least` is set ("N or more";
"any number of target X" is `count = 0, at_least = true`).

```toml
[cards.target_requirement]
filter = "any"          # "object" | "player" | "any" (CR 115.4's any target: creature/player/
                         # planeswalker/battle)
card_types = ["Creature"] # "object" filter only, default [] (no card-type restriction)
excluded_card_types = ["Land"] # "object" filter only, default [] - CR 601.2c/115's negated form,
                         # e.g. "target nonland permanent"; independent of card_types above
subtypes = ["Human"]     # "object" filter only, default [] (no subtype restriction) - CR 205.4b,
                         # matches as an "or" across the list
excluded_subtypes = ["Wall"] # "object" filter only, default [] - CR 601.2c/115/205.4b's negated
                         # form, e.g. "target creature that isn't a Wall"; independent of subtypes
supertypes = ["Legendary"] # "object" filter only, default [] (no supertype restriction) - CR
                         # 205.4a, matches as an intersection against SupertypeSet
controller = "you"       # "object" filter only, optional: "you" | "opponent" | "any"
zone = "battlefield"     # "object" filter only, optional, default: battlefield (CR 115.2)
count = 1                # u32, required - see above for what it means with up_to/at_least
up_to = false            # optional, default false - CR 601.2c/115.6 "up to N"
at_least = false         # optional, default false - CR 601.2c/115.6 "N or more"; mutually
                         # exclusive with up_to
exclude_source = false   # optional, default false - CR 115.4 "another target": excludes the
                         # spell/ability's own source from ever being a legal candidate
```

See `crates/engine/tests/scenarios/phase5_targeting_basics.toml` for a worked example (an "any
target" instant cast targeting a battlefield creature, resolving normally since the target is
still legal) — the reject/fizzle paths (wrong count, filter mismatch, duplicate targets, a target
becoming illegal before resolution) are exercised exhaustively in
`crates/engine/tests/phase5_targeting_basics.rs` instead, not via TOML.

**`modes` (CR 700.2, modal spells):** if non-empty, this card is modal — casting it requires
`PlayerActionKind::CastSpell::modes` to supply `mode_count` valid indices into this list
(`mode_count`/`mode_up_to`/`mode_repeatable` below name how many and whether repeats are allowed;
`mode_count = 1` — the default — is CR 700.2a's "choose exactly one"). Indices must be distinct
unless `mode_repeatable = true` (CR 700.2d's reminder-text-gated "you may choose the same mode more
than once," e.g. Profane Command) — a repeated mode is treated as if it appeared that many times in
sequence, each instance getting its own target slot and (if the mode carries one) its own additional
cost. CR 700.2e (another player chooses) is real in the engine (`ModeChoice::ExactlyChosenByOpponent`/
`UpToChosenByOpponent`) but has no TOML field here — it needs a resolution-time pause
(`Engine::submit_mode_choice`) a scenario fixture's synchronous `run_action`/`run_payment` driving loop
has no entry point for yet. 700.2i (pawprint budgets) is still deferred entirely. A single
chosen mode's own `target_requirement`/effect are used instead of the card's top-level fields for the
rest of casting/resolution (CR 700.2c: a mode with no `target_requirement` casts as though it
required no targets at all, regardless of what other modes on the same card ask for); 2+ chosen
modes (including 2+ instances of one repeated mode) combine their own `target_requirement`s into one
multi-target cast and resolve in card-print order, narrowed to the synchronous `AbilityEffect` subset
`commands::is_modal_resolvable` accepts (a mode using an unsupported kind can't be combined with
another chosen mode — rejected at cast time). No TOML field for a mode's own effect or CR 700.2h
additional cost exists (see below), so a scenario fixture can only exercise the "choose exactly one"
(or "choose N, no effect the engine resolves") shape either way.

```toml
[cards]
# ...
mode_count = 2         # optional, default 1 - how many modes must be chosen
mode_up_to = true      # optional, default false - "choose up to mode_count" instead of exactly
mode_repeatable = true # optional, default false - CR 700.2d, allows repeated mode indices
```

```toml
[[cards.modes]]
# mode 0: no target_requirement — e.g. "You gain 3 life."

[[cards.modes]]
[cards.modes.target_requirement]
filter = "object"        # same TargetRequirementSpec shape as the top-level target_requirement
card_types = ["Creature"]
count = 1
```

No TOML field for a mode's own effect yet (same pre-existing gap as `CardSpec` itself having no
field for a Rust-native `AbilityEffect` — only reachable via direct `CardDefinition`/`SpellMode`
struct literals in a Rust integration test, e.g.
`crates/engine/tests/phase5_script_modal_spells.rs`). See
`crates/engine/tests/scenarios/phase5_modal_spell_toml.toml` for a worked example proving the
TOML → `CardDefinition.modes` → `CastSpell` mode-selection pipeline, one cast per mode.

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
controlled_since_turn = 0   # optional u32, default 0 - see Summoning sickness below
damage_marked = 0           # optional u32, default 0 - see Damage marking below
is_commander = false        # optional bool, default false - CR 903.3, see Commander below
```

**Damage marking (CR 704.5g):** `damage_marked` defaults to `0`. No combat-damage step (CR 510)
exists yet to produce this for real, so this is the only way to build a fixture that's already
lethally damaged — set it to (or above) the card's `toughness` to have CR 704.5g's state-based
action destroy it the next time anything runs `check_state_based_actions` (e.g. the very first
`pass_priority`). Same "fixture stands in for a producer that doesn't exist yet" pattern as
`controlled_since_turn` above.

**Summoning sickness (CR 302.6/602.5a):** `controlled_since_turn` defaults to `0`, which is always
less than any real `turn_number` (the game's first turn seeds `last_active_turn_number` to `1` at
setup time) - so a setup card is **never** summoning sick by default. To build a fixture that *is*
sick (a creature that just entered the battlefield this turn), set this to the scenario's current
`turn_number`. No haste override exists - haste isn't modeled in this engine yet.

**Commander (CR 903.3):** `is_commander` defaults to `false`. Set it `true` (typically paired with
`zone = "command"`) to mark that specific setup instance as its player's commander — CR 903.8
lets its owner cast it from the command zone like any other spell, no alternative cost needed. Not
enforced against `zone` here — casting/legality checks are the engine's job (`CastSpell`), not the
scenario loader's, same "engine trusts the setup" stance every other `[[setup]]` field takes. CR
903.9a (owner may return a commander from graveyard/exile to the command zone) has no scenario
action yet — drive it directly via `Engine::submit_commander_return_choice` in a Rust test instead.

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

**Manual path only.** `[[setup]]` is ignored when `[game]` is present.

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

CR 508.1: the active player declares attackers, only during the declare attackers step, only once
per combat. Each attacker must be a creature (not also a battle) controlled by `player`, untapped,
and not summoning sick (see `controlled_since_turn` above - no haste override). Each `target` must
be an opponent (player) or a permanent they control that's a planeswalker or battle. Validates the
whole list before tapping anything - one illegal entry rejects the entire declaration (no partial
taps). Tapping the attackers and recording them happens immediately, no stack object involved.

```toml
[[actions]]
type = "declare_blockers"
player = 1
blocks = [
  { blocker = "wall", attacker = "bear" },
]
```

CR 509.1: the defending player (in this engine: whichever player in `turn_order` isn't
`active_player` - multiplayer's "which defending player" ambiguity isn't modeled) declares
blockers, only during the declare blockers step, only once per combat, only after attackers have
been declared. Each blocker must be a creature controlled by `player`, untapped, and can't appear
twice in the same `blocks` list. Each referenced `attacker` must actually be attacking that
defending player (or their planeswalker/battle).

**Neither action requires `player` to hold priority** - CR 508.1/509.1 call these turn-based
actions that explicitly don't use the stack, so they're exempt from the priority check every other
action goes through (`Engine::submit_action`). `execute_command`'s own active-player/
defending-player checks are the real gate. See
`crates/engine/tests/scenarios/phase5_combat_declarations.toml` for a full worked example, including
the pass-priority sequence needed to reach the declare attackers/blockers steps with two players
(every step transition needs *both* players to pass in a row, not just one, unlike the solo-player
examples elsewhere in this doc).

```toml
[[actions]]
type = "assign_combat_damage"
player = 0
assignments = [
  { attacker = "bear", splits = [
    { blocker = "wall1", amount = 3 },
    { blocker = "wall2", amount = 2 },
  ] },
]
```

CR 510.1/510.2: the active player assigns and deals all combat damage in one turn-based action,
only during the combat damage step (also exempt from the priority check, same as
`declare_attackers`/`declare_blockers` above). `assignments` only needs an entry for an attacker
with **two or more** blockers - this engine's block model is one blocker/one attacker (no banding),
so an unblocked attacker always sends its full power to its original target, and a single-blocked
attacker/its lone blocker always send their full power to each other; neither has a real choice to
submit. An `assignments` entry must cover *exactly* the attackers that are actually double(+)-
blocked (no extra entries for 0/1-blocker attackers, no missing entries for one that needs a split),
each `splits` list must name every one of that attacker's blockers exactly once, and the `amount`s
must sum to exactly the attacker's power (CR 510.1e - an illegal split rejects the whole action,
nothing gets marked). Damage is marked on creature targets and converted to life loss on player
targets in the same pass - no damage prevention/replacement check yet (CR 615/616), and damage to a
planeswalker/battle target doesn't do anything yet (no loyalty/defense-damage path wired up - only
player and creature targets are functional this slice). See
`crates/engine/tests/scenarios/phase5_combat_damage.toml` for a full worked example (attack, double
block, explicit split, lethal/non-lethal outcomes).

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

```toml
[[actions]]
type = "submit_discard"
player = 0
cards = ["c1", "c2"]   # card keys to discard; list may be empty for []
```

Answers a pending `ProcessState::AwaitingDiscard` pause (CR 514.1) — i.e. the action triggered when
the active player's hand exceeds `max_hand_size` (normally 7) at the start of Cleanup. The number of
keys in `cards` must equal exactly `hand_size - max_hand_size` (the "excess" count the engine
computed); too many, too few, duplicates, or a key not in hand are all rejected with a `DiscardError`
and reported as `Errored` in the scenario output. Same "own entry point" design as `submit_payment`
— not routed through `submit_action`. Each card key must have a unique `[[setup]]` entry (if two
setups share the same key, only the last ObjectId is tracked and only that instance can be named in
`cards`).

### Pregame actions (new-game path only)

These three action types answer mulligan decisions during `ProcessState::AwaitingMulliganDecision`
and `ProcessState::AwaitingBottomCardChoice`. They route through `Engine::submit_mulligan_decision`
and `Engine::submit_bottom_cards` respectively — not `submit_action`.

The engine pauses at `AwaitingMulliganDecision` for the player whose turn it is to decide. London
Mulligan round order (CR 103.5): starting player decides first, then each other player in turn order.
After everyone has decided once, any player who took a mulligan decides again (and so on). Once all
players have kept, the engine pauses for bottoming (one player at a time, in turn order, for each
player who took paid mulligans).

```toml
[[steps]]
type = "keep_hand"
player = 0     # u32, must match the player the engine is currently waiting on
```

Keeps the current hand. Sets `hand_kept = true`; player won't be asked again. If this player took
any paid mulligans, they'll be asked to bottom cards after all players have decided.

```toml
[[steps]]
type = "take_mulligan"
player = 0     # u32, must match the player the engine is currently waiting on
```

Returns hand to library (CR 103.5b), shuffles, draws `starting_hand_size` cards, increments
`mulligans_taken`. Player will be asked to decide again on the next round. Rejected if
`max_mulligans` is set and already reached.

```toml
[[steps]]
type = "bottom_cards"
player = 0       # u32, must match the player the engine is currently waiting on
first_n = 1      # usize, how many cards to bottom; must equal paid mulligan count for this player
```

Answers a `ProcessState::AwaitingBottomCardChoice` pause. Takes the first `first_n` cards from the
player's hand zone (positional — hand zone order) and puts them on the library bottom. The count
must equal `max(0, mulligans_taken - free_mulligans)` exactly; wrong counts are rejected. Use
`first_n = 0` only when `mulligans_taken <= free_mulligans` (no cards owed).

**`bottom_cards` selects by position, not by card key** — it takes the first `first_n` cards in
hand zone order regardless of which specific cards they are. The scenario states how many go to
the bottom, not which ones.

### Targets

Anywhere a target is expected (`targets`, `selection`, attacker `target`), write either:

- a player id as a bare number: `0` (becomes `Target::Player(PlayerId(0))`)
- a card key as a string: `"bear"` (becomes `Target::Object(<that card's id>)`)

```toml
targets = [0, "bear"]   # a player and a card, mixed, both valid
```

## `[[steps]]` — interleaved actions and assertions

Use `[[steps]]` instead of separate `[[actions]]`/`[[assertions]]` when you need assertions to
fire at specific points mid-sequence rather than all at the end.

Action entries use the same `type` names as `[[actions]]`. Assertion entries use `assert_`-prefixed
names so they're visually distinct when reading the file. Both share the same field shapes as their
`[[actions]]`/`[[assertions]]` counterparts.

```toml
[[steps]]
type = "play_land"          # action — same as [[actions]]
player = 0
card = "forest"

[[steps]]
type = "assert_object_zone" # assertion fires here, before the next action
card = "forest"
equals = "battlefield"

[[steps]]
type = "pass_priority"
player = 0

[[steps]]
type = "assert_phase"
equals = "main"
```

**If `[[steps]]` is non-empty, `[[actions]]` and `[[assertions]]` are ignored.** They cannot be
mixed in the same file — pick one style per scenario.

Assertion type names for `[[steps]]` (all take the same fields as their `[[assertions]]`
equivalents): `assert_life`, `assert_ledger_len`, `assert_object_zone`, `assert_mana_pool`,
`assert_tapped`, `assert_eliminated`, `assert_phase`, `assert_step`, `assert_active_player`,
`assert_priority_player`, `assert_hand_size`, `assert_object_counters`, `assert_player_counters`.

---

## `[[assertions]]`

Checked once, after every action has run. Tagged by `type`.

> **Tip:** if you need assertions at specific points mid-sequence, use `[[steps]]` instead (see above).

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
type = "eliminated"
player = 1
equals = true
```

Checks `PlayerState.eliminated` — the way to confirm CR 704.5a's state-based action actually fired
for a player at 0 or less life. See `crates/engine/tests/scenarios/phase5_sba_basics.toml` for a
full worked example of both this and the `damage_marked` setup field above, covering CR 704.5a/f/g
in one scenario.

```toml
[[assertions]]
type = "object_counters"
card = "wall1"
kind = "minus_one_minus_one"   # plus_one_plus_one/minus_one_minus_one/loyalty/poison/energy/charge, or any other string (Custom)
equals = 3
```

Checks `GameObject.counters` — the way to confirm Wither/Infect put `-1/-1` counters on a creature
instead of marking damage.

```toml
[[assertions]]
type = "player_counters"
player = 1
kind = "poison"
equals = 3
```

Checks `PlayerState.counters` — the way to confirm Infect gave a player poison counters instead of
life loss. See `crates/engine/tests/scenarios/phase5_keyword_primitives.toml` for a full worked
example of both alongside the other keyword primitives.

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

```toml
[[assertions]]
type = "hand_size"
player = 0
equals = 7
```

Checks the number of cards in `player`'s hand zone. Useful after a `submit_discard` to confirm the
active player discarded down to exactly `max_hand_size`, or after a mulligan sequence to confirm the
opening hand size.

**A card's key keeps resolving correctly across zone moves.** `play_land`/`cast_spell`/resolution
all mint a *new* `ObjectId` for the moved object (CR 400.7) instead of reusing the old one — every
`[[setup]]` card key is tracked and automatically re-pointed at its new id after each action that
moves it (`remap_moved_card_ids` in `crates/scenario/src/lib.rs`), so `object_zone`/later actions
referencing `"bear"` keep working after it's been cast, resolved, etc. You don't need to do anything
for this — just keep using the same key from `[[setup]]` throughout the file.

## Example: full file (manual path)

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

## Example: new-game path with London Mulligan

Uses `[game]` to trigger `Engine::new_game`. P0 takes a mulligan, P1 keeps, P0 keeps — P0 then
bottoms 1 card (paid mulligan). Assertions use `assert_hand_size` since per-card keys aren't
available on this path.

```toml
seed = 42

[[cards]]
key = "bear"
name = "Grizzly Bears"
types = ["Creature"]
power = 2
toughness = 2

[game]
turn_order = [0, 1]
rng_seed = 42

[game.settings]
shuffle_libraries = false    # deterministic for test

[[game.players]]
id = 0
library = ["bear", "bear", "bear", "bear", "bear", "bear", "bear"]

[[game.players]]
id = 1
library = ["bear", "bear", "bear", "bear", "bear", "bear", "bear"]

# P0 is starting player — decides first
[[steps]]
type = "take_mulligan"
player = 0

# P1 decides (still first round)
[[steps]]
type = "keep_hand"
player = 1

# P0 decides again (took a mulligan — gets another turn)
[[steps]]
type = "keep_hand"
player = 0

# P0 bottoms 1 card (1 paid mulligan, free_mulligans = 0 by default)
[[steps]]
type = "bottom_cards"
player = 0
first_n = 1

# P0 hand = 7 drawn - 1 bottomed = 6
[[steps]]
type = "assert_hand_size"
player = 0
equals = 6

# P1 hand = full 7 (no mulligans)
[[steps]]
type = "assert_hand_size"
player = 1
equals = 7

[[steps]]
type = "assert_phase"
equals = "beginning"
```

## Example: new-game path with skip_mulligan

For scenarios that don't care about the mulligan phase and just want to get into gameplay:

```toml
seed = 1

[[cards]]
key = "bear"
name = "Test Bear"
types = ["Creature"]
power = 2
toughness = 2

[game]
turn_order = [0, 1]

[game.settings]
shuffle_libraries = false
skip_mulligan = true      # auto-keeps all hands, no decisions needed

[[game.players]]
id = 0
library = ["bear", "bear", "bear", "bear", "bear", "bear", "bear"]

[[game.players]]
id = 1
library = ["bear", "bear", "bear", "bear", "bear", "bear", "bear"]

# Engine jumps straight to Beginning phase — no mulligan steps needed
[[assertions]]
type = "hand_size"
player = 0
equals = 7

[[assertions]]
type = "phase"
equals = "beginning"
```

## Extending the format

The schema lives in `crates/scenario/src/lib.rs` (`ScenarioFile` and friends — `PlayerSpec`,
`CardSpec`, `SetupEntry`, `ActionSpec`, `AssertionSpec`, `GameSpec`, `GamePlayerSpec`,
`GameSettingsSpec`). Adding a new action or assertion variant is: add the enum variant + its
TOML-facing fields, add the matching arm in `to_player_action` or `check_assertion`, update this
doc. Update this doc in the same change as the code — don't let it drift.
