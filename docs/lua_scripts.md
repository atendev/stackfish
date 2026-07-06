# Lua Scripts

Reference for writing the Lua that powers a card's abilities — condition/operation source
strings, what's bound into the VM today, and what isn't yet. Written from the card-author's
side; see `docs/DESIGN.md`'s "Scripting model" section for the engine-side rationale, and
`docs/implementation.md`'s script-bridge slice notes for how this was built (and why some things
below are stubbed the way they are).

**Status:** narrow and real, not a placeholder. Everything documented here actually runs through
`ScriptRuntime` (`crates/engine/src/script.rs`) and is exercised by `cargo test --workspace`. It
is *not* the full `Game.*` API `DESIGN.md` describes — see "Not available yet" at the bottom
before assuming something works.

## The condition/operation split

Per `docs/ygo_pro_reference.md`'s effect lifecycle (condition/cost/target/operation as separate
callbacks, not one monolithic function), a `RegisteredAbility` keeps each concern separate so the
engine can check one without committing to another. Only **condition** and **operation** are
real this slice — **cost** and **target** callbacks exist as `Option<ScriptHandle>` fields but
are never invoked yet (see "Not available yet").

- **condition** — did this ability's trigger event happen? Runs at `CollectTriggers` time,
  once per matching event, and must be a pure yes/no check.
- **operation** — what does the ability actually do? Runs once, when the ability resolves off
  the stack (`ResolveStackTop`).

