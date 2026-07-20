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
engine can check one without committing to another. **condition**, **cost**, and **operation** are
all real — **cost** (`RegisteredAbility.cost: Option<CostSpec>`) is built from `Game.Costs.*`
primitives via `ActivatedAbilityBuilder`, not a script callback at all (there's no `Script`
fallback shape for costs, see "Not available yet"). **target** stays unreal — it exists as an
`Option<TargetSpec>` field but nothing sets it from Lua yet.

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

Two ways, both real, coexisting:

**Harness-wired** (`trigger_condition`/`trigger_operation`, prior slice) — the test/scenario
harness builds the `RegisteredAbility` by hand, Rust-side:

1. Load the condition string via `Engine::load_condition(source)` -> `ConditionSpec`.
2. Load the operation string via `Engine::load_operation(source)` -> `(Option<AbilityEffect>, ScriptHandle)`.
3. Push a `RegisteredAbility { source, kind: AbilityKind::Triggered, condition: Some(condition),
   operation, effect, cost: None, target: None }` onto `GameState.abilities` directly.

`crates/scenario/src/lib.rs`'s `register_trigger_ability` does exactly this for
`trigger_condition`/`trigger_operation` fields. Still fully supported — see any test in
`crates/engine/tests/phase5_script_bridge_primitives.rs` for the pattern.

**Real self-registration** (`script`, this slice) — the card's own Lua calls
`Game.CreateTriggeredAbility(c)` itself; nothing outside the script builds a `RegisteredAbility`
by hand. See the next section.

## Real self-registration: a full card script

`CardDefinition.script_ref` is a `ScriptReference` (`types/card.rs`) — either `Inline(String)`
(a literal Lua source string) or `ByKey(String)` (a lookup key into an external card-database
project's script registry, per `DESIGN.md`'s "no card database tooling" non-goal — this engine
consumes that registry, it doesn't author or lay it out; `ByKey` stays unimplemented here, see
"Not available yet"). `ScriptRuntime::run_card_script` loads `Inline` source, evaluates it to a
Lua table with a callable `initial_effect` field (the `s.initial_effect(c)` module convention,
per `docs/ygo_pro_reference.md`), and calls it with the object's own id — real, running, tested
(`crates/engine/tests/phase5_script_self_registration.rs`):

```lua
-- test_bear.lua
-- CardDefinition.script_ref = ScriptReference::Inline(<this file's contents>)
--
-- Test Bear
-- Creature - Bear, 2/2
-- When Test Bear enters the battlefield, you gain 1 life.

local s = {}

function s.initial_effect(c)
    -- c is this card's own object id - Game.CreateTriggeredAbility(c) returns
    -- a builder mirroring ygopro's Effect.CreateEffect(c)/e1:SetCondition(...)
    -- /c:RegisterEffect(e1) convention.
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end

return s
```

`:SetCondition`/`:SetOperation` accept either shape from the tables above — a primitive tag or a
custom function:

```lua
function s.initial_effect(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(function(source, controller, event)
        return event ~= nil and event.kind == "zone_change"
           and event.object == source and event.to == "battlefield"
    end)
    e:SetOperation(function(source, controller)
        return { gain_life = 1 }
    end)
    e:Register()
end
```

Omitting `:SetCondition` before `:Register()` means unconditional (same meaning `condition: None`
already has). Calling `:Register()` with no `:SetOperation()` set is a real error — an ability
needs an operation. A script can call `Game.CreateTriggeredAbility(c)` more than once inside one
`initial_effect` to register more than one ability on the same card — this isn't a special case,
it falls out of the mechanism for free (each `:Register()` independently queues its own
`RegisteredAbility`).

`kind` is implied by which constructor you called (`Game.CreateTriggeredAbility` here) rather
than a settable field — `Game.CreateActivatedAbility` is the natural remaining future sibling
(`Game.CreateStaticAbility` now exists for real, see the next section). `:SetCost`/`:SetTarget`
aren't bound on the builder either — `invoke_cost`/`invoke_target` are still `todo!()`
regardless, so nothing would consume them yet.

**Registration timing (CR 400.7):** a card's script re-runs — unconditionally, every time,
no "first time only" check — at exactly the two points a *new* battlefield object id is minted:
`PlayLand`'s hand→battlefield move, and `ResolveStackTop`'s permanent-entry branch
(stack→battlefield, after casting). This matches CR 400.7's "an object that moves from one zone
to another becomes a new object with no memory of its previous existence" literally — the
previous id's `RegisteredAbility`/`ContinuousEffect` (if any) is purged once that id is retired
(`commands::retire_object`, called at every zone-change site that doesn't otherwise carry the
ability/effect forward via `remap_abilities` — e.g. SBA-driven death), so a card that returns to
the battlefield later gets a genuinely fresh registration, not a stale leftover. Abilities keyed
off a *pre-battlefield* zone (a cast trigger, a graveyard-based ability like flashback) aren't in
scope — self-registration only runs at the two "entering the battlefield" points above.

