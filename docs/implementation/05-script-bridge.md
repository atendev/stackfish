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
- Scope: card's Lua `initial_effect(c)` calls `Game.CreateTriggeredAbility(c)` and self-registers.
- Verify: 1 unit test + 9 integration tests, 178 total, 0 failed.
- Key changes:
  - Mutability fix: `Rc<RefCell<ScriptRegistry>>` with `pending_triggered` queue; registration drained after script returns.
  - Builder pattern: `Game.CreateTriggeredAbility(c)` returns `TriggeredAbilityBuilder` with `:SetCondition`/`:SetOperation`/`:Register()`.
  - `ScriptRuntime::run_card_script(definition, source)` evaluates `initial_effect`, drains queue.
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

## General notes

- `ScriptRuntime` uses `mlua` vendored Lua 5.4.
- `ScriptHandle` opaque; registry stores `RegistryKey` for functions and suspended coroutine threads.
- Design principle: "Lua asks, Rust decides" — scripts return intent tables; Rust validates and applies.
