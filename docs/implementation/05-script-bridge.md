# 05-script-bridge

## Slice: script-bridge-trigger (merged)

- Branch `phase5/script-bridge-trigger`.
- Scope: first real triggered ability end-to-end (ETB gain 1 life), registration stays Rust-side.
- Verify: 7 new tests, 158 total, 0 failed.
- Key changes:
  - `ScriptRuntime` real: `HashMap<ScriptHandle, RegistryKey>`, monotonic handle counter.
  - `load_function(source)` loads Lua function, stores registry key, returns handle.
  - `invoke_condition` builds `(source, controller, event)` table; only `ZoneChange` bridged to Lua.
  - `invoke_operation` rewritten to take `&mut GameState`, decodes `gain_life` table key.
  - `PlayerState::gain_life` real body.
  - `triggers::collect_triggers` takes `&ScriptRuntime`, evaluates conditions.
  - `commands::find_ability` extracted.
  - `Engine::load_script` public wrapper.
  - `PutTriggersApnap` pushes real `TriggeredAbility` `StackObject`s; `ResolveStackTop` resolves them.
  - Scenario `CardSpec.trigger_condition`/`trigger_operation`.
- Design: "Lua asks, Rust decides" — operation returns intent table, Rust applies.
- Deviations:
  - `build_initial_state` returns 3-tuple `(GameState, card_ids, Vec<PendingTriggerScript>)`.
- Unresolved:
  - `Game.*` global table binding and card self-registration.
  - Hidden-information leak.
  - `invoke_target` synchronous signature.
  - Only `ZoneChange` bridged as event.
  - Only `gain_life` decoded.

## Slice: script-bridge-primitives (merged)

- Branch `phase5/script-bridge-primitives`.
- Scope: `Game.Conditions.*` / `Game.Ops.*` load-time primitive tags.
- Verify: 10 new tests, 168 total, 0 failed.
- Key changes:
  - `AbilityEffect::GainLife(i32)`.
  - `ConditionPrimitive { EntersBattlefield, Always }`, `ConditionSpec { Primitive, Script }`.
  - `ScriptRuntime::load_condition`/`load_operation` decode tags or functions.
  - `Game.Conditions = { Enters, Always }`, `Game.Ops = { GainLife }`.
  - `triggers::collect_triggers` branches on `ConditionSpec`.
  - `StackObject.effect` baked at push time.
- Deviations:
  - Borrow-checker workaround for `mlua::Value` lexical scope (inner block).
- Unresolved:
  - `Game.*` self-registration still out of scope.
  - Only `GainLife`, `Enters`, `Always` wired.

## Slice: script-self-registration (merged)

- Branch `phase5/script-self-registration`.
- Scope: card's Lua `register_abilities(c)` calls `Game.CreateTriggeredAbility(c)` and self-registers.
- Verify: 1 unit test + 9 integration tests, 178 total, 0 failed.
- Key changes:
  - Mutability fix: `Rc<RefCell<ScriptRegistry>>` with `pending_triggered` queue; registration drained after script returns.
  - Builder pattern: `Game.CreateTriggeredAbility(c)` returns `TriggeredAbilityBuilder` with `:SetCondition`/`:SetOperation`/`:Register()`.
  - `ScriptRuntime::run_card_script(definition, source)` evaluates `register_abilities`, drains queue.
  - `commands::retire_object` removes object + purges abilities/continuous effects; `remap_abilities` used at hand→battlefield/cast/resolve sites; `retire_object` used at discard/draw/SBA-death.
  - `CardSpec.script` sets `ScriptReference::Inline`.
  - `docs/lua_scripts.md` rewritten for builder API.
- Deviations:
  - `CommandOutcome::battlefield_entries` added so `engine.rs` can run scripts after `PlayLand`.
- Unresolved:
  - `CreateActivatedAbility`/`CreateStaticAbility` not built yet.
  - `ByKey` resolution stays error.
  - `run_card_script` re-parses source every call.

## Slice: continuous-effects-script (merged)