Scenario TOML fixtures can use this directly via `CardSpec.script` — see `docs/scenario_format.md`
and `crates/engine/tests/scenarios/phase5_script_self_registration.toml`.

## Self-registering a static ability

`Game.CreateStaticAbility(c)` is the CR 604/611 sibling of `Game.CreateTriggeredAbility` — same
queued-registration mechanism (`StaticAbilityBuilder` next to `TriggeredAbilityBuilder` in
`script.rs`), producing a real `ContinuousEffect` pushed onto `GameState.continuous_effects`
instead of a `RegisteredAbility`:

```lua
-- Anthem: creatures you control get +1/+1.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected({ controller = "you", card_types = { "Creature" } })
    e:Register()
end

return s
```

`Game.Layers` only exposes the 5 layers with a real query path in `layers.rs` —
`TypeChanging`/`ColorChanging`/`AbilityAdding`/`PowerToughnessSetting`/`PowerToughnessModifying`.
Layers 1/2/3/7d/7e aren't offered at all (see `Layer`'s own doc comment in `types/effect.rs` for
why each is unbuilt).

`Game.Effects` mirrors `Game.Ops`'s one-factory-per-shape convention: `SetPT(power, toughness)`,
`ModifyPT(power, toughness)`, `AddKeyword(name)`, `AddCardType(name)`, `AddColor(letter)`,
`AddProtectionColor(letter)`, `AddProtectionCardType(name)`, `AddProtectionSubtype(name)`,
`AddProtectionName(name)`, `AddProtectionPlayer(who)` — each call returns a tagged value for
`:SetEffect`. Three shapes take no arguments at all and are bare values instead of functions to
call — `Game.Effects.SwitchPT` (layer 7e), `Game.Effects.RemoveAllAbilities` (layer 6, CR 613.1f's
ability-removing effects — "~ loses all abilities"; strips every keyword *and* all six protection
fields accumulated so far in the layer-6 fold, though a later-timestamp add-shape still applies on
top — see `EffectPrimitive::RemoveAllAbilities`'s own doc comment for why no special CR 613.8
dependency case is needed for that), and `Game.Effects.AddProtectionEverything` (CR 702.16j, see
below). `:Register()` rejects a layer/effect pairing that doesn't match (e.g.
`SetLayer(TypeChanging)` with `SetEffect(ModifyPT(...))`) with a real error, rather than silently
building a `ContinuousEffect` the layer engine could never actually apply — this check only runs
for a `Game.Effects.*` tag, see the script-backed form below for why.

`AddProtectionColor`/`AddProtectionCardType`/`AddProtectionSubtype`/`AddProtectionName`/
`AddProtectionEverything`/`AddProtectionPlayer` (CR 702.16, the color (702.16e), card-type,
subtype, name (702.16a), "everything" (702.16j), and player (702.16k) qualities — the complete CR
702.16 quality list this engine models) all pair with `Game.Layers.AbilityAdding`, same layer
`AddKeyword` uses — protection isn't its own `KeywordSet` bit (it has to carry *which*
color/type/subtype/name/player, which a flat bitflag can't), so each quality is its own field,
`EffectiveCharacteristics.protection_colors`/`.protection_card_types`/`.protection_subtypes`/
`.protection_names`/`.protection_everything`/`.protection_players`, folded in alongside `keywords`
in the same layer-6 pass. Unlike the first two, `protection_subtypes`/`protection_names` are plain
`Vec`s, not a bitflag `Set` — both are open-ended strings (new creature types are added over time;
card names aren't enumerable at all), same reason `CardDefinition.subtypes` itself is a `Vec`.
`protection_everything` is a bare `bool` — CR 702.16j has no quality payload at all, it protects
"regardless of that object's characteristic values," so `AddProtectionEverything` (like
`RemoveAllAbilities`/`SwitchPT`) is a bare value rather than a function taking an argument.
`AddProtectionSubtype`/`AddProtectionName` each take one value per call (CR 702.16g's "protection
from A and from B" is already modeled as two separate registered static abilities, same convention
every quality uses). `AddProtectionName` matches against the source's *effective* name
(`EffectiveCharacteristics.name`, post any CR 612.8 `SetName`), not the printed
`CardDefinition.name`.

`AddProtectionPlayer(who)` is the odd one out: `who` is `"you"` or `"opponent"`, not a raw
`PlayerId` — this engine has no multiplayer arbitrary-player-targeting yet (backlog item 11), and a
card script's `initial_effect(c)` is never handed a `PlayerId` at registration time to name a
specific player with anyway. It reuses `Game.EffectFilters`'/`:SetAffected({controller=...})`'s
same relative vocabulary (`parse_filter_controller_name`), resolved at each of the three
enforcement sites against the *protected object's own* controller — `"you"` means protection from
that object's own controller (unusual but valid — e.g. an aura that grants an opponent's creature
protection from you), `"opponent"` means protection from anyone else, the common real case.
`"any"` is rejected with a real error — "protection from any player" isn't a CR 702.16k quality,
that's what `AddProtectionEverything` (CR 702.16j) already covers:

