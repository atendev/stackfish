# 00-overview

## Repo layout

- Workspace root `Cargo.toml`, member `crates/engine` (`stackfish-engine`).
- `crates/engine/src/types/` — strictly data: `ids`, `mana`, `card`, `zone`, `counter`, `object`, `cost`, `stack`, `effect`, `event`, `turn`, `command`, `player`, `game`. `types/mod.rs` flat-reexports all.
- Logic modules live at `crates/engine/src/` sibling to `types/`: `engine`, `commands`, `layers`, `replacement`, `sba`, `triggers`, `script`, `script_api`, `color_identity`.
- Internal imports inside `types/` use full `crate::types::*` paths; external code uses flat `crate::TypeName`.
- `bitflags` used for `CardTypeSet`, `SupertypeSet`, `KeywordSet` (later widened to `u32`), `ColorSet`.
- `crates/testutil` (`stackfish-testutil`) — shared `GameState`/`GameObject`/`PlayerState` fixture builders for `crates/engine/tests/phase5_*.rs`. See "Test fixtures" below.

## Test fixtures

`crates/engine/tests/phase5_*.rs` integration tests build fixtures via `stackfish_testutil` rather than hand-rolling `GameState`/`GameObject`/`PlayerState` struct literals per file — that duplication (~240 near-identical builder copies) was paid down in the `refactor/test-suite` branch (merged to master).

**When writing or migrating a phase5 test, use `stackfish_testutil`'s builders, overriding only what the test varies via struct-update syntax:**
- `card_object(id, definition, owner, zone) -> GameObject` (`crates/testutil/src/object.rs`)
- `player_state(id) -> PlayerState` — life 20, empty pools/counters (`player.rs`)
- `drain(engine) -> StepStatus` — loops `step()` until non-`Progressed` (`engine_ext.rs`)
- `two_player_state(zones) -> GameState` / `single_player_state(player, zones) -> GameState` — turn 1, `Phase::Main`, `next_object_id` 1000 (`state.rs`)
- `hand_and_battlefield_zones` / `hand_battlefield_graveyard_zones` / `battlefield_graveyard_exile_zones` / `zones(p0, p1, kinds)` — common zone-set presets (`state.rs`)

Example: `GameState { step: Step::PreCombatMain, ..two_player_state(hand_battlefield_graveyard_zones(p0, p1)) }`. Only build a fully custom `GameState`/`Zone` literal when a fixture's shape genuinely doesn't fit an existing preset — add a new preset to `testutil` first if the shape looks reusable, rather than hand-rolling in the test file again.

## Phase 1: data model

### Bitmasks
`CardTypeSet`, `SupertypeSet`, `ColorSet` are bitflags; paired with plain enums `CardType`, `Supertype`, `Color`. Bool-structs were rejected.

### Mana symbols
- `AtomicManaSymbol`: `Colored`, `Generic`, `Colorless`, `Snow`, `Phyrexian(Color)`.
- `ManaSymbol`: `Atom`, `Hybrid(Atomic, Atomic)`, `X`.
- Hybrid is a fixed 2-tuple; `{W/U/P}` is `Hybrid(Phyrexian(W), Colored(U))`, no 3-slot container.
- Plain Phyrexian is `Atom(Phyrexian(_))`.

### P/T / CDA
- `PTValue { Fixed(i32), CharacteristicDefining { display, script } }` on `CardDefinition`.
- `EffectiveCharacteristics` holds resolved live values; layer engine later computes them.

### Zones
- `ZoneKind` enum; `Zone { kind, owner: Option<PlayerId>, contents: VecDeque<ObjectId> }`.
- `GameObject.zone` is only `ZoneKind`, never a full `Zone`.
- No `zone_position` on object; ordering lives solely in `Zone.contents`.
- Stack is separate `GameState.stack: Vec<StackObject>`, not a generic zone.
- `GameState.objects: HashMap<ObjectId, GameObject>`.
- `GameEventKind::ZoneChange { object, from, to, last_known_info }` carries an LKI snapshot.

### Visibility
`VisibilityPolicy` kept as per-object override on top of zone defaults.