- Branch `phase5/continuous-effects-script`.
- Scope: `AffectedFilter::Script` / `EffectPayload::Script` real evaluation; builder sugar `:SetAffected`/`:SetEffect`.
- Verify: 12 new tests, 422 total, 0 failed.
- Key changes:
  - `applicable_effects_in_layer` gains `&ScriptRuntime`, evaluates `AffectedFilter::Script`.
  - New `invoke_continuous_affects(handle, object, source, controller)` and `invoke_continuous_effect(handle, source, controller)`.
  - Decodes 6 effect keys: `set_pt`, `modify_pt`, `add_keyword`, `add_card_type`, `add_color`, `switch_pt`.
  - `StaticAbilityBuilder` fields changed to `Option<EffectPayload>` / `AffectedFilter`.
  - Builder supports function-form `:SetAffected` / `:SetEffect`.
- Deviations:
  - `StaticAbilityBuilder.effect`/`affected` type changes.
- Unresolved:
  - No `Game.*` state-query bindings for scripts (later fixed by `script-state-query`).

## Slice: script-state-query (merged)

- Branch `phase5/script-state-query`.
- Scope: hidden-information-safe state snapshot passed to scripts.
- Verify: 7 new tests, 553 total, 0 failed.
- Key changes:
  - `state_to_lua` now exposes: per-player `life`, `hand_count`, `library_count`; public zone summaries for `battlefield`, `graveyard`, `exile`, `command`.
  - Hand/library contents never exposed; only counts.
  - `invoke_cda`, `invoke_continuous_affects`, `invoke_continuous_effect` all receive `&GameState` and pass snapshot.
  - `card_summary_to_lua` exposes base P/T netted with counters, is_creature, controller — deliberately not layer-resolved to avoid recursion.
- Deviations:
  - `invoke_cda`/etc. gained `state: &GameState` params.
- Unresolved:
  - Card summaries base only, not effective characteristics.
  - Stack not included.
  - Face-down permanents not special-cased.

## Slice: script-error-surfacing (merged)

- Branch `phase5/script-error-surfacing`.
- Scope: log Lua runtime errors instead of silently treating as no-match.
- Verify: 4 new tests, 0 failed.
- Key changes:
  - `GameEventKind::ScriptError { message }`.
  - `triggers::collect_triggers` returns `(Vec<PendingTrigger>, Vec<String>)`; caller logs errors as events.
  - `ResolveStackTop` intervening-if recheck logs script errors.
- Exception (deliberately unfixed):
  - `layers.rs` pure `&GameState` query functions cannot log without threading error reporting through whole call graph.

## Slice: register-abilities-and-script-operation (merged)

- Branch `phase5/register-abilities-and-script-operation`.
- Scope: two related pieces.
  1. Rename `initial_effect` → `register_abilities` across the whole convention (Lua module key,
     the one real Rust lookup + its error message, every doc/test/TOML occurrence except
     `docs/ygo_pro_reference.md`, which describes ygo-pro's own convention and is left alone).
     Prompted by the user: the name was a direct ygo-pro carryover, where a card keeps one
     continuous identity for its whole time in a duel, so "initial effect" meant "runs once, sets
     everything up." This engine's CR 400.7 adaptation changed the semantics (re-runs fresh every
     battlefield entry) while keeping the old name. Considered `on_enters_battlefield(c)` first;
     the user preferred `register_abilities` since the function registers *any* kind of ability
     with *any* condition, not just enters-the-battlefield ones — the old candidate conflated when
     it runs with what it's for.
  2. A new sibling function, `s.operation()` — CR 608.2m, a plain spell's own resolution effect
     (Armageddon, Shock). A plain instant/sorcery has no ability wrapper and never becomes a
     battlefield object, so `register_abilities(c)` can't help it (same reasoning
     `phase5/script-target-requirement` already established for a spell's own base targeting).