```lua
-- This creature has protection from red.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionColor("R"))
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

```lua
-- This creature has protection from artifacts.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionCardType("Artifact"))
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

```lua
-- This creature has protection from Zombies.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionSubtype("Zombie"))
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

```lua
-- This creature has protection from Progenitus.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionName("Progenitus"))
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

```lua
-- This creature has protection from everything.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionEverything)
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

```lua
-- This creature has protection from an opponent.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionPlayer("opponent"))
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

Enforced at three real call sites, none of them a `state.replacement_effects` entry (protection is
a static, always-on characteristic, not something a card "registers" — same "checked up front, not
a registered replacement" stance CR 702.12b indestructible already takes ahead of `Destroy`), each
checking all six quality fields as an OR (protected if the color, card-type, subtype, name,
"everything," *or* player quality matches the source — `protection_everything` short-circuits
before even looking up the source's own characteristics, since CR 702.16j needs none; the player
quality compares the source's raw `GameObject.controller` field, not a characteristic, resolved
against the protected object's own controller): damage (`replacement::apply_protection`, CR
702.16a — the matched source's damage is prevented entirely, re-checked fresh every
replacement-resolution pass so a mid-resolution `RedirectDamage` retarget is caught too), targeting
(`commands::target_is_legal`, CR 702.16c — a spell/ability matching a protected quality can't
target it, including the CR 608.2b recheck at resolution time if protection is gained after the
spell was already cast), and blocking (`DeclareBlockers`, CR 702.16d — a creature matching a
protected quality can't block it, same call site Flying/Reach/Menace already gate). Enchant/Equip
(702.16b) stays unmodeled — no attach/enchant machinery exists in this engine yet. That's the
complete CR 702.16 quality list otherwise — color, card type, subtype, name, everything, and player
are all real.

`:SetAffected` takes a plain table, not its own family of tag factories — every key optional,
same shape `EffectFilter` (`types/effect.rs`) already has:

```lua
e:SetAffected({
    controller = "you",           -- "you" | "opponent" | "any", optional (default: no filter)
    card_types = { "Creature" },  -- optional (default: no filter)
    self_only = true,             -- optional, default false - "just this effect's own source"
})
```

Omitting `:SetAffected()` entirely means no filter at all — the effect applies to every object
(a real, if unusual, shape). Calling `:Register()` with no `:SetLayer()`/no `:SetEffect()` is a
real error, same stance `TriggeredAbilityBuilder`'s `Register()` takes for a missing
`:SetOperation()`. A card can call `Game.CreateStaticAbility(c)` more than once per
`initial_effect` — each `:Register()` independently queues, same as multiple triggered abilities.

`:SetDuration(Game.Durations.*)` (CR 611.2a-style) makes the registered effect temporary instead
of permanent — e.g. "artifacts you control gain flying until end of turn":

```lua
e:SetDuration(Game.Durations.UntilEndOfTurn)
```

Same 4 tags `Game.Ops.Pump`/`Game.Ops.GainControl`'s own duration arguments already expose
(`UntilEndOfTurn`/`UntilYourNextTurn`/`UntilEndOfCombat`/`Permanent` — see `DurationTag`'s own doc
comment in script.rs for why the rest of `EffectDuration` aren't offered). Omitting
`:SetDuration()` defaults to `Permanent`, this builder's original (and still most common)
behavior. The drain itself is the same cleanup/end-of-combat/next-turn mechanism every other
`EffectDuration`-tagged `ContinuousEffect` already uses (`phase5/effect-duration-expiration`) —
this builder only gained a way to opt into it.

`Game.Ops.GainControl(duration)` (CR 108.4/613.1 layer 2) is a resolving spell/ability's own
primitive, same `return Game.Ops.X(...)` shape `Game.Ops.GainLife(n)` uses — e.g. "Gain control of
target creature until end of turn" (Threaten-style) as a spell's own operation:

```lua
return Game.Ops.GainControl(Game.Durations.UntilEndOfTurn)
```

Every legal target gets its own `ContinuousEffect` (layer 2, `EffectPrimitive::GainControl`),
pinned `self_only: true` to that target exactly like `Pump` pins its own `ModifyPT` effect — the
new controller is always the resolving spell/ability's own controller, no separate player argument
from Lua. Only this resolving-effect creation path exists — a static ability's own "you control
enchanted creature" (Control Magic-style aura) needs real attach/enchant machinery this engine
doesn't have yet, a separate, larger gap. `phase5_gain_control_effect.rs` proves the primitive
itself end to end (via direct `CardDefinition.effect` construction, same convention every other
`AbilityEffect` primitive's own test file follows — not by invoking the Lua binding, which just
decodes to the identical value), including reverting control once the duration expires.

**Script-backed `:SetAffected`/`:SetEffect`** — both also accept a custom function instead of a
table/tag, same two-shapes pattern `:SetCondition`/`:SetOperation` already use:

```lua
-- "Other creatures you control get +1/+1" via a script predicate instead of
-- the {controller=.., card_types=..} table shape.
e:SetAffected(function(object, source, controller)
    return object ~= source
end)

-- An effect whose shape is decided by a script instead of a Game.Effects.*
-- tag - the "Lua asks, Rust decides" table convention Game.Ops.* operations
-- already use, generalized to seven recognized keys:
e:SetEffect(function(source, controller)
    return { modify_pt = { power = 1, toughness = 1 } }
    -- also recognized: set_pt = {power=,toughness=}, add_keyword = {"Flying",...},
    -- add_card_type = {"Creature",...}, add_color = "WU", switch_pt = true,
    -- remove_all_abilities = true
end)
```

An affects-predicate is invoked once per candidate object (`ScriptRuntime::invoke_continuous_affects`)
and must return a plain boolean. An effect-shape script is invoked once per query
(`ScriptRuntime::invoke_continuous_effect`) and must return a table with exactly one of the seven
recognized keys above — an empty/unrecognized table, or a script that errors, resolves to "no
effect this call," same "skip rather than guess" stance every other unresolvable script value in
this engine already takes. Neither call passes `GameState` into Lua (same limitation
`invoke_cda`/`invoke_condition`/`invoke_operation` already carry) — a script can only decide from
`object`/`source`/`controller` identity or a constant, not real board state, until `Game.*`
state-query bindings exist (still open, see `docs/implementation.md`'s slice backlog).

`validate_layer_effect_pairing` can't check a script-backed effect's real shape at `:Register()`
time (it's only known once the script actually runs) — a mismatch between the declared layer and
the decoded shape (e.g. `SetLayer(PowerToughnessModifying)` whose script returns `add_keyword`) is
instead silently skipped at query time, not surfaced as an error. A real, narrower safety net than
the `Game.Effects.*` tag form gets — flagged, not fixed.

`timestamp` (CR 613.7 ordering) is stamped by `run_card_script`'s drain step, not the builder
itself — the builder has no `&mut GameState` to allocate one from inside a Lua callback.

Same registration timing and `retire_object` purge as triggered abilities above — no new call
sites needed; `run_card_script` drains both `pending_triggered` and `pending_static` in one pass.

Real, tested end to end: `crates/engine/tests/phase5_static_ability_self_registration.rs` (table/tag
form), `crates/engine/tests/phase5_continuous_effects_script.rs` (script-backed form),
`crates/engine/tests/phase5_static_ability_duration.rs` (`:SetDuration`),
`crates/engine/tests/phase5_lose_all_abilities.rs` (`RemoveAllAbilities`, both forms),
`crates/engine/tests/phase5_protection_from_color.rs` (`AddProtectionColor`, all three enforcement
sites), `crates/engine/tests/phase5_protection_from_card_type.rs`
(`AddProtectionCardType`, same three sites plus the `RemoveAllAbilities` interaction),
`crates/engine/tests/phase5_protection_from_subtype.rs` (`AddProtectionSubtype`, same three sites
plus `RemoveAllAbilities`), `crates/engine/tests/phase5_protection_from_name.rs`
(`AddProtectionName`, same three sites plus `RemoveAllAbilities`),
`crates/engine/tests/phase5_protection_from_everything.rs` (`AddProtectionEverything`, same three
sites plus `RemoveAllAbilities`), and `crates/engine/tests/phase5_protection_from_player.rs`
(`AddProtectionPlayer`, same three sites plus `RemoveAllAbilities` and the `"any"` rejection).
Scenario TOML fixtures can use the table/tag form via `CardSpec.script` too — see
`crates/engine/tests/scenarios/phase5_static_ability_self_registration.toml`.

## Self-registering a replacement effect

`Game.CreateReplacementEffect(c)` (CR 614/615) is a narrower sibling of `Game.CreateStaticAbility`
— it doesn't touch layers/`effective_characteristics` at all (a replacement effect rewrites an
*event*, a wholly separate mechanism, `GameState.replacement_effects`), so there's no
`:SetLayer`/`:SetEffect` pairing to pick; `:SetFilter`/`:SetPayload` serve the equivalent role.
Omitting every settable field reproduces this builder's original CR 701.19b-only shape — a
`Repeatable` regeneration shield ("...each time that permanent would be destroyed," not just the
next time — contrast `Game.Ops.Regenerate()`, a resolving spell/ability's own `OneShot` shield, CR
701.19a):

```lua
-- This creature regenerates.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

`:SetFilter(Game.EventFilters.*)` picks which event shape this effect watches —
`DamageDealt`/`WouldDie`/`WouldMoveZone(fromName, toName)` are the ones with a real query path
(`ReplacementEffect::matches`). `WouldDie` is hardcoded to Battlefield -> Graveyard (CR 700.4
"dies"); `WouldMoveZone` is the general form — either `fromName`/`toName` may be `nil` for "any
zone," e.g. `Game.EventFilters.WouldMoveZone("Battlefield", nil)` matches any move off the
battlefield, `Game.EventFilters.WouldMoveZone(nil, "Exile")` matches any move into exile regardless
of source. `:SetPayload(Game.ReplacementPayloads.*)` picks the rewrite: `PreventDamage` (CR 615,
pairs only with `DamageDealt`), `RedirectDamageToSource` (CR 614.9, pairs only with `DamageDealt` —
see below), `Regenerate` (CR 701.19b, pairs only with `WouldDie` — CR 614.8's "genuine destruction"
meaning is specifically tied to `WouldDie`'s own Battlefield -> Graveyard shape, not something a
wildcard `WouldMoveZone` filter should reinterpret), or `ReplaceZoneDestination(zoneName)` (CR
614.13-style "instead" substitution, pairs with `WouldDie` *or* `WouldMoveZone` — `zoneName` is a
zone name string, e.g. `"Exile"`). A mismatched pairing is a real `:Register()`-time error, same
stance `StaticAbilityBuilder`'s `:SetLayer`/`:SetEffect` pairing already takes. `:SetConsumption(Game.
Consumption.*)` picks `OneShot`, `Repeatable` (the default), or `LimitedUses(n)`.

```lua
-- If this creature would be exiled, put it into its owner's graveyard instead.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Exile"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Graveyard"))
    e:SetAffected({ self_only = true })
    e:Register()
end

return s
```

`WouldMoveZone`'s wildcard sides are real and correct but still unreachable for *some* zone shapes via
actual gameplay. `Sacrifice`/`Discard` cost payments are real now — `Engine::submit_payment` collects
them separately from the immediate payments (mana/tap/life) and drives each one through `Engine::
continue_cost_zone_moves`, the same propose/replacement-check/commit split `sba::resolve_deaths`
already uses (`phase5/replaceable-cost-zone-moves`) — so `Game.EventFilters.WouldMoveZone("Hand",
"Graveyard")` genuinely intercepts a Discard-as-cost now, a `from` shape `WouldDie` could never
express. CR 514.1's cleanup hand-size discard is real too now (`phase5/replaceable-cleanup-discard`)
— `Engine::submit_discard` drives its own queue through `Engine::continue_discard_moves`, the same
propose/replacement-check/commit loop `continue_cost_zone_moves` uses, just with a different tail
(routes into real trigger collection afterward instead of logging a `CostPaid` event).

CR 504.1's turn-based draw is real too, closing this reachability gap's last piece
(`phase5/replaceable-draw`) — `Game.EventFilters.WouldMoveZone("Library", "Hand")` now genuinely
intercepts it (a "you draw instead" Notion Thief-style effect, or the simpler "if you would draw a
card, exile it instead" shape below):

```lua
-- If you would draw a card, exile it instead.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Library", "Hand"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ controller = "you" })
    e:Register()
end

return s
```

The one remaining holdout is `Game.Ops.DrawCards(n)` (a scripted "draw N cards" ability) — it lives
inside `ResolveStackTop`'s own giant effect match, a different pause/resume shape than
`AdvanceTurnStep`'s (and needs a count-based queue, not a list of ids, since each draw re-peeks
whatever's now on top of the library), so it wasn't folded into the same slice. A scripted multi-draw
ability stays inert against a `WouldMoveZone` effect until that follow-up lands - the turn's own
regular draw (CR 504.1, the overwhelmingly common real-card case) is unaffected.

```lua
-- If a creature you control would be sacrificed, exile it instead.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ controller = "you", card_types = { "Creature" } })
    e:Register()
end

return s
```

This intercepts a `Sacrifice`/`Discard` cost payment, a cleanup-step discard, or (once a real producer
exists) a draw — the same registered effect matches all of them, since they all propose the identical
`ZoneChange` shape. `Game.EventFilters.WouldMoveZone("Hand", "Graveyard")` alone (no `"Battlefield"`
counterpart) catches only a discard, whichever way the card got discarded:

```lua
-- If a card would be discarded, exile it instead.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Hand", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ card_types = { "Creature" } })
    e:Register()
end

return s
```

```lua
-- Fog: prevent all combat damage that would be dealt to you this turn.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.PreventDamage)
    e:SetConsumption(Game.Consumption.OneShot)
    e:SetAffected({ controller = "you" })
    e:Register()
end

return s
```

`Game.ReplacementPayloads.RedirectDamageToSource` (CR 614.9) redirects the matched damage to this
effect's own registering permanent instead — the Pariah/Pariah's Shield shape ("damage that would
be dealt to you is dealt to [this permanent] instead"), the only redirect target that's a static
Lua literal (it's just `c`, already known when the builder was created):

```lua
-- Pariah: all damage that would be dealt to you is dealt to this creature instead.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
    e:SetAffected({ controller = "you" })
    e:Register()
end

return s
```

Redirecting to an arbitrary *chosen* target (e.g. "to that source's controller," or "to target
creature") stays unbound — that would need live target selection (`Game.SelectTargets`, which
doesn't obviously compose with a builder that's queued and drained later rather than resolved
synchronously) or a computed-from-the-event target neither `EventFilter` nor this builder currently
expose. Flagged, not guessed at — a card that needs one still has to be built directly in Rust.

CR 614.9's own destination-validity clause ("if [the destination] is no longer on the battlefield,
or is no longer a creature/planeswalker/battle... the effect does nothing," "if damage would be
redirected to or from a player who has left the game, the effect does nothing") is rechecked fresh
by `ReplacementEffect::matches` on every application, not just once at construction — same "recheck
fresh" stance CR 701.19c's `CANT_BE_REGENERATED` check already takes for `Regenerate`. A redirect
whose destination has become invalid simply doesn't match; it doesn't prevent the damage, so the
original, unredirected damage proceeds normally.

`:SetAffected` takes the same plain-table shape `Game.CreateStaticAbility`'s own `:SetAffected`
does (`controller`/`card_types`/`self_only`, every key optional) — but a custom function isn't
accepted here (unlike the static-ability builder): `EventFilter` has no `Script`-affected shape
yet, so passing one is a real `:SetAffected()` error rather than something silently discarded.

CR 701.19c ("can't be regenerated" effects) is checked fresh every time a `Regenerate` shield is
asked to apply (`ReplacementEffect::matches`), not just once when it was created — unlike the
one-shot `Game.Ops.Regenerate()` path (which also skips *creating* a shield for an already-can't-
be-regenerated target as a cheap optimization), a repeatable shield is registered once but the
target's CANT_BE_REGENERATED status can change afterward, and CR 701.19c's own wording ("cause
regeneration shields to not be applied") is about application, not creation.

Same registration timing and `retire_object` purge as static abilities above — `run_card_script`
also drains `pending_replacement` into `GameState.replacement_effects` in the same pass. Real,
tested end to end: `crates/engine/tests/phase5_static_regeneration.rs` (the original
`WouldDie`/`Regenerate`-only shape) and `crates/engine/tests/phase5_replacement_effect_shapes.rs`
(`:SetFilter`/`:SetPayload`/`:SetConsumption`, including a real combat-damage Fog effect and a
real exile-instead-of-dying effect, both driven through a full `Engine`).

## Self-registering an activated ability

`Game.CreateActivatedAbility(c)` is the CR 602 sibling of `Game.CreateTriggeredAbility`/
`Game.CreateStaticAbility` — same queued-registration mechanism (`ActivatedAbilityBuilder` next to
the other two builders in `script.rs`), producing a real `RegisteredAbility` with
`kind = AbilityKind::Activated`:

```lua
-- {T}: Gain 1 life.
local s = {}

function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.TapSelf)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end