### Counters
`CounterKind` enum with rule-significant variants (`PlusOnePlusOne`, `MinusOneMinusOne`, `Loyalty`, `Poison`, `Energy`, `Experience`, `Charge`) plus `Custom(String)`.

### Emblems
`Emblem` struct directly on `PlayerState.emblems`, not a `GameObject`.

### Cost / payment
- `AdditionalCost` filter fields became structured `FilterSpec { card_types, controller, random, extra }`.
- `PaymentPlan` validates atomically against `CostSpec`; no partial payment commits.

### Commands vs. actions
`EngineCommand` mirrors `PlayerActionKind` 1:1; no lower command tier. `SubmitPayment` is its own action, not a sub-command.

### Effects / durations
- `ContinuousEffect` / `ReplacementEffect` renamed (no `*Instance`).
- `EffectDuration` added `UntilYourNextTurn`, `UntilEndOfCombat`, `WhileConditionHolds`, `Custom(String)`.

### Pre-game
Added `Phase::PreGame` (engine-only) for setup before first `Beginning`.

### Life / damage / Target
- `PlayerState.lose_life` / `gain_life` are CR 119 primitives.
- `DamageDealt.target` became `Target { Object(ObjectId), Player(PlayerId) }`.
- `DeclareAttackers.attackers: Vec<(ObjectId, Target)>`; `MakeChoice.selection: Vec<Target>`.

## Phase 2: engine loop + script bridge signatures

### Engine loop
- `engine::Engine` wraps `GameState` with `submit_action` / `step`.
- `StepResult.events_added` is `Range<usize>` into `state.ledger`.
- `commands::execute_command` mutates state and appends events.
- `layers::effective_characteristics` is the CR 613 query.
- `replacement::apply_replacements` noted needing CR 616.1 player choice (later fixed).
- `sba::check_state_based_actions` is one pass; engine loop repeats until empty.
- `triggers::PendingTrigger` kept transient, not in `GameState`.

### Script bridge
- Added `mlua` vendored dependency.
- `script.rs`: `ScriptRuntime`, `ScriptContext`, `ScriptError`.
- `script_api.rs`: `Game.*` surface.
- Added `types/ability.rs`: `AbilityKind`, `RegisteredAbility`.
- Added `ScriptHandle` opaque callback handle; `ContinuousEffect.affected_predicate` and `ReplacementEffect.event_pattern` typed as `ScriptHandle`.
- Restored `StackObject.operation: ScriptHandle`.
- Removed placeholder `StepResult` from `game.rs`; real one lives in `engine.rs`.
- Open gaps tracked: hidden information (`ScriptContext.state` full), player-input-mid-script synchronous signatures.

## Phase 3: TODO pass
- Every `todo!()` body got `// TODO:` comment referencing CR rule / struct / field.
- Gaps surfaced: `CounterKind` missing derives, `GameObject::new` successor-vs-new distinction, no combat state, `ReplacementEffect` had matcher but no rewrite, `register_triggered_ability` missing `AbilityKind`, `pay_cost` missing `CostSpec`, `ScriptRuntime::invoke_operation` mutability mismatch, `order_apnap` missing intra-controller ordering.

## Phase 4: verification setup

### Workspace additions
- `crates/scenario` (lib): TOML-driven `GameState` builder + runner.
- `crates/cli` (bin): `cargo run -p stackfish-cli -- <scenario.toml>`.
- `crates/engine/tests/scenarios.rs` + `tests/scenarios/*.toml`: integration tests.

### Design
- Scenario crate builds `GameState` via direct struct literals, not `todo!()` constructors, so baseline setup+assertions run green immediately.
- `CounterSet::new()` and `Engine::new()` / `Engine::state()` were implemented early to enable this.
- Running actions still hits `todo!()`; `stackfish_scenario::run_action` wraps each in `catch_unwind`, reports `StepOutcome::NotImplemented` without aborting.
- `cargo test --workspace` is the verification command; assertion failures only count as real failures.

### Scenario schema v1
- Cards use `generic_cost` + `colors` arrays; no mana-cost-string parser yet.
- `SubmitPayment` not supported yet at that point.

### Known open into Phase 2
- `StepResult` placeholder in `game.rs` had no fields.
- Script-bound callback representation unresolved.
- mlua hidden-info boundary unresolved.