A source string for either slot resolves at *load time* (not every time it's checked/resolved)
into one of two shapes:

1. **A `Game.Conditions.*` / `Game.Ops.*` primitive** — a recognized one-liner. Decoded once,
   then answered entirely on the Rust side forever after — zero Lua VM calls at trigger-check or
   resolve time. Fastest, but only covers what's been wired (see the tables below).
2. **A custom function** — `return function(...) ... end`. Invoked for real through the Lua VM
   every time. Slower, but expresses anything the primitive tables don't cover yet.

Nothing else is accepted. A condition/operation string that evaluates to `nil` (e.g. a typo'd
`Game.Conditions.Typo` — indexing a Lua table with an unknown key silently yields `nil`, not an
error) is a **load-time error**, not silently treated as "always" or a no-op. If you're not sure
which shape you wrote, load-time will tell you immediately.

## Writing a condition

**Primitive** (fast path, use when it covers your case):

| Lua | Meaning |
|---|---|
| `return Game.Conditions.Enters` | CR 603 "when this enters the battlefield" — matches a zone change moving this ability's source onto the battlefield. |
| `return Game.Conditions.Always` | No filter — matches every event. Write this explicitly for an unconditional trigger; don't rely on `nil`/typos meaning the same thing. |

**Custom function** (general case):

```lua
return function(source, controller, event)
    return event ~= nil
       and event.kind == "zone_change"
       and event.object == source
       and event.to == "battlefield"
end
```

Called with `(source, controller, event)`:
- `source` — the ability's source object id (a plain integer, `ObjectId.0`).
- `controller` — the source's controller (a plain integer, `PlayerId.0`).
- `event` — the event being checked, bridged into a Lua table, or `nil`. **Only
  `GameEventKind::ZoneChange` is bridged today** — every other event kind (damage dealt, life
  changed, tapped, ...) arrives as `nil`. A `ZoneChange` table has:
  - `event.kind` — always the string `"zone_change"`.
  - `event.object` — the object id that moved (an integer).
  - `event.from` / `event.to` — the zone it moved out of/into, as a lowercase string
    (`"library"`, `"hand"`, `"battlefield"`, `"graveyard"`, `"stack"`, `"exile"`, `"command"`).

Must return a plain boolean. Must not attempt to read broader game state — nothing beyond
`source`/`controller`/`event` crosses into Lua right now (see "Hidden information" below).

## Writing an operation

**Primitive** (fast path):

| Lua | Meaning |
|---|---|
| `return Game.Ops.GainLife(n)` | The controller gains exactly `n` life (CR 119). `n` is a plain integer argument. |

**Custom function** (general case) — "Lua asks, Rust decides": the function never mutates
anything itself. It returns a plain table describing *intent*, and the engine
(`ScriptRuntime::invoke_operation`) decodes recognized keys and applies the mutation on the Rust
side.

```lua
return function(source, controller)
    return { gain_life = 1 }
end
```

Called with `(source, controller)` — no `event` parameter (an operation isn't re-checking
whether it should run; that's the condition's job). Recognized keys in the returned table:

| Key | Applies |
|---|---|
| `gain_life` | Integer. `PlayerState::gain_life(amount)` on `controller`. |

Any other key is silently ignored (no error) — there's no "unrecognized key" validation yet.
This is the same limitation `Game.Ops.GainLife` has as a primitive: only this one effect shape
exists. Extend `ScriptRuntime::invoke_operation` (for the custom-function key) and
`AbilityEffect`/`Game.Ops` (for the primitive) together when a card needs a new effect shape —
see "Adding a new primitive" below.

## Full worked example

An ETB "gain 1 life" creature, primitive form (see
`crates/engine/tests/scenarios/phase5_script_bridge_primitives.toml` for the full runnable
scenario this is drawn from):

```toml
[[cards]]
key = "bear"
name = "Test Bear"
types = ["Creature"]
power = 2
toughness = 2
trigger_condition = "return Game.Conditions.Enters"
trigger_operation = "return Game.Ops.GainLife(1)"
```

The same ability, written the long way with custom functions (identical observable behavior —
see `crates/engine/tests/phase5_script_bridge_primitives.rs`'s
`primitive_and_script_abilities_both_fire_in_the_same_game` test, which runs both forms side by
side in the same game to confirm they don't interfere):

```toml
trigger_condition = """
return function(source, controller, event)
    return event ~= nil and event.kind == "zone_change"
       and event.object == source and event.to == "battlefield"
end
"""
trigger_operation = """
return function(source, controller)
    return { gain_life = 1 }
end
"""
```

See `docs/scenario_format.md` for the full `CardSpec` TOML schema `trigger_condition`/
`trigger_operation` live in, including the "one triggered ability per card" limitation that
schema currently has (a scenario-harness limitation, not a Lua one).

## How registration actually happens today

There is **no self-registration** yet — a card's own Lua script cannot call
`Game.RegisterTriggeredAbility` to register itself (see "Not available yet"). Every
`RegisteredAbility` in the engine today is built Rust-side:

1. Load the condition string via `Engine::load_condition(source)` -> `ConditionSpec`.
2. Load the operation string via `Engine::load_operation(source)` -> `(Option<AbilityEffect>, ScriptHandle)`.
3. Push a `RegisteredAbility { source, kind: AbilityKind::Triggered, condition: Some(condition),
   operation, effect, cost: None, target: None }` onto `GameState.abilities` directly.

The scenario TOML harness (`crates/scenario/src/lib.rs`'s `register_trigger_ability`) does
exactly this for `trigger_condition`/`trigger_operation` fields, once the `Engine` (and its
`ScriptRuntime`) exists. A hand-written Rust test can do the same thing directly — see any test
in `crates/engine/tests/phase5_script_bridge_primitives.rs` for the pattern.

## Where this is headed: one script file per card

`CardDefinition.script_ref` is a `ScriptReference` (`types/card.rs`) — either `Inline(String)`
(a literal Lua source string, what every example above uses) or `ByKey(String)` (a lookup key
into an external card-database project's script registry, per `DESIGN.md`'s "no card database
tooling" non-goal — this engine consumes that registry, it doesn't author or lay it out).
`ByKey` is the shape a real card database will use: one `.lua` file per card, looked up by key,
loaded once via `ScriptRuntime::load_card_script` and cached per `CardDefinition` (not
per-`GameObject` instance).

**This isn't runnable yet** — `load_card_script` is `todo!()`, and so is the self-registration
path it depends on (`Game.RegisterTriggeredAbility` isn't bound into the Lua VM; see "Not
available yet"). But the *shape* of that eventual file is worth writing down now, since it's
what today's `trigger_condition`/`trigger_operation` snippets are standing in for. Following
`docs/ygo_pro_reference.md`'s `s.initial_effect(c)` convention (one Lua module per card, effects
built and registered inside a single setup function, condition/operation broken out as named
functions rather than written inline):

```lua
-- test_bear.lua
-- CardDefinition.script_ref = ScriptReference::ByKey("test_bear")
--
-- Test Bear
-- Creature - Bear, 2/2
-- When Test Bear enters the battlefield, you gain 1 life.

local s = {}

function s.initial_effect(c)
    -- c is this card's own object id (the `source` every condition/operation
    -- below receives as their first argument) - self-registers one
    -- triggered ability using the same primitives documented above.
    Game.RegisterTriggeredAbility(c, Game.Conditions.Enters, nil, nil, Game.Ops.GainLife(1))
end

return s
```

A card whose ability needs more than the primitive tables cover falls back to named custom
functions instead — same `condition`/`operation` split, just written out instead of a one-liner
(mirroring `s.cond`/`s.op` in the ygopro reference):

```lua
-- some_other_card.lua
-- CardDefinition.script_ref = ScriptReference::ByKey("some_other_card")

local s = {}

function s.initial_effect(c)
    Game.RegisterTriggeredAbility(c, s.condition, nil, nil, s.operation)
end

function s.condition(source, controller, event)
    return event ~= nil and event.kind == "zone_change"
       and event.object == source and event.to == "battlefield"
end

function s.operation(source, controller)
    return { gain_life = 1 }
end

return s
```

Caveats on both examples, so they don't get mistaken for a locked-in API:

- `Game.RegisterTriggeredAbility`'s exact calling convention isn't finalized. `script_api.rs`'s
  Rust-side signature (`source, condition, cost, target, operation`) is what these examples
  mirror positionally, but its own doc comment flags a real open question (no `AbilityKind`
  parameter yet, despite the function nominally covering triggered/activated/static abilities) —
  the shape above is illustrative of the intended ergonomics, not something to code against yet.
- Calling `Game.RegisterTriggeredAbility` *from inside a running Lua callback* needs `&mut
  GameState` reachable at registration time — the same interior-mutability-during-script-
  execution problem flagged since Phase 2's script bridge notes, still unsolved. This is the
  actual blocker keeping self-registration out of scope, not just the binding being unwritten.
- File naming/location (`test_bear.lua`, one file per card key) is illustrative, not a decision
  this engine makes — per `DESIGN.md`'s non-goals, that layout belongs to whichever external
  project maintains the card database, and `ScriptReference::ByKey`'s own doc comment is
  deliberately silent on how a key resolves to source.

## Hidden information

A condition function only ever receives `source`/`controller`/`event` as plain values — the full
`GameState` is *not* passed into Lua. This is stricter than `ScriptContext.state`'s own doc
comment might suggest (it still holds `&GameState` on the Rust side, flagged there as an
unresolved hidden-information leak) — in practice, nothing beyond those three arguments crosses
the Rust/Lua boundary today, so there's currently no way for a condition script to read an
opponent's hand or library contents even accidentally. This is incidental (a side effect of how
narrow `event_to_lua` is), not a designed guarantee — don't rely on it staying this narrow
forever, and don't add a Lua-visible full-state argument without re-reading
`ScriptContext`'s doc comment and `DESIGN.md`'s open question about it first.

## Not available yet

Don't spend time trying these — they're signatures only, or explicitly unbound:

- **`Game.RegisterTriggeredAbility`, `Game.AddContinuousEffect`, `Game.ReplaceEvent`,
  `Game.CastSpell`, `Game.PayCost`, `Game.SelectTargets`** (`script_api.rs`) — none of these are
  bound into the Lua VM's globals. A card script cannot call any of them. Only the
  `Game.Conditions.*`/`Game.Ops.*` sugar tables documented above exist in `Game` right now.
- **Cost and target callbacks** — `RegisteredAbility.cost`/`.target` exist as fields but
  `ScriptRuntime::invoke_cost`/`invoke_target` are both `todo!()`. Every trigger exercised so far
  has no cost and no targets.
- **Activated (non-mana) abilities** — `commands::execute_command`'s `ActivateAbility` arm still
  `todo!()`s for anything other than a CR 605 mana ability (see `mana_ability` in
  `docs/scenario_format.md`, which doesn't go through Lua at all). A `Game.Ops.GainLife`-style
  primitive on an *activated* (not triggered) ability can't resolve yet — there's nowhere for
  `ActivateAbility` to push a `StackObject` for it.
- **Continuous effects and replacement effects** — `ContinuousEffect::applies_to` and
  `ReplacementEffect::matches` are both `todo!()`. No "as long as you control a Forest" or
  "if a source would deal damage, prevent it" style effect can be checked yet, scripted or not.
- **Any event kind other than `ZoneChange`** — `event_to_lua` (`script.rs`) only bridges zone
  changes. A condition wanting to check damage/life-change/tap events will see `nil` and must
  wait on `event_to_lua` being extended.
- **Player-input mid-script** (asking the player to choose something from inside a script) —
  needs Lua coroutines spanning multiple `Engine::step()` calls, not designed yet. See
  `DESIGN.md`'s open questions.

## Adding a new primitive

To add e.g. `Game.Ops.DrawCards(n)`:

1. Add a variant to `AbilityEffect` (`types/ability.rs`), e.g. `DrawCards(i32)`.
2. Register a factory function on the `Game.Ops` table in `script.rs`'s `bind_game_globals`,
   mirroring `GainLife`'s `lua.create_function(|_, amount: i32| Ok(OperationTag(...)))`.
3. Add the matching apply arm in `ScriptRuntime::invoke_operation` (the custom-function/table-key
   path) *and* wherever the primitive's `Some(AbilityEffect::...)` fast path is dispatched
   (`engine.rs`'s `ResolveStackTop` `TriggeredAbility` branch) — both paths need to produce the
   same observable effect, since either one might be what a given ability ends up using.

Adding a new `Game.Conditions.*` primitive follows the same shape: a new `ConditionPrimitive`
variant, a Lua table entry in `bind_game_globals`, and the matching arm in
`ConditionPrimitive::matches`.