return s
```

`:AddCost` is called once per cost component, not a single `:SetCost` — an activation can need
more than one at once (e.g. `{T}, Sacrifice a creature:`):

```lua
e:AddCost(Game.Costs.TapSelf)
e:AddCost(Game.Costs.Sacrifice({ card_types = { "Creature" } }))
```

`Game.Costs` mirrors `Game.Effects`'s one-factory-per-shape convention:

- `Game.Costs.TapSelf` — CR 602.5a's `{T}` on the ability's own source. A plain tagged value (no
  arguments), always taps whichever object is activating the ability — never a filter choice.
- `Game.Costs.Sacrifice(filter)` / `Game.Costs.Discard(filter)` — `filter` is a plain table, same
  shape `:SetAffected` already uses: `{ card_types = {...}, controller = "you"|"opponent"|"any",
  random = true }`, every key optional.
- `Game.Costs.TapPermanents(filter, count)` — taps `count` *other* permanents matching `filter`
  (distinct from `TapSelf`).
- `Game.Costs.Life(n)` — pay `n` life.

No `:SetCost` — every `:AddCost` call appends to the ability's `CostSpec.additional`. Omitting
`:AddCost` entirely means `cost: None` (no cost at all), not an empty `CostSpec`. Mana-cost
components on an activated ability's own cost stay out of scope — no Lua-facing mana-cost-string
parser exists anywhere yet (same boundary `CastSpell`'s own cost has).

`:SetTarget(table)` (`phase5/equip-ability`) gives a script-registered activated ability a real
target — reuses `decode_target_filter_table`, the same table-shape decoder `Game.SelectTargets`
already uses (`{type="object"|"player"|"any", card_types={...}, subtypes={...}, supertypes={...},
controller="you"|"opponent"|"any"}`, `type` defaults to `"object"`). Always single-target (`count:
1, up_to: false`) — no "up to N" or multi-target shape, same narrowing precedent `Game.SelectTargets`'s
own doc comment already set. Omitting `:SetTarget` entirely keeps the ability targetless (`target:
None`), the prior behavior. CR 702.6a's Equip is the worked example — "`[Cost]`: Attach this
permanent to target creature you control. Activate only as a sorcery":

```lua
-- Equip {1}
local s = {}