- Key finding that shaped the design: `CardDefinition` has no `Default` impl and ~400 existing
  struct-literal construction sites — adding a sibling `operation: ScriptHandle` field (the shape
  `RegisteredAbility` already has, `effect` + `operation` side by side) was avoided for that reason
  (a previous slice, `modal-multi-mode`, flagged that exact ripple class as prohibitive). Instead:
  a new `AbilityEffect::Script(ScriptHandle)` variant packs the custom-function-fallback handle
  into the *existing* `CardDefinition.effect: Option<AbilityEffect>` field. `commands::
  execute_command`'s `CastSpell` arm unpacks it at push time into `StackObject`'s real two-field
  shape (`Script(handle)` → `operation: handle, effect: None`) — the exact shape `ResolveStackTop`
  already treats `effect: None` as "invoke `operation` for real," the same dual-path convention
  triggered/activated abilities already use. The only ripple: two pre-existing matches that
  enumerate every `AbilityEffect` variant by name with no wildcard (`ResolveStackTop`'s dispatch,
  `ActivateAbility`'s dispatch) each needed one "exhaustiveness only, can't actually reach here"
  arm — same pattern already used there for `AbilityEffect::Modal`.
- Verify: 7 new tests (`phase5_script_operation.rs` — decode with/without the function, primitive-
  tag decode, custom-function decode, a script-declared no-target sorcery resolving for real via
  the primitive fast path, one resolving via the script bridge, a targeted spell combining
  `target_requirement()`+`operation()`, an illegal-target rejection) plus a new scenario TOML
  fixture proving the scenario-crate wiring, full workspace suite green.
- Caller trap found while testing, now documented on `ScriptRuntime::build_operation`'s own doc
  comment: a custom-function decode's `ScriptHandle` is only valid against the specific
  `ScriptRuntime` that decoded it. Building a `CardDefinition` with a throwaway runtime and handing
  it to a freshly-constructed `Engine` (which builds its own, separate runtime) leaves the handle
  dangling — the custom function silently never runs, since `ResolveStackTop`'s own `Err(_) => {}`
  arm swallows the resulting "unknown ScriptHandle" error (a pre-existing, separately tracked gap).
  A `Game.Ops.*` primitive tag has no such dependency (decodes to a plain Rust value). The real
  pattern (and what `register_script_operation` does): decode through the `Engine`'s own runtime
  *after* construction, not before.
- Key changes:
  - `types/ability.rs`: `AbilityEffect::Script(ScriptHandle)`.
  - `script.rs`: `ScriptRuntime::build_operation(source) -> Result<Option<(Option<AbilityEffect>,
    ScriptHandle)>, ScriptError>` — reuses `decode_operation_value` unchanged, no new Lua-facing
    vocabulary.
  - `engine.rs`: `Engine::build_operation` delegate; one exhaustiveness-only match arm in
    `ResolveStackTop`.
  - `commands.rs`: `CastSpell`'s `StackObject::new` push site unpacks `AbilityEffect::Script`;
    one exhaustiveness-only match arm in `ActivateAbility`.
  - Scenario crate: `PendingScriptOperation`/`register_script_operation`, same placeholder-then-
    patch timing fix `register_script_target_requirement` already established — every card with a
    `script` set gets a chance to declare one (no TOML competitor field to gate against, unlike
    `target_requirement` — `CardSpec` has never had a TOML field for a card's own effect at all).
- Unresolved: the mass-destroy (`DestroyAll`-with-filter) `AbilityEffect` primitive Armageddon
  itself still needs is out of scope here, by design — a separate next slice.

## General notes

- `ScriptRuntime` uses `mlua` vendored Lua 5.4.
- `ScriptHandle` opaque; registry stores `RegistryKey` for functions and suspended coroutine threads.
- Design principle: "Lua asks, Rust decides" — scripts return intent tables; Rust validates and applies.

## Adding a new `Game.Ops.*` primitive

To add e.g. `Game.Ops.DrawCards(n)`:

1. Add a variant to `AbilityEffect` (`types/ability.rs`), e.g. `DrawCards(i32)`.
2. Register a factory function on the `Game.Ops` table in `script.rs`'s `bind_game_globals`,
   mirroring `GainLife`'s `lua.create_function(|_, amount: i32| Ok(OperationTag(...)))`.
3. Add the matching apply arm in `ScriptRuntime::invoke_operation` (the custom-function/table-key
   path) *and* wherever the primitive's `Some(AbilityEffect::...)` fast path is dispatched
   (`engine.rs`'s `ResolveStackTop`) — both paths need to produce the same observable effect,
   since either one might be what a given ability ends up using.

Adding a new `Game.Conditions.*` primitive follows the same shape: a new `ConditionPrimitive`
variant, a Lua table entry in `bind_game_globals`, and the matching arm in
`ConditionPrimitive::matches`.

The card-author-facing reference for the resulting API lives in [docs/lua/](../lua/index.md), not
here — update the relevant page there once the primitive is real and tested.
