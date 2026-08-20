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

## Slice: script-error-surfacing-cluster (branch pending review)

- Branch `phase5/script-error-surfacing-cluster`.
- Scope: the sibling half `script-error-surfacing` above left untouched — `run_card_script` (9 direct
  call sites + the `run_card_script_for` wrapper's 7 more) and `invoke_operation`/`resume_operation`/
  `resume_operation_bool` (5 sites total), all previously `let _ = ...`/`.unwrap_or(...)`.
- Key changes:
  - `Engine::record_script_error` (`engine/mod.rs`) — shared helper every `run_card_script` call site
    now routes through; logs `GameEventKind::ScriptError` on `Err`.
  - `resolve_misc_or_script_effect`'s `invoke_operation` arm, `each_player.rs`'s two `invoke_operation`
    calls, `choice_answers.rs`'s two `resume_operation*` calls, and `resolve_pending_cast`'s discarded
    `CommandError` all log the same way inline.
- Real, discovered-mid-slice deviation from the naive "log every failure" design: `run_card_script`'s
  `ScriptReference::ByKey` case *always* errors by design (its own doc comment), and the scenario-TOML
  loader defaults an unset `script` field to `Inline("")`, which also fails to eval to a table — both
  are the ordinary "vanilla card, no real script" shape `stackfish_testutil::minimal_card_definition`
  defaults to, used throughout this engine's own test suite, and `payment.rs`'s `run_script` flag means
  `run_card_script` fires for *every* permanent that resolves onto the battlefield. Logging
  unconditionally would have flooded a wide swath of existing `ledger.len()`-asserting tests with a new
  false "failure." `record_script_error` deliberately skips logging unless `script_ref` is a non-empty
  `Inline` source. Confirmed with the user before proceeding (narrower scope chosen over "log
  everything, then fix however many existing tests break").
  - Same shape for `invoke_operation`: a vanilla permanent/instant/sorcery/hand-built `RegisteredAbility`
    with no script-declared operation carries `ScriptHandle(0)` (`cast.rs`'s own placeholder convention,
    already relied on by ~90 existing test call sites). `resolve_misc_or_script_effect`'s arm skips
    logging when `top.operation == ScriptHandle(0)`.
  - Tried and reverted: making that check fully sound by reserving `ScriptHandle(0)` as an actually-
    unallocatable sentinel (`ScriptRegistry::next_handle` seeded at 1 instead of the derived-`Default` 0)
    — the exact "a real script's first-ever handle could coincide with the placeholder" ambiguity
    `phase5_script_operation.rs`'s own `operation_function_decodes_a_custom_function_to_a_real_handle`
    test comment already named, not a new discovery. Broke a real, unrelated, already-passing test
    (`phase5_mass_choice_effects.rs`'s
    `for_each_player_asks_in_apnap_order_and_sacrifices_simultaneously`) — root-caused via direct
    instrumentation, not guessed: `Game.Ops.ForEachPlayer(fn)`'s own wrapped-function handle
    (`decode_operation_value`'s `ForEachPlayerTag::Function` arm) is a real, separately allocated
    `ScriptHandle`, but `cast.rs`'s own `(operation, effect) = match effect { Some(Script(handle)) =>
    (handle, None), other => (ScriptHandle(0), other) }` unpacking still discards it down to the same
    placeholder for any non-`Script` effect shape (`ForEachPlayer` included) — a real, pre-existing bug
    in that unpacking this engine was *already* silently depending on: every existing test exercising
    this shape happened to load its wrapped function as the very first handle in a fresh registry, so
    the placeholder and the real handle coincided by accident. Reserving 0 broke that accidental
    coincidence without fixing the actual discard bug — a separate, bigger gap (`AbilityEffect::
    ForEachPlayer` has nowhere to carry its own handle at all), named here rather than attempted.
    Reverted; the narrow pre-existing ambiguity stays open, unchanged from before this slice.
- Tests: `phase5_script_error_surfacing_resolution.rs` — one representative test per distinct code
  shape touched (not one per call site, since 8 of the 9 `run_card_script` sites share the exact same
  `record_script_error` call verbatim): a `PlayLand` with a real erroring script logs an error; the same
  with a `ByKey` land logs nothing; a vanilla `ByKey` creature cast to resolution logs nothing (the
  highest-blast-radius regression guard); a triggered ability's real, genuinely-loaded operation handle
  throwing logs an error and still resolves cleanly; a vanilla instant with no operation (`ScriptHandle(0)`)
  logs nothing; a script-requested cast at a nonexistent object logs an error (mirrors
  `phase5_script_cast_spell.rs`'s pre-existing `casting_a_nonexistent_object_...` test, adding this
  slice's own new assertion).
- Unresolved / out of scope, named not attempted:
  - No rollback of whatever a call already mutated before erroring — same non-atomicity
    `StepStatus::Error`'s own doc comment already admits for `execute_command` itself.
  - `commands::legality.rs`'s `invoke_target_legal`/`invoke_player_target_legal` still do
    `.unwrap_or(false)` with no `ScriptError` event at all — found incidentally, a separate,
    not-yet-attempted gap (worse than this cluster's own pre-fix state, since it doesn't even log).
  - `invoke_cost` (`script/runtime.rs`) is still a literal `todo!()` — zero call sites anywhere in the
    workspace, unrelated dead code.
  - No `cargo`/toolchain available in the environment this slice was implemented in — verify the new
    test file and the full suite before merging.

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

## Slice: trigger-target-lua-exposure (branch open)

- Branch `phase5/trigger-target-lua-exposure`.
- Scope: `Game.CreateTriggeredAbility(c)` gains a real `:SetTarget(table | function)` method —
  CR 603.3d's target-selection pause has been engine-complete since `phase5/trigger-target-
  selection`, but `TriggeredAbilityBuilder` had no field/method for it at all, so it was only
  reachable by constructing a `RegisteredAbility` directly in Rust, never from an actual card
  script. Scoped narrowly to `:SetTarget` only (not mode declaration) after checking with the user
  first — CR 603.3c's mode choice (`phase5/trigger-modal-choice`) has no existing Lua vocabulary to
  mirror at all (modal *spells* don't have a script-facing way to declare modes either), a bigger,
  more novel design task than mirroring an existing pattern.
- Key changes:
  - `TriggeredAbilityBuilder.target: Option<TargetFilter>` — new field, mirrors
    `ActivatedAbilityBuilder.target` exactly.
  - `TriggeredAbilityBuilder::SetTarget` — identical body to `ActivatedAbilityBuilder::SetTarget`
    (`decode_target_filter_value`, table or function shape).
  - `TriggeredAbilityBuilder::Register` wraps `this.target` into `TargetSpec::Primitive(TargetRequirement
    { filter, count: 1, up_to: false, at_least: false, exclude_source: false })`, same single-
    exact-count narrowing `ActivatedAbilityBuilder::Register` already uses — no CR 601.2c multi-word
    form from this builder.
- Verify: 4 new tests (`phase5_trigger_target_lua_exposure.rs`) — table-shape decode into a real
  `TargetRequirement`, omitting `:SetTarget` still registers `target: None` (regression), function-
  shape decode into `TargetFilter::Script`, and a full end-to-end (`PlayLand` self-registers a
  script-declared "destroy target creature" trigger, fires, pauses on a real
  `AwaitingTriggerTargets`, resolves against the chosen target). Full workspace suite green (164
  test binaries, 0 failed).
- Unresolved: CR 601.2c multi-word targets from this builder (same narrowing
  `ActivatedAbilityBuilder` already has). Mode declaration from Lua (named above, a separate,
  bigger gap). `:SetCost` stays unbound (triggered abilities have no activation cost concept).

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