function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.Mana("{1}"))
    e:SetTarget({ type = "object", card_types = { "Creature" }, controller = "you" })
    e:SetOperation(Game.Ops.Attach)
    e:Register()
end

return s
```

`Game.Ops.Attach` is a bare tagged value (no arguments, same toggle shape `Game.Ops.Destroy`/
`Sacrifice`/`Exile`/`Regenerate`/`BecomeCopy` already use) — attaches the resolving ability's own
source to whatever `:SetTarget` resolved, via `commands::attach_permanent` (the same function
`phase5/aura-attach` built for a resolving Aura's own CR 303.4b entry-attach). CR 702.6a's "Activate
only as a sorcery" is enforced in Rust (`ActivateAbility`'s command handler matches on `effect ==
Some(AbilityEffect::Attach)` directly, not a new `RegisteredAbility` field or Lua-facing flag — see
that variant's own doc comment for why), so no `Game.Costs.*`/builder call is needed to opt into it;
every `Game.Ops.Attach` ability gets the restriction for free. CR 704.5n's own SBA cleanup (an
illegally-attached Equipment becomes unattached and stays on the battlefield, unlike an Aura which
dies under CR 704.5m) needs no Lua surface either — it's a pure state-based action, same as every
other SBA in this engine.

Activating one of these still pauses on `AwaitingPayment` the same way `CastSpell` does, even for a
bare `Game.Costs.TapSelf` cost — the actual tap (or sacrifice/discard/life deduction) happens once
`Engine::submit_payment` commits a real `PaymentPlan`, not when the ability is announced.

Real, tested end to end: `crates/engine/tests/phase5_activated_ability_self_registration.rs`,
`crates/engine/tests/phase5_activate_ability_costs.rs` (the latter covers hand-built, non-script
`CostSpec`s exercising every additional-cost shape), and `crates/engine/tests/phase5_equip_ability.rs`
(`:SetTarget`, `Game.Ops.Attach`, the sorcery-speed restriction, and CR 704.5n's SBA cleanup).

## Asking the player for a target mid-resolution

`Game.SelectTargets(filter)`, called from inside an operation function, suspends the script and
asks the player to choose a real target before continuing — the first real player-input-mid-script
mechanism (backlog item 9), built on Lua coroutines:

```lua
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        local target = Game.SelectTargets({ card_types = { "Creature" } })
        -- target is the chosen creature's object id (a plain integer),
        -- available here exactly as if the call had returned synchronously
        return { gain_life = 1 }
    end)
    e:Register()
end
```

Under the hood, `ScriptRuntime::invoke_operation` runs the operation function as a Lua coroutine
(`Lua::create_thread`/`Thread::resume`) instead of a plain call. `Game.SelectTargets` itself is a
plain Lua function (not a Rust-bound one) that calls `coroutine.yield` directly — mlua 0.9 without
the `async` feature (not enabled in this engine) has no way for a Rust closure to suspend the Lua
stack from inside itself, so the yield has to happen in real Lua code. The engine sees the yield as
`StepStatus::AwaitingScriptChoice { player, request }`; the caller prompts and answers via
`Engine::submit_script_choice(chosen)`, which resumes the exact suspended script with that
value and continues its execution from the `Game.SelectTargets(...)` call as if it had returned
synchronously. A script that never calls `Game.SelectTargets` behaves identically to before this
existed — nothing about existing operations changes.

`filter` is the same flat option-bag shape `:SetAffected`/cost filters already use, plus a `type`
discriminator: `{ type = "object"|"player"|"any", card_types = {...}, subtypes = {...}, supertypes =
{...}, controller = "you"|"opponent"|"any" }`, every key optional (`type` defaults to `"object"`).
`subtypes` (`phase5/targeting-subtype-filter`, CR 205.4b — "target Human," "Equip Dragons") is a
plain list of subtype name strings, matched as an "or" (no parser — `Subtype` is a bare string
newtype, any value is legal). `supertypes` (`phase5/targeting-supertype-filter`, CR 205.4a —
"target legendary creature") is a list of supertype names (`"Basic"`/`"Legendary"`/`"Snow"`/
`"World"`, same `parse_supertype_name` table `:SetAffected`'s own `supertypes` key already uses),
matched as an intersection against `SupertypeSet`. `target` resumes as a tagged table, `{ kind =
"object"|"player", id = n }` — a script
checks `target.kind` to tell an object answer from a player one. Exactly one target per call (no
"up to N," no zone override). An
illegal answer to `submit_script_choice` is rejected and the pause stays open, same "invalid
candidate, no state change" contract every other choice-submit method in this engine already has.

`invoke_target`/`TargetSpec::Script` (a *static* target-requirement predicate, checked once at
cast/activation time before the stack) is a different, unrelated concept and stays unimplemented.

Real, tested end to end: `crates/engine/tests/phase5_coroutine_spike.rs` (the raw coroutine
mechanism, standalone) and `crates/engine/tests/phase5_script_target_selection.rs` (the full
pause/resume path through a real `Engine`, including the player/any-target filter shapes).

## Asking the player a yes/no question mid-resolution

`Game.MayChoose()` (backlog item 9a, CR 608.2d) is `Game.SelectTargets`'s sibling for a plain
optional-action choice, with no filter to satisfy:

```lua
e:SetOperation(function(source, controller)
    if Game.MayChoose() then
        return { gain_life = 4 }
    end
    return { gain_life = 1 }
end)
```

Same coroutine mechanism, same literal-Lua-function-calling-`coroutine.yield` reason as
`Game.SelectTargets`. The engine sees the yield as `StepStatus::AwaitingScriptChoice { player,
request: ScriptChoiceRequest::MayChoose }`; the caller answers via `Engine::
submit_script_may_choice(bool)`, a separate entry point from `submit_script_choice` (one method
per pause shape, same precedent `submit_replacement_choice`/`submit_legend_rule_choice` already
set) — any `bool` is a legal answer, there's no `target_is_legal`-style rejection. Calling the
wrong submit method for the pending request (`submit_script_choice` on a `MayChoose` pause, or
`submit_script_may_choice` on a `SelectTarget` pause) is rejected with `ScriptChoiceError::
WrongRequestKind`, not silently misapplied.

Not yet possible: modal spells ("choose one"), where the mode is chosen *before* the stack as
part of casting (CR 700.2a) — a fundamentally different mechanism from this mid-resolution
coroutine ask, closer to `TargetRequirement`/`PaymentPlan`. Named, unattempted follow-up.

Real, tested end to end: `crates/engine/tests/phase5_script_may_abilities.rs`.

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

- **`Game.ReplaceEvent`, `Game.CastSpell`, `Game.PayCost`** (`script_api.rs`) — none of these are
  bound into the Lua VM's globals. A card script cannot call any of them.
  `Game.Conditions.*`/`Game.Ops.*`/`Game.Costs.*`/`Game.Layers.*`/`Game.Effects.*`/
  `Game.CreateTriggeredAbility`/`Game.CreateStaticAbility`/`Game.CreateActivatedAbility`/
  `Game.SelectTargets`/`Game.MayChoose` are what exist on `Game` right now (both are real as of the
  script-coroutines/script-may-abilities slices — see the two "Asking the player..." sections
  above; they're literal Lua functions, not `script_api.rs` bindings, which is why they're not
  grouped with the three unbound ones here). `Game.AddContinuousEffect` specifically is superseded by
  `Game.CreateStaticAbility`'s builder (same relationship the old `register_triggered_ability` free
  function has to `Game.CreateTriggeredAbility`) — it was never bound and isn't expected to be.
- **A mana ability via Lua** — still wired via `mana_ability` (`docs/scenario_format.md`), not
  `Game.CreateActivatedAbility` — CR 605 mana abilities resolve immediately and never read `.cost`
  at all, out of scope for that builder.
- **`ByKey` script resolution** — `run_card_script` only handles `ScriptReference::Inline`;
  `ByKey` is a real error. Real integration is expected to always hand this engine an
  already-resolved `Inline` source (the database crate resolves the key upstream), so this may
  never need solving here.
- **Script-backed costs/targets** — `ScriptRuntime::invoke_cost`/`invoke_target` are both
  `todo!()`. `Game.Costs.*` covers every structured `AdditionalCost` shape without needing either
  one. Neither builder binds a bare `:SetCost` (both use `:AddCost`, appending to `CostSpec.
  additional`, since a single activation can need more than one component). `TriggeredAbilityBuilder`
  still doesn't bind `:SetTarget` at all (`invoke_target`'s own gap, nothing built yet to consume
  one); `ActivatedAbilityBuilder`'s own `:SetTarget` (`phase5/equip-ability`) only accepts the
  structured table shape `decode_target_filter_table` already handles (`{type=..., card_types=...,
  controller=...}`) - a custom-Lua-function target predicate still isn't possible for either
  builder, only the recognized primitive shapes.
- **{X} costs, alternative costs (flashback-style), hybrid/Phyrexian/snow mana payment** —
  pre-existing `CostSpec`/`PaymentPlan` gaps, not specific to the script bridge.
- **Any event kind other than `ZoneChange`** — `event_to_lua` (`script.rs`) only bridges zone
  changes. A condition wanting to check damage/life-change/tap events will see `nil` and must
  wait on `event_to_lua` being extended.
- **Modal spells ("choose one")** — the mode is chosen *before* the stack as part of casting (CR
  700.2a), a different mechanism from `Game.SelectTargets`/`Game.MayChoose`'s mid-resolution
  coroutine ask — not built yet, named follow-up split off from backlog item 9a.

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
