# Implementation Notes

> Working reference for whoever (human or agent) picks this back up. Captures decisions made
> and *why*, mistakes caught and corrected, and things deliberately deferred — not a restatement
> of what the code already says. Update this as later phases land; don't let it go stale.

## Repo layout

- Cargo workspace at repo root (`Cargo.toml`, `members = ["crates/engine"]`).
- `crates/engine` — the core engine lib crate (`stackfish-engine`).
- `crates/engine/src/types/` — strictly data types, no behavior: `ids`, `mana`, `card`, `zone`,
  `counter`, `object`, `cost`, `stack`, `effect`, `event`, `turn`, `command`, `player`, `game`.
  `types/mod.rs` declares all of them and flat-re-exports everything (`pub use card::*` etc).
  `lib.rs` just does `pub mod types; pub use types::*;`. Logic (the step-able rules processor,
  script bridge, etc., once those get built in later phases) goes in sibling top-level modules
  under `crates/engine/src/`, *not* into `types/` — kept the data/behavior split visible at the
  folder level from the start, per the human's request, rather than letting it implicitly
  fall out of one flat module list.
- Internal imports inside `types/` use full `crate::types::modname::Item` paths (not relative
  `super::`) — explicit about where things live now that there's a nesting level. Files outside
  `types/` can still use the flat `crate::TypeName` form via the root re-export.
- `bitflags` is the one external dependency so far (used for `CardTypeSet`, `SupertypeSet`,
  `ColorSet`).

## Phase 1 status

Structs only, no function bodies, per `docs/workflow.md` Phase 1 rules. Builds clean
(`cargo build`, zero warnings) as of this writing. Went through two review rounds with the
human; first round caught real modeling gaps (see below), second round caught a wrong technical
claim I made (see "Mistakes caught" at the bottom — don't repeat it).

## Decisions and rationale, by area

### Bitmasks vs bool-structs

`CardTypeSet`, `SupertypeSet`, `ColorSet` are `bitflags`-backed bitmasks (`u16`/`u8`/`u8`), each
paired with a plain enum (`CardType`, `Supertype`, `Color`) for matching/comparison. First draft
had these as structs of `bool` fields mislabeled "bitflags" — not an actual bitmask, just looked
like one. Caught by the human across two separate instances (`CardTypeSet`/`Supertype` first,
then `ColorSet` for the same reason) — if a new flag-set type gets added later, default to the
bitmask + enum pair from the start, not bools.

### Mana symbols (`mana.rs`)

`AtomicManaSymbol` (the indivisible unit: `Colored`, `Generic`, `Colorless`, `Snow`,
`Phyrexian(Color)`) is separate from `ManaSymbol` (`Atom`, `Hybrid(Atomic, Atomic)`, `X`).

`Hybrid` is a fixed 2-tuple, not a `Vec`. Real-world shapes it needs to cover: color/color
(`{W/U}`), generic/color (`{2/W}`), and hybrid Phyrexian (`{W/U/P}`-style symbols, CR 107.4f).
The hybrid Phyrexian case does **not** need a third slot: `Phyrexian(Color)` already bundles two
payment options (that color, or 2 life) into a single atom, so pairing it with a second atom in
a 2-slot `Hybrid` — e.g. `Hybrid(Phyrexian(Blue), Colored(Green))` — yields three total ways to
pay using only two slots. I initially got this wrong (see "Mistakes caught" below); the fix was
realizing the atom itself can be inherently multi-way, not the container.

Plain Phyrexian (`{W/P}` on its own, no second color) is `Atom(Phyrexian(_))`, not a hybrid at
all — hybrid implies two *symbols* in the printed glyph, not two *payment options*.

### Power/toughness/loyalty/defense (`card.rs`)

`PTValue { Fixed(i32), CharacteristicDefining(String) }` replaces plain `Option<i32>` on
`CardDefinition`. Reason: some cards have non-fixed base P/T (`*`, `*+1`, etc. — characteristic-
defining abilities, CR 208). The numeric resolution doesn't need separate representation: the CDA
script callback returns the final resolved number regardless of how the oracle text phrases it.
The `String` (added by the human, not in my first draft) keeps the literal text (`"*"`, `"*+1"`)
for display/debugging — `oracle_text` on `CardDefinition` has the full rules text but not the
isolated P/T expression on its own.

Live in-game values are a different concern entirely — `EffectiveCharacteristics` (`effect.rs`)
holds plain resolved `i32`s, because by the time anything queries live characteristics, the
layer engine has already run any CDA and arrived at a concrete number. `CardDefinition` holds
the base/printed value (possibly dynamic); `EffectiveCharacteristics` holds the live value
(always concrete). Two different structs on purpose — collapsing them would mean storing a
"current value" as a mutable field somewhere, which breaks the layer engine's timestamp/
dependency reordering when multiple effects stack (CR 613).

### Zones (`zone.rs`, `object.rs`, `game.rs`)

Two-tier split: `ZoneKind` (enum: `Library`, `Hand`, `Battlefield`, `Graveyard`, `Stack`,
`Exile`, `Command`) vs `Zone` (struct: `kind: ZoneKind`, `owner: Option<PlayerId>` — `None` for
shared zones, `contents: VecDeque<ObjectId>`).

`GameObject.zone` is `ZoneKind` only, never a full `Zone`. Storing a whole `Zone` on every object
in it would embed that zone's entire sibling-id list onto each object — the exact "two places
that can drift out of sync" problem the human flagged for `zone_position` (see next item),
just bigger. The actual `Zone` instances (with `contents`) live solely in `GameState.zones:
Vec<Zone>` — one flat list, not a per-player struct, because each `Zone`'s own `owner` field
already disambiguates whose zone it is.

`GameObject` originally also had a `zone_position: u32` field for in-zone ordering. Removed:
ordering must live in exactly one place (the zone's `contents` order), not duplicated as a
second number on the object that has to be kept in sync by hand. `contents` is a `VecDeque`
specifically so the future zone-manipulation code (insert at top/bottom of library, etc.) has
that finer control without it needing to be a plain `Vec` shuffled by index.

The Stack is the deliberate exception: it's not one of the `Zone`s in `GameState.zones`. Stack
entries need `StackObject`'s metadata (chosen modes/targets/X/costs paid), not just an ordered
id list, so stack ordering lives in `GameState.stack: Vec<StackObject>` instead. Confirmed
explicitly with the human — don't try to fold the stack into the generic `Zone` model.

Caught (while doing the `types/` folder move, unrelated to the move itself) that
`GameEventKind::ZoneChange` in `event.rs` had `from`/`to` typed as the full `Zone` struct, not
`ZoneKind` — the same "don't embed the whole zone's contents into something smaller" mistake as
the `GameObject.zone` field had, just not caught the first time around since `event.rs` wasn't
under review then. Fixed to `ZoneKind`. Worth deliberately re-scanning for this same pattern if
any other field in the codebase still names a bare `Zone` where only the kind is meant.

`GameState.objects` is a `HashMap<ObjectId, GameObject>` (lookup-by-id store), not a flat `Vec`.
Needed `ObjectId` to derive `Hash`/`Eq` for this — added those derives (plus `Clone`/`Copy`) to
all the id newtypes in `ids.rs` for consistency, even though not all of them need it yet.

### Visibility (`object.rs`)

Kept `VisibilityPolicy` on `GameObject` despite the zone already implying a default visibility
(hidden zones vs. public zones). Zone is the right *default*, but there are real per-object
exceptions a zone-only model can't express: a face-down permanent on the (public) battlefield
whose owner secretly knows its identity, or a card revealed to one player by a scry/surveil-
style effect while still sitting in a (hidden) library. `VisibilityPolicy` is an override layer
on top of the zone default, not the primary mechanism — confirmed with the human, this isn't
duplicate state in the same way `zone_position` was, because it's expressing genuinely different
information (exceptions), not redundantly restating the zone's default.

### Counters (`counter.rs`, `object.rs`, `player.rs`)

Counters were originally `Vec<(String, i32)>` on `GameObject` — no type safety, and CR 122.3
lists 100+ named counter kinds (mostly flavor, no rules significance). Added a shared
`CounterKind` enum with variants for the ones that actually matter to rules logic
(`PlusOnePlusOne`, `MinusOneMinusOne`, `Loyalty`, `Poison`, `Energy`, `Experience`, `Charge`) plus
`Custom(String)` for everything else. Shared between `GameObject.counters` (object-level, e.g.
+1/+1 counters) and the new `PlayerState.counters` (player-level, e.g. poison/energy/experience —
these were missing entirely from the first draft of `PlayerState`). `GameEventKind::CounterAdded`
updated to use `CounterKind` too, for the same reason.

### Emblems (`player.rs`)

Modeled as a standalone `Emblem { emblem_id, source, ability_text }` struct directly on
`PlayerState.emblems`, not as a `GameObject`. Reason: emblems live in the command zone per CR
114.1, but they have no `CardDefinition` — no mana cost, no type line, no database entry — so
forcing them through the `GameObject` model (which is built around having a `CardDefinition`)
would mean a fake/empty definition just to satisfy the type. Simpler to keep them as their own
small type. Revisit if the zone model ever needs to enumerate "everything in the command zone"
uniformly (would currently miss emblems).

### Cost/payment (`cost.rs`)

`AdditionalCost` filter fields (`Sacrifice`, `Discard`, `TapPermanents`) were raw `String`
descriptions. Replaced with a structured `FilterSpec { card_types: Option<CardTypeSet>,
controller: Option<FilterController>, random: bool, extra: Option<String> }` — covers the common
shapes (type + controller + count + random-selection) structurally, with `extra` as a script-
bind-time-refined catch-all for filters that don't reduce to that (power/toughness thresholds,
named-card restrictions, etc).

`PaymentPlan` flow (the human asked for this to be explained, documented in `cost.rs` directly):
a `CostSpec` describes what's owed but not how it'll be paid. The host collects the player's
choices (which lands to tap, which creature to sacrifice, whether to take an alternative cost)
via `PlayerActionKind::SubmitPayment` and assembles a `PaymentPlan`. The engine validates the
whole plan atomically against the `CostSpec` and only commits it (taps lands, sacrifices,
deducts mana) if everything checks out — no partial payment ever lands in game state.

### Commands vs. actions (`command.rs`)

`EngineCommand` mirrors `PlayerActionKind` 1:1 — there's no lower command tier ("pay cost", "push
onto stack", etc.) underneath it. The human asked for this to be explained explicitly since it
wasn't obvious; reasoning, documented in the file:

A `Command` represents one validated *player decision*. The internal sub-steps of carrying it
out (move card to stack zone, deduct mana, create the `StackObject`) aren't further decisions —
they're `GameEvent` facts the engine produces while executing the one decision it was given. When
casting genuinely needs *another* decision mid-flight (e.g. "sacrifice a creature" as an
additional cost — which one?), that's not a lower-level command either: it's the next
`PlayerAction` in the stream, at the same tier as `CastSpell`, with the engine just pausing
between `step()` calls expecting `SubmitPayment` next. This is also why `SubmitPayment` is its
own `PlayerActionKind` variant rather than folded into `CastSpell`.

### Effect naming and durations (`effect.rs`)

Renamed `ContinuousEffectInstance` -> `ContinuousEffect` and `ReplacementEffectInstance` ->
`ReplacementEffect` per the human's preference — avoid `*Instance` suffixes in type names
generally going forward.

`EffectDuration` was missing real cases: added `UntilYourNextTurn`, `UntilEndOfCombat`, and
`WhileConditionHolds` (for "as long as you control a Forest"-style durations that are
re-evaluated continuously rather than ending at a fixed point), plus a `Custom(String)`
catch-all for unusual oracle wording.

### Pre-game (`turn.rs`)

Added `Phase::PreGame` — not a CR-defined phase, but the engine needs somewhere to sit during
setup (shuffling, mulligans, commander/partner reveal, opening hand) before the turn loop's
first `Beginning` phase. The host steps the engine out of it explicitly once setup resolves.

### Life vs. damage, and the `Target` gap (`ids.rs`, `command.rs`, `stack.rs`, `event.rs`)

CR 119 (life) and CR 120 (damage) are related but distinct: damage prevention
shields stop damage, not generic life loss; "whenever you lose life" and
"whenever a source deals damage to you" are separate trigger conditions that
can both fire off one damage event; damage to a permanent marks damage
(`GameObject.damage_marked`), while damage to a player converts straight to
life loss with no marked-damage state of its own. `PlayerState::lose_life`/
`gain_life` are the literal CR 119 primitives — used directly for life-loss/
gain effects, and also as the downstream effect when a (not-yet-written)
damage-resolution step in the engine loop decides a player takes a hit.
Damage itself stays `GameEventKind::DamageDealt`, not something `PlayerState`
knows about.

That question surfaced a real gap while checking it: `DamageDealt`'s
`target` field was typed `ObjectId`, which can't represent damage *to a
player* — players aren't `GameObject`s in this model. The same gap existed
in `command.rs`'s `targets: Vec<ObjectId>` (`CastSpell`/`ActivateAbility`)
and in `StackObject.targets` — none of them could target a player, despite
plenty of real cards targeting "any target" (player, creature, planeswalker,
battle). Fixed with a shared `Target { Object(ObjectId), Player(PlayerId) }`
in `ids.rs`, swapped in everywhere a target slot existed. Also caught the
same shape of bug in `DeclareAttackers`: it had `attackers: Vec<ObjectId>`
and a separate `defending_players: Vec<PlayerId>`, which (a) couldn't
represent attacking a planeswalker/battle (CR 506.4) and (b) didn't even
pair each attacker with *which* defender it was attacking — two parallel
lists don't encode that pairing. Changed to
`attackers: Vec<(ObjectId, Target)>`. `MakeChoice.selection` got the same
treatment (`Vec<Target>`) since "choose a player" is a real choice shape.

`Target` does *not* cover battles/planeswalkers as a separate case — those
are still `Object(ObjectId)` since they're `GameObject`s like any other
permanent. `Player` only exists because a player is the one targetable/
damageable thing in the game that isn't a `GameObject` at all.

## Phase 2, part 2: core engine loop signatures

New top-level modules (logic, sibling to `types/`, per the data/behavior split from the folder
restructure): `engine.rs`, `commands.rs`, `layers.rs`, `replacement.rs`, `sba.rs`, `triggers.rs`.

- `engine::Engine` wraps `GameState` with `submit_action`/`step`. `StepResult.events_added` is a
  `Range<usize>` into `Engine::state().ledger` rather than cloned-out events — the ledger already
  has them, this just tells the caller which slice is new.
- `commands::execute_command` carries out one already-accepted `EngineCommand`, mutating
  `GameState` and appending to its ledger — no return value beyond `Result<(), CommandError>`,
  since the ledger append *is* the observable result.
- `layers::effective_characteristics` is the CR 613 layer-engine query that
  `EffectiveCharacteristics` (in `effect.rs`) was waiting on since Phase 1. Will need a
  script-runtime handle threaded through eventually (to resolve `PTValue::CharacteristicDefining`
  via the card's script callback) — not added yet since the script bridge itself isn't designed.
- `replacement::apply_replacements` has a known open gap: CR 616.1 has the *affected player*
  choose ordering when multiple replacements could apply to the same event. A pure
  `&GameState -> GameEventKind` function can't resolve a player decision — this will likely need
  to return a choice request in the ambiguous case instead of always a resolved event. Signature
  is a first pass, documented as likely to change.
- `sba::check_state_based_actions` is one pass, not the CR 704.3 fixed-point loop — `engine.rs`'s
  loop is responsible for calling it repeatedly until it returns empty, not the function itself,
  so a replay viewer stepping one ledger entry at a time can observe each pass separately.
- `triggers::PendingTrigger` is deliberately not part of `GameState` — it's transient process
  state between "a trigger matched" and "it's been placed on the stack as a `StackObject`," not
  part of the authoritative game model. `collect_triggers` finds matches; `order_apnap` is a
  separate step since ordering is independent of matching.

## Phase 2, part 3: script bridge (mlua)

Added `mlua = { version = "0.9", features = ["lua54", "vendored"] }` to `crates/engine/Cargo.toml`
(vendored so it doesn't need a system Lua install). New files: `script.rs` (`ScriptRuntime`,
`ScriptContext`, `ScriptError`) and `script_api.rs` (the `Game.*` surface named in DESIGN.md —
`register_triggered_ability`, `add_continuous_effect`, `replace_event`, `cast_spell`, `pay_cost`,
`select_targets`). Both are logic, sibling to `types/`, same as `engine.rs`/`commands.rs`/etc.

Doing this surfaced a real Phase 1 gap: there was no struct for a card's *registered* ability
(the thing `Game.RegisterTriggeredAbility` actually creates — condition/cost/target/operation
script handles, per ygopro's effect lifecycle). `ContinuousEffect`/`ReplacementEffect` existed;
the more basic "ability" type didn't. Added `types/ability.rs`: `AbilityKind` (Triggered/
Activated/Static) + `RegisteredAbility`, stored flat on `GameState.abilities` (same pattern as
`continuous_effects`/`replacement_effects` — scan-friendly, not nested under `GameObject`).

Also added `ScriptHandle` (`types/ids.rs`) — an opaque newtype handle into a loaded Lua callback,
deliberately *not* an `mlua::Function` held directly, so data types (`ContinuousEffect`,
`ReplacementEffect`, `RegisteredAbility`) can say "which script" without themselves depending on
`mlua` or holding VM-tied state. Used it to finally give concrete types to two Phase 1 String
placeholders that were explicitly marked "decided in Phase 2": `ContinuousEffect.affected_predicate`
and `ReplacementEffect.event_pattern` are now `ScriptHandle`, not `String`.

Also restored `StackObject.operation: ScriptHandle` — the original adaptation doc's `StackObject`
spec named a "resolution callback reference" that got dropped in Phase 1 (nothing to type it as,
yet). Without it, nothing on the stack would know what to actually run when it resolves.

Removed a leftover duplicate: Phase 1's placeholder `StepResult` marker struct in `game.rs` (data
module) is gone now that the real one lives in `engine.rs` (logic module) with actual fields.

Two deliberately-unsolved design problems, surfaced rather than papered over with a wrong-shaped
signature:

- **Hidden information.** `ScriptContext.state` is the full `&GameState` for now — exactly the
  leak DESIGN.md's open question warns about (a script could read an opponent's hand off it).
  Kept full-state only to get the call shape down; needs a visibility-filtered view before this
  is real.
- **Player-input-mid-script.** `ScriptRuntime::invoke_target` and `script_api::select_targets`
  both need to ask the player and get an answer back, but a player's answer arrives as a
  `PlayerAction` on a *later* `Engine::step()` call — it can't come back synchronously from within
  one Rust function call the way these signatures currently claim (`-> Vec<Target>`). Real fix is
  almost certainly Lua coroutines (`Lua::create_thread`, yield from the Lua callback, resume once
  the engine has the player's answer) spanning multiple `step()` calls. Signatures are written
  synchronously anyway for this first pass — they're very likely to change shape once the
  coroutine plumbing is designed, flagged in both places rather than treated as settled.

## Phase 3: TODO annotation

Every `todo!()` body across `types/` and the logic modules got a specific `// TODO:` comment
above it describing what needs implementing, referencing the relevant struct/field/CR rule.
`todo!()` itself stays as the body so the crate still compiles and panics if a stub is ever hit.

Writing those TODOs surfaced several more real gaps (same pattern as Phase 1/2 — writing the next
layer of detail keeps finding holes in the layer below). Noting each since they'll block Phase 7:

- `CounterKind` has no `PartialEq` (or any derive) — `CounterSet::count`/`add` can't compare kinds
  without it. Trivial fix, just not done yet (Phase 3 is comments-only).
- `GameObject::new` doesn't distinguish "brand new object" from "successor of a zone change" —
  the latter needs a populated `ObjectLineage`, the former needs `None`. One constructor covers
  neither cleanly; likely needs two, or a `lineage` parameter.
- `commands::execute_command`'s `DeclareAttackers`/`DeclareBlockers` handling has nowhere to put
  combat state — `GameState` has no combat-state field at all yet.
- `ReplacementEffect` can match an event (`event_pattern`) but has no field for what it actually
  rewrites the event into — only the matcher is modeled, not the effect. `replace_event` (script
  API) also can't construct one anyway since it has no `ConsumptionPolicy` parameter.
- `register_triggered_ability`'s signature has no `AbilityKind` parameter despite needing one to
  construct a `RegisteredAbility` — can't tell triggered/activated/static apart from the call.
- `script_api::pay_cost` has no `CostSpec` parameter to validate the `PaymentPlan` against, despite
  `PaymentPlan::validate` requiring one.
- `ScriptRuntime::invoke_operation` takes `&mut ScriptContext`, but `ScriptContext.state` is `&
  GameState` (shared, not mutable) — an operation callback needs to call back into `Game.*`
  functions that take `&mut GameState`. The mutability story isn't sorted, not just the Lua call
  mechanics.
- `triggers::order_apnap` can order *between* controllers but not *within* one controller's own
  set of simultaneous triggers — CR 603.3b leaves that order up to the controller, a decision this
  pure function can't make alone. Same shape of problem as the replacement-effect-ordering gap and
  the player-input-mid-script gap already tracked — there's a recurring pattern of "a pure
  `&GameState -> X` function hits a point where it actually needs a player decision."

## Phase 4: Verification Setup

Three new workspace crates/dirs:

- `crates/scenario` (lib, `stackfish-scenario`) — loads a declarative TOML scenario (players, ad-
  hoc test cards, initial zone placement, an action sequence, assertions), builds a real
  `GameState`, drives it through `Engine`, reports what happened.
- `crates/cli` (bin, `stackfish-cli`) — `cargo run -p stackfish-cli -- scenario.toml`. File-driven
  only, no live REPL yet — covers both a human (edit the file, rerun) and an AI agent (run via
  shell, read stdout) without needing real interactivity. Revisit if live typed-command
  exploration turns out to be wanted on top of this.
- `crates/engine/tests/scenarios.rs` + `crates/engine/tests/scenarios/*.toml` — real `cargo test`
  integration tests, one test that runs every scenario file in the directory.

**Key design point**: the scenario crate does *not* call the engine's still-`todo!()`
constructors (`GameObject::new`, `PlayerState::new`, `Zone::new`, etc) to build initial state —
it builds `GameState`/`GameObject`/`PlayerState`/`Zone` via direct struct literals, since their
fields are `pub`. That decouples the verification tool from which inherent-method bodies are
implemented yet, and means setup-plus-assertions scenarios (no actions) already run for real
today with zero panics — a genuine, currently-green baseline exercising the Phase 1 data model,
not a placeholder.

Two structs blocked even struct-literal construction because they have a private field
(`CounterSet.counters`, `Engine.state`) — implemented those two constructors for real, ahead of
phase order, since nothing else could exist without them:

- `CounterSet::new()` (counter.rs)
- `Engine::new()` / `Engine::state()` (engine.rs)

Running *actions* through `Engine::submit_action`/`step()` still hits `todo!()` until Phase 5/7 —
expected, and handled deliberately: `stackfish_scenario::run_action` wraps each action call in
`std::panic::catch_unwind`, so one stubbed action reports `StepOutcome::NotImplemented` instead of
aborting the whole scenario run. A scenario's assertions still run afterward and are checked for
real; `ScenarioReport::has_hard_failures()` only counts assertions that actually ran and got the
wrong answer, not unimplemented actions — that's the distinction that lets `cargo test` stay green
today while still being a real, tightening-over-time check as Phase 5/7 fill bodies in.

**Verification method (per workflow.md Phase 4):**

- COMMAND: `cargo test --workspace` (automated; also `cargo run -p stackfish-cli -- <scenario.toml>`
  for one-off manual/AI-driven runs against a specific scenario).
- EXPECTED: `cargo test --workspace` passes (0 failed). Individual scenario actions may report
  "not yet implemented" — that's expected pre-Phase 7, not a failure.
- FAILURE: any assertion in any scenario file producing a wrong answer, or a compile error.
- Confirmed passing on current codebase (baseline) — see `tests/scenarios/basic_setup.toml`
  (setup+assertions only, exercises the data model, currently green) and
  `tests/scenarios/pass_priority.toml` (exercises the action path, demonstrates the
  not-implemented-is-not-a-failure reporting).

**Scenario TOML schema (v1, intentionally simplified):**

Cards specify `generic_cost: u32` + `colors: [String]` rather than a parsed `"{2}{R}"`-style cost
string — no mana-cost-string parser exists anywhere, this sidesteps needing one just for fixtures.
`SubmitPayment` actions aren't supported yet — `PaymentPlan`'s `CostPayment` shapes are more
involved and the cost system itself isn't implemented either. Both are easy to extend later.

## Phase 5: Dry Run #1 — PassPriority slice

Branch `phase5/pass-priority`, off master. Slice: a player calls `PassPriority` with an empty
stack; the engine should accept it, log it, hand priority to the next player in turn order, and
once everyone in a row has passed, reset the pass count and advance the step. Implemented,
verified via `crates/engine/tests/scenarios/phase5_pass_priority.toml` + `cargo run -p
stackfish-cli`, then discarded (commit `acb3dae`, not merged — branch deleted after this report
was written). Master never carried the implementation.

**VERIFICATION RESULT:** Pass. Both `PassPriority` actions in the verification scenario reported
`ran` (not `NotImplemented`), and the `ledger_len == 2` assertion held. Confirmed via both the CLI
(`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_pass_priority.toml`) and
`cargo test --workspace` before reverting.

**STRUCT DEVIATIONS:**
- `GameState` needed two fields Phase 1 didn't have: `priority_player: PlayerId` and
  `consecutive_passes: u32`. `turn_order` + `active_player` alone don't carry "who currently has
  priority" or "how many players in a row have passed" — both are needed to detect CR 117.4's
  "everyone passed, move on" condition.
- `Engine` (Phase 2 interface) needed a field beyond its only documented one (`state`):
  `pending_action: Option<PlayerAction>`, so `step()` has somewhere to find what `submit_action`
  queued. The split between the two methods doesn't work without it.
- `Phase` and `Step` (turn.rs) had zero derives in Phase 1. Step-advancement logic needs to
  compare current phase/step, so both needed `Clone, Copy, PartialEq, Eq`.
- `CommandError` needed `Debug` to satisfy `.expect()` — incidental, not a modeling gap.

**INTERFACE DEVIATIONS:** None beyond the `Engine` field above — `submit_action`/`step`/
`execute_command`'s signatures as drafted in Phase 2 didn't need to change shape, only gain
internal state to actually do their job.

**SCOPE DEVIATIONS:**
- `crates/scenario`'s `build_initial_state` had to be updated to populate the two new `GameState`
  fields — not an engine file, but necessary for the verification tooling to keep compiling. Not
  covered by any Phase 3 TODO since it didn't exist when Phase 3 ran.
- Step advancement itself only handles one hardcoded transition (Beginning phase, Untap ->
  Upkeep) via a private helper in `engine.rs`, not a general mechanism — confirms the Phase 3
  prediction that `Step`/`Phase` can't answer "what's next" about themselves; the engine loop
  needs full phase-aware logic, which is real, undone work, not a quick TODO.
- Implementing `PassPriority` for real changed the *observable behavior* of pre-existing scenario
  fixtures that assumed nothing was implemented yet: `schema_check.toml`'s `ledger_len == 0`
  assertion broke, because its first action (`PassPriority`) now genuinely runs and appends an
  event. Not a bug — confirms the implementation was real — but worth knowing: fixtures written
  against an all-`todo!()` baseline can become stale as soon as any one path gets implemented.
  Those fixtures didn't need fixing here since the branch was discarded, but Phase 7 will need to
  go back and tighten assertions like that one as each path stops being a stub.

**UNRESOLVED:** Nothing blocked the slice itself. The combat-state gap, the
ReplacementEffect-has-no-rewrite-callback gap, the script-bridge gaps, etc (all flagged in Phase
3) remain exactly as documented — this slice didn't touch any of them.

**Takeaway for Phase 6/7:** the `GameState`/`Engine` struct additions above are real, not
speculative — they should go into the actual implementation directly rather than being
rediscovered slice by slice. Worth doing a small "fold in confirmed Phase 5 deviations" pass on
`GameState`/`Engine` before running more dry-run slices, so each new slice isn't re-deriving the
same priority-tracking fields.

## Post-dry-run interface update: explicit `ProcessState`

Not a dry run — a real, committed interface change to `engine.rs`, prompted by the human
proposing an explicit pipeline-stage enum (COLLECT_ACTIONS/PUSH_STACK_OBJECT/PASS_LOOP/
RESOLVE_STACK_TOP/REPLACEMENT_CHECK/COMMIT_EVENTS/SBA_LOOP/COLLECT_TRIGGERS/PUT_TRIGGERS_APNAP)
instead of the implicit control flow `step()`'s TODO comment described in prose. It mapped
cleanly onto modules already split out (`replacement.rs`/`sba.rs`/`triggers.rs`/`commands.rs`
basically *are* those stages) — what was missing was naming them as an explicit state `Engine`
sits in, which is also exactly the fine-grained per-stage `step()` granularity DESIGN.md wanted
for replay scrubbing.

Added `ProcessState` enum to `engine.rs`: `PriorityWindow`, `PendingAction(PlayerAction)`,
`ResolveStackTop`, `ReplacementCheck { proposed }`, `CommitEvents { checked }`, `SbaLoop`,
`CollectTriggers { since }`, `PutTriggersApnap { triggers }`. `Engine` now holds
`process_state: ProcessState` instead of the dry run's ad-hoc `pending_action` field — this
subsumes it. `COLLECT_ACTIONS`/`PRIORITY_WINDOW` merged into one `PriorityWindow` variant since
there's no `Engine::legal_actions()` yet to make a real "collect" stage out of.

Also folded in for real (not as a dry-run throwaway) the two `GameState` fields the PassPriority
dry run found necessary: `priority_player`, `consecutive_passes`. `crates/scenario`'s
`build_initial_state` updated to match. `CommandError` gained `Debug` (needed for `.expect()`).

**What's real vs. still stubbed:** the *transitions* between stages are real code now
(`PendingAction` -> build `EngineCommand` -> `execute_command` -> `SbaLoop`; `ReplacementCheck` ->
`apply_replacements` per event -> `CommitEvents`; `SbaLoop` -> `check_state_based_actions`,
re-entering itself until empty -> `CollectTriggers`). The *work* each stage delegates to
(`commands::execute_command`, `replacement::apply_replacements`, `sba::check_state_based_actions`,
`triggers::collect_triggers`/`order_apnap`) is still exactly as stubbed as Phase 3 left it —
running anything through `Engine` still bottoms out at the same `todo!()`s as before, just
reached through real pipeline plumbing instead of nonexistent plumbing. `ResolveStackTop` and
`PutTriggersApnap`'s bodies are themselves still `todo!()` (script invocation, APNAP ordering +
stack push respectively).

**Known incompleteness, flagged rather than silently wrong:**
- `PendingAction` always transitions to `SbaLoop` unconditionally. The original PASS_LOOP
  branching ("did everyone just pass with a non-empty stack? -> `ResolveStackTop` instead") isn't
  wired in yet — every action currently behaves as if the stack were empty.
- `SbaLoop`'s transition to `CollectTriggers { since: 0 }` hardcodes `since: 0` instead of the
  ledger position from before the current action/resolution started — needs threading through
  from `PendingAction`/`ResolveStackTop`, not invented at the `SbaLoop` boundary.
- `PutTriggersApnap`'s eventual return to `PriorityWindow` needs to reset priority to
  `active_player` (CR 117.3b — priority after any stack action goes to the active player, not
  whoever held it last), not implemented yet since that transition itself is still `todo!()`.

## `ProcessState` correctness pass (human review of the above)

The human reviewed the `ProcessState` wiring and found 7 real issues, all fixed:

1. **`since: 0` was a correctness bug, not just a TODO.** Added `engine_context.rs`:
   `PipelineContext { since_ledger, caused_by_command, parent_event }`, set when a pipeline run
   begins (`PendingAction`; `ResolveStackTop` still needs to set it too, noted in its TODO),
   consumed by `CommitEvents` (causal refs) and `SbaLoop`'s transition to `CollectTriggers` (real
   `since` instead of a hardcoded 0).
2. **No more panicking on command failure.** `StepStatus::Error(CommandError)` replaces the
   `.expect()`. Documented choice: on error, `Engine` resets to `PriorityWindow` rather than
   attempting rollback — `execute_command` doesn't guarantee atomicity, so there's nothing
   trustworthy to roll back to yet. Real fix is making `execute_command` atomic, not engine.rs's
   job.
3. **Pass-with-empty-stack vs pass-with-non-empty-stack.** `PendingAction`'s handling now checks
   `consecutive_passes >= turn_order.len()` after `execute_command` runs, branching to
   `ResolveStackTop` (non-empty stack) or `AdvanceTurnStep` (empty) via a new `PipelineExit` enum
   (`engine_context.rs`) consumed once trigger placement finishes. Documented the contract this
   relies on directly on `commands::execute_command`: every variant *other* than `PassPriority`
   must reset `consecutive_passes` to 0 (CR 117.3c) — `engine.rs` can't tell which variant ran, it
   can only read the aftermath.
4. **Causality preserved.** `CommitEvents` now fills `caused_by_command`/`parent_event` from
   `pipeline_context` instead of hardcoding `None`.
5. **`apply_replacements` takes `&mut GameState`.** A replacement can consume a one-shot shield or
   mark a `ConsumptionPolicy::LimitedUses` effect as used — that's a mutation, not a read.
6. **Added `ProcessState::AdvanceTurnStep`.** Explicit stage for turn/phase/step advancement,
   still `todo!()` (needs the general CR 500-514 version, not the dry run's hardcoded
   Untap->Upkeep), but now has a real place to live instead of being implied.
7. **`CommandId`/`EventId` no longer derived from `ledger.len()`.** `Engine` got
   `next_command_id: u64` + `allocate_command_id()`; `GameState` got `next_event_id: u64` +
   `allocate_event_id()`. Both dedicated monotonic counters, independent of ledger contents.

Side effects of the fix: `StepOutcome` (scenario crate) gained an `Errored(String)` variant
alongside `Ran`/`NotImplemented`, and `run_action` got rewritten — it was silently swallowing
`ActionError`/`StepStatus::Error` as `Ran` before (`catch_unwind`'s `Ok(_)` arm matched
`Ok(Err(_))` too, since `catch_unwind` only distinguishes panic-vs-no-panic, not success-vs-
returned-error). `ActionError` and `CommandError` both needed `Debug` derives along the way.

## Open items carried into Phase 2

- `StepResult` (`game.rs`) is a placeholder marker type with no fields yet — its real shape (what
  a caller can observe/act on after one `step()` call) is Phase 2 work.
- Concrete representation of script-bound callbacks (condition/cost/target/operation, in the
  ygopro-core sense) isn't decided yet — `ContinuousEffect`/`ReplacementEffect`'s `String`
  description fields are placeholders for whatever that representation turns out to be.
- mlua <-> Rust hidden-information boundary still unresolved (also tracked in `DESIGN.md`).

## Fix: double-writing ledger events

`commands::execute_command`'s doc previously said it appends to `state.ledger` directly, while
`engine.rs` separately had `ReplacementCheck -> CommitEvents` also appending ledger entries —
two write paths into the same ledger, only one of which (`CommitEvents`) actually ran
replacement/prevention checks (CR 614-616) first. Picked one model: `execute_command` now
returns `Result<Vec<GameEventKind>, CommandError>` (proposed events) and mutates only
non-ledger `GameState` fields (zones, `consecutive_passes`, `priority_player`, etc.);
`engine.rs`'s `PendingAction` branch feeds that `Vec` into `ProcessState::ReplacementCheck`
instead of going straight to `SbaLoop`. `engine.rs` is now the only place that allocates
`EventId`/`SequenceNo` and pushes onto `state.ledger`. Updated `commands.rs`'s doc comments to
match (no more "appends to ledger" claim).

## Fix: pass-chain logic location

`execute_command` previously had an implicit CONTRACT comment: every variant except
`PassPriority` had to remember to reset `state.consecutive_passes` to 0, and `engine.rs` inferred
"did everyone just pass" purely by reading that counter afterward — brittle, since a future
variant could forget the reset with no compile-time signal. Replaced with `commands::
CommandOutcome { events: Vec<GameEventKind>, broke_pass_chain: bool }`: `execute_command` returns
this explicitly (`broke_pass_chain = false` only for `PassPriority`) instead of touching
`consecutive_passes` itself. `engine.rs`'s `PendingAction` branch now owns
`state.consecutive_passes` entirely — increments it on `false`, resets on `true` — so there's one
place that both mutates and reads the counter.

## Fix: pipeline_context lifecycle (TODO note, not yet implemented)

`pipeline_context` gets set at the start of a pipeline run (`PendingAction`/`ResolveStackTop`) but
`PutTriggersApnap` — the run's last stage — is still `todo!()`, so nothing clears it back to
`None` yet. Left as-is would leak stale `since_ledger`/`caused_by_command`/`parent_event` into the
next run's `CommitEvents`/`CollectTriggers`. Documented as a MUST on both the `pipeline_context`
field doc and inside `PutTriggersApnap`'s TODO so whoever implements that stage doesn't skip it.

## Fix: ReplacementCheck needs a real await-choice state (CR 616.1)

`apply_replacements` was `&GameState -> GameEventKind` (well, `&mut GameState ->
GameEventKind`), and `engine.rs`'s `ReplacementCheck` did a blind `proposed.into_iter().map(...).
collect()`. That has no way to represent CR 616.1: when more than one `ReplacementEffect`
matches the same event, the *affected player* — not the engine — picks which applies first.
A pure transform can't make that call, and a `.map()` can't pause partway through a batch.

Changed `replacement::apply_replacements` to return `ReplacementOutcome::{Resolved(GameEventKind),
NeedsOrderingChoice { affected_player, event, candidates: Vec<ObjectId> }}` (candidates identified
by `ReplacementEffect::source`, since there's no dedicated id type). `engine.rs`'s
`ReplacementCheck` now processes `remaining` one event at a time instead of mapping the whole
`Vec` at once, so it can stop cleanly on `NeedsOrderingChoice` and move into a new
`ProcessState::AwaitingReplacementChoice { affected_player, pending_event, candidates, remaining,
checked }` — carrying the rest of that `ReplacementCheck` run's progress across the pause.
Added `StepStatus::AwaitingReplacementChoice` (mirrors `AwaitingAction`, but for this) and a
separate `Engine::submit_replacement_choice` entry point — deliberately not folded into
`submit_action`, since this isn't a `PlayerAction`/priority decision and `submit_action` only
accepts those while sitting in `PriorityWindow`.

Both `apply_replacements`'s replacement-resolution body and `submit_replacement_choice`'s
apply-the-chosen-rewrite body are still `todo!()` — blocked on `ReplacementEffect` not having an
"apply" callback yet (only `event_pattern`, the matcher, is modeled). This fix is the structural
piece: the *shape* of the pause/resume now exists so that gap can be filled in without another
signature change.

## Fix: trigger-order pause, explicit priority handoff, centralized pipeline-end

Three related structural gaps, fixed together:

1. **`ProcessState::AwaitTriggerOrderChoice { player, group, ... }`** (CR 603.3b). Same shape of
   gap as the CR 616.1 replacement-ordering fix: `triggers::order_apnap` couldn't decide a
   controller's intra-group trigger order itself — that's a player choice. Renamed it to
   `group_apnap` (now returns `Vec<(PlayerId, Vec<PendingTrigger>)>`, grouped by controller in
   APNAP order, intra-group order left untouched) and reworked `ProcessState::PutTriggersApnap`
   to walk one group per `step()` instead of ordering+pushing everything in one shot, pausing
   into `AwaitTriggerOrderChoice` whenever a group has more than one trigger. Added
   `StepStatus::AwaitingTriggerOrderChoice` and `Engine::submit_trigger_order` mirroring the
   replacement-choice pair. `PendingTrigger` gained `Clone, Copy` so it's cheap to clone for the
   status struct, same as `ReplacementOutcome::candidates` does for `ObjectId`.
2. **`commands::PriorityHandoff`**. `execute_command` used to rotate `state.priority_player`
   directly inside its `PassPriority` handling. Replaced with `CommandOutcome::priority_handoff:
   PriorityHandoff` (`Unchanged` or `To(PlayerId)`) — `execute_command` resolves *who* priority
   should go to but doesn't write the field; `engine.rs`'s `PendingAction` branch applies it.
   Same reasoning as the earlier `broke_pass_chain` fix: one place writes the field, one place
   decides the value, and they're not silently coupled through a mutation buried in a match arm.
3. **`Engine::end_pipeline_run`**. The "consume `pipeline_exit`, clear `pipeline_context`, pick
   the next `ProcessState`" transition was previously sketched only as a TODO comment inside
   `PutTriggersApnap`. Pulled out into its own method so there's exactly one place that performs
   it — `PutTriggersApnap`'s now-empty-`pending_groups` branch calls it (the actual `StackObject`
   push for `ordered` is still a TODO there, blocked on the same script-bridge gap as everywhere
   else, but the pipeline-end transition itself is no longer ad hoc).

## Phase 5: Dry Run #2 — pass-priority-loop slice (merged to master)

Branch `phase5/pass-priority-loop`, off master. Per the human's explicit instruction this time,
the branch was **not** discarded after verification (contrast Dry Run #1, which was deleted) —
kept around for review, then merged to master (`--no-ff`) once approved, including a follow-up
verification pass (ordering/pass-chain/pipeline-context/trigger-permutation checks, second commit
on the branch) before the merge. Unlike Dry Run #1, master now actually carries this
implementation — treat everything below as real, current behavior, not a discarded experiment.
Slice scope: finish the minimal loop back to `PriorityWindow` that the `ProcessState` correctness
pass (above) left structurally wired but leaf-`todo!()`'d — `PassPriority` execution, all-pass
detection's two branches (`AdvanceTurnStep`/`ResolveStackTop`), and the empty-trigger no-op path
through `CollectTriggers`/`PutTriggersApnap`.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 7 new tests in
`crates/engine/tests/phase5_pass_priority_loop.rs` plus the existing `scenarios.rs` harness (now
covering the updated `pass_priority.toml`/`schema_check.toml` fixtures), all green. Also confirmed
via `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/pass_priority.toml`.

**What got implemented (real, not stubbed further than necessary):**
- `commands::execute_command`: `PlayerActionKind::PassPriority` arm only — appends
  `PriorityPassed`, resolves the next player in `turn_order` via `PriorityHandoff::To`,
  `broke_pass_chain: false`. Every other `PlayerActionKind` variant is still `todo!()`, untouched.
- `engine::Engine::advance()`'s `ResolveStackTop`: pops `state.stack`, sets priority to
  `active_player` (CR 117.3b), feeds a mock `GameEventKind::StackObjectResolved { stack_id }`
  through the existing `ReplacementCheck` pipeline. Real resolution (invoking
  `StackObject::operation` via the script bridge) is still a TODO — this only proves the pipeline
  plumbing around a stack-object resolution is wired correctly.
- `engine::Engine::advance()`'s `CollectTriggers`: now actually scans `state.ledger[since..]` and
  calls `triggers::collect_triggers`/`group_apnap` for real, instead of `todo!()`.
- `engine::Engine::advance()`'s `AdvanceTurnStep` + new private `advance_turn_step()`: a
  deterministic CR 500-514 phase/step transition table (Untap→Upkeep→Draw→Main→Combat's five
  steps→Main→End→Cleanup→next turn's Untap), resetting priority to `active_player` afterward. Does
  **not** perform any of the actual rules work a step triggers (untapping permanents, drawing a
  card) — answers "what step comes next," nothing else.
- `triggers::collect_triggers`: returns `Vec::new()` when `state.abilities` is empty (true no-op,
  not a hidden gap) — still `todo!()` the moment any ability actually exists, since real condition
  evaluation needs the script bridge.
- `triggers::group_apnap`: fully real now (no script dependency needed) — rotates `turn_order` to
  start at `active_player`, partitions by controller, returns empty for empty input.
- `replacement::apply_replacements` / `sba::check_state_based_actions`: both given the same
  "no-op when genuinely nothing is registered to check" treatment — `Resolved(proposed)` unchanged
  when `state.replacement_effects` is empty; `Vec::new()` always for SBA (no SBA *can* apply with
  no creatures/damage tracking machinery existing yet — not a shortcut around real logic, there's
  nothing real to detect). Both still `todo!()`/no-op respectively beyond that, by design — full
  CR 614-616 and CR 704.5 implementations are out of this slice's scope.

**STRUCT DEVIATIONS:**
- `GameState` needed `postcombat_main: bool` — `Phase` alone can't tell CR 505.1's precombat Main
  from postcombat Main apart (flagged as a known gap in `turn.rs`'s own doc comment since Phase 1).
  Reset to `false` on every Beginning/Untap transition (new turn or game start).
- `Phase`/`Step` (turn.rs) had zero derives still on master (the Dry Run #1 branch that added
  `Clone, Copy, PartialEq, Eq` was discarded, never merged) — re-added those plus `Debug`, needed
  for `advance_turn_step`'s `match (phase, step)` and test assertions.
- `PlayerId`/`ObjectId`/`EventId`/`SequenceNo`/`CommandId`/`ScriptHandle`/`Target` (ids.rs) all
  gained `Debug` — needed for `assert_eq!`/panic-message formatting in the new integration tests;
  harmless, every other derive on these was already present.
- `PendingTrigger` (triggers.rs) gained `Debug` for the same reason.
- `engine::StepStatus` gained `Debug` — same reason; `CommandError`/`ActionError` already had it.

**INTERFACE DEVIATIONS:**
- `Engine` gained a new public method, `pipeline_context_active(&self) -> bool` — a diagnostic for
  tests/replay tooling to confirm `pipeline_context` is `None` once back at `PriorityWindow`. Not
  in any prior phase's interface list; added because there was no other way for an external test to
  observe that the documented "MUST clear `pipeline_context`" invariant (see the `pipeline_context`
  field doc, and the "Fix: pipeline_context lifecycle" note above) actually holds.

**SCOPE DEVIATIONS:**
- `crates/scenario/src/lib.rs`'s `build_initial_state` needed `postcombat_main: false` added to its
  `GameState` literal — same "verification tooling has to track new fields" pattern as Dry Run #1's
  `priority_player`/`consecutive_passes` addition.
- `tests/scenarios/pass_priority.toml` and `tests/scenarios/schema_check.toml` both had stale
  `ledger_len` assertions/comments written against the all-`todo!()` baseline (`0` events expected).
  Now that `PassPriority` genuinely runs and commits an event, both were updated to the real
  resulting count (`1` in each case) — same kind of observable-behavior change Dry Run #1 flagged
  for `schema_check.toml`, except this time the branch isn't being discarded, so the fixtures
  needed to actually be fixed rather than just noted.
- Wrote a new test file, `crates/engine/tests/phase5_pass_priority_loop.rs`, rather than only TOML
  scenarios — the acceptance criteria for this slice (StepStatus alternation, pipeline_context
  clearing between runs, exact ledger contents/order) need direct `Engine` access that the
  TOML-driven `stackfish_scenario` harness intentionally abstracts away (it only loops until
  `AwaitingAction`/etc, collapsing intermediate `Progressed` steps). Builds `GameState`/`StackObject`
  via struct literals directly, same pattern `stackfish_scenario::build_initial_state` already
  established for the same reason (engine constructors are still `todo!()` past `PassPriority`).

**UNRESOLVED:**
- `ResolveStackTop` sets `priority_player = active_player` immediately upon popping the stack,
  rather than only after SBA/triggers settle — a simplification for this slice; CR-accurate timing
  would assign priority only once the whole resolution (including any triggers it causes) has fully
  resolved. Harmless today since nothing between here and `PriorityWindow` reads/depends on a
  different value, but flag before anything starts caring about priority *during* a stack
  resolution's SBA/trigger sub-steps.
- `AdvanceTurnStep`'s untap/upkeep/draw/combat steps don't perform any of the real rules work CR
  500-514 actually requires there (untapping permanents, drawing a card, etc.) — only phase/step
  bookkeeping. CR 502.1's "players don't get priority during untap" special case also isn't
  modeled; this slice grants priority uniformly at every step, Untap included.
- `check_state_based_actions` returning unconditional `Vec::new()` and `apply_replacements`'s
  empty-registry-only fast path are both real no-ops *today* (nothing registered in any scenario
  this slice tests), but neither is closer to the actual CR 614-616/704.5 logic than before — purely
  "doesn't block the pipeline from terminating," not partial progress on either subsystem.

**Follow-up verification pass (same branch, second commit, before merge):** the human asked for a
targeted re-check against five specific risks before merging. Found and fixed one real gap, added
coverage for the rest:
- `submit_trigger_order` only checked `chosen_order.len() == group.len()` — same length doesn't
  rule out a substituted, dropped+duplicated, or repeated `PendingTrigger`. Added a real
  `is_permutation` multiset check (remove-and-match, no `Ord` needed) and a new
  `TriggerOrderError::NotAPermutation`. Required adding `PartialEq, Eq` to `PendingTrigger`. Unit
  tests live in `engine.rs` itself (`#[cfg(test)] mod tests`), not the integration test file —
  exercising this needs to set `Engine::process_state` directly, which is private outside the crate.
- Ordering (`Vec::pop()` vs `VecDeque::pop_front()`): audited, no bugs found. `state.stack.pop()` is
  correctly LIFO (intentional — last object pushed resolves first); `ReplacementCheck::remaining`
  and `PutTriggersApnap::pending_groups` both correctly use `pop_front` (FIFO) — the Dry Run #1
  reversal bug (commit `e3fd3ba`) was already fixed before this slice started.
- "Non-pass action resets pass chain": real implementation only had `PassPriority` to exercise this
  with (every other `PlayerActionKind` still `todo!()`), so added a minimal `MakeChoice` stub in
  `commands.rs` (`events: Vec::new()`, `broke_pass_chain: true`, `priority_handoff: Unchanged`) —
  not a real implementation of `MakeChoice` itself, exists solely so the reset wiring has a
  non-PassPriority command to test against. New test: `non_pass_action_breaks_pass_chain`.
- `pipeline_context` clearing: the existing test only checked the post-condition (`None` after
  drain) — strengthened to also assert it's `Some` immediately after the first `step()` of a run,
  so the test can't pass trivially from a context that was never set in the first place.
- All-pass+empty-stack / all-pass+non-empty-stack branches: already covered by Dry Run #2's
  original tests, re-confirmed correct on re-read, no changes needed.

**MERGE:** `--no-ff` into master. Both `phase5/pass-priority-loop` commits (`9cb46d6`,
`23350de`) landed; `cargo test --workspace` re-confirmed green on master post-merge.

## Fix: `ZoneChange` had no last-known-information slot (CR 603.10)

Found while discussing what a future landfall trigger's condition would actually have to check.
`GameEventKind::ZoneChange` was `{ object: ObjectId, from: ZoneKind, to: ZoneKind }` — just an id,
no snapshot of what `object` looked like in `from`. `collect_triggers` (still `todo!()`, but this
would block it the moment it's real) would have to look `object` up live in `GameState.objects` to
evaluate a leaves-the-battlefield condition — except a battlefield -> graveyard move mints a *new*
`ObjectId` (`ObjectLineage`), so the old id may not resolve to anything, or whatever does resolve
already lost characteristics it only had up to the moment it left (e.g. an ability granted by an
aura that left in the same SBA batch). Same class of bug `StackObject.source_lki` (CR 608.2b)
already exists to prevent on the stack side — `ZoneChange` had no equivalent.

Fixed: added `last_known_info: Option<GameObject>` to `ZoneChange`, to be populated by whoever
produces the event (a snapshot of `object` taken before the move, not after). No real producer
exists yet (`PlayLand`/etc are still `todo!()` in `execute_command`, and nothing in
`crates/scenario` constructs `GameEventKind` directly) — this is a type-level fix ahead of the
first real producer landing, not a behavior change. `triggers::collect_triggers`'s TODO updated
to flag that leaves-a-zone conditions must prefer this field over a live `state.object()` lookup
once that function is real.

**STRUCT DEVIATIONS:** `GameObject` needed `Clone` to be embeddable in an event, which cascaded
`Clone` (plus `Debug`, added alongside for free) down its whole field chain: `CardDefinition`,
`ScriptReference`, `Subtype`, `PTValue`, `ManaCost`, `ManaSymbol`, `AtomicManaSymbol`, `Color`,
`CounterSet`, `CounterKind`, `VisibilityPolicy`, `ObjectLineage`, plus `Debug` on the three
bitflags sets (`CardTypeSet`/`SupertypeSet`/`ColorSet`) so `CardDefinition`'s own `Debug` derive
compiles. Mechanical derives only, no logic changes. Side benefit: `CounterKind` also picked up
`PartialEq, Eq` — a gap flagged back in Phase 3 ("`CounterSet::count`/`add` can't compare kinds
without it") that hadn't been fixed yet; fixed here as a side effect of the same pass, not a
separate deliberate task.

`cargo test --workspace` reconfirmed green after the change (no behavior touched, only ledger
event size grew slightly via the new field).

## Phase 5: phase5/play-land slice (merged to master)

Branch `phase5/play-land`, off master. Same "kept, reviewed, merged `--no-ff`" treatment as
`phase5/pass-priority-loop`. Slice scope: `PlayerActionKind::PlayLand` (CR 305.1) as the first real
caller of the zone-manipulation helpers (`Zone::remove`/`push_bottom`,
`GameState::object`/`object_mut`/`zone`/`zone_mut`), all `todo!()` since Phase 1. Reuses the
existing `ReplacementCheck`→`CommitEvents`→`SbaLoop`→`CollectTriggers`→`PutTriggersApnap` pipeline
proven generic by the PassPriority slice — no new pipeline wiring needed, just a new producer.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` green both pre-merge (on the branch) and
post-merge (on master) — 9 tests in `crates/engine/tests/phase5_play_land.rs`, 1 new in-crate unit
test (`types::game::tests::zone_helpers_roundtrip`), plus the pre-existing 3+8+1 suites unaffected.

**What got implemented:**
- `Zone::remove`/`push_bottom`, `GameState::object`/`object_mut`/`zone`/`zone_mut` — straight
  one-line fills, exactly matching the TODO comments already on each. `Zone::new`/`push_top`/
  `contains`/`len`/`top` and `GameState::player`/`player_mut` deliberately left `todo!()` — not
  needed by this slice.
- `commands::execute_command`'s `PlayLand` arm: validates the card is actually a `Land`
  (`CardTypeSet::LAND`) and actually sits in the actor's hand under the actor's control, snapshots
  it for `ZoneChange::last_known_info` (CR 603.10), moves it hand→battlefield via the helpers
  above, sets `GameObject.zone` to match (single source of truth for *order* is still the zone
  `VecDeque`; this field is just the classification, kept in sync by hand here since nothing else
  does it for this code path yet). `broke_pass_chain: true`, `priority_handoff: Unchanged`.
- Mid-review, the human flagged that the original implementation skipped CR 305.1/305.2/305.3
  entirely (priority+main-phase+empty-stack timing, and the one-land-per-turn limit) — caught
  before merge, not after. Added `commands::is_sorcery_speed(state, player)` as a standalone
  predicate (priority held + actor's own turn + `Phase::Main` + empty stack), called from
  `PlayLand` and documented as the thing `CastSpell`/`ActivateAbility` will need too once real.
  Added `GameState.lands_played_this_turn: HashMap<PlayerId, u32>`, incremented by `PlayLand`,
  cleared in `engine::Engine::advance_turn_step`'s Cleanup→next-turn transition (same trigger as
  `postcombat_main`'s reset).

**STRUCT DEVIATIONS:**
- `GameState` needed `lands_played_this_turn: HashMap<PlayerId, u32>` — not anticipated until the
  human's sorcery-speed/land-limit review pass; same "verification tooling has to track new
  fields" ripple as `postcombat_main` before it (every existing `GameState` struct literal across
  `engine.rs`, both `tests/phase5_*.rs` files, `crates/scenario/src/lib.rs`, and `game.rs`'s own
  test module needed the field threaded through).

**INTERFACE DEVIATIONS:**
- `commands::is_sorcery_speed` — new private fn, not anticipated in the original slice plan (which
  only called for the bare hand→battlefield move). Added in direct response to the human's comp-
  rules review, not part of the original acceptance criteria.

**SCOPE DEVIATIONS:**
- `engine::Engine::advance_turn_step`'s Cleanup arm — one new line (`lands_played_this_turn.clear()`)
  alongside the existing `postcombat_main = false` reset. Not in the original TODO list for this
  slice; needed once the land-limit tracking existed at all.

**UNRESOLVED (flagged with explicit `TODO` comments in `commands.rs`, not just here):**
- `is_sorcery_speed` doesn't consult `state.continuous_effects`/`abilities` for rules-text timing
  permissions (flash-granting effects, "play lands as though it were your main phase," etc.) —
  there's no query API on `continuous_effects` yet (`layers.rs` is still mostly `todo!()`).
- The land-limit check is hardcoded against the literal `1`, ignoring any continuous effect that
  would raise a player's allowed land-plays-per-turn (e.g. "you may play an additional land each
  turn") — same blocker as above.

## Fix: PlayLand reused the hand card's ObjectId instead of minting a new one (CR 400.7)

`ids.rs`'s own doc comment on `ObjectId`/`ObjectLineage` says a zone change that creates a "new
object" mints a fresh id rather than reusing the old one — but the merged `PlayLand` slice above
actually reused `card`'s id across the hand→battlefield move, just flipping `GameObject.zone`.
Caught while scoping the next slice (`CastSpell`, which does the same kind of move twice over:
hand→stack→battlefield/GY) — needed one consistent convention before writing a second mover, and
the doc-intent was the more correct of the two to standardize on (matters for real once continuous
effects/auras need to ask "is this still the same object that was targeted/granted-an-effect").

Fixed on master directly (not its own dry-run branch — a correctness fix to already-merged code,
not a new vertical slice): added `GameState.next_object_id: u64` + `allocate_object_id()` (mirrors
`allocate_event_id`/`allocate_sequence_no`). `PlayLand`'s arm now mints a new id, sets
`ObjectLineage.previous = Some(old_id)`, gives the new `GameObject` a fresh timestamp, and removes
the old id from `state.objects` entirely. `ZoneChange.object` now carries the new id;
`last_known_info` still snapshots the old one — that field is genuinely load-bearing now, not just
defensive, since the old id no longer resolves to anything afterward.

**STRUCT DEVIATIONS:** `GameState.next_object_id` (above) — rippled into all 5 `GameState` struct-
literal call sites (`game.rs`'s own test, `engine.rs`'s test, `crates/scenario/src/lib.rs`, both
`tests/phase5_*.rs` files) needing the field threaded through, same pattern as every prior
`GameState` field addition.

**SCOPE DEVIATIONS:** 3 assertions in `phase5_play_land.rs` assumed id-reuse and had to be rewritten
to read the new id back off the `ZoneChange` ledger event instead of looking it up by the original
id (added a `moved_object_id()` test helper for this). `cargo test --workspace` reconfirmed green
after.

## Phase 5: phase5/cast-spell slice (merged to master)

Branch `phase5/cast-spell`, off master. Same "kept, reviewed, merged `--no-ff`" treatment as the
prior two real slices. Slice scope: `PlayerActionKind::CastSpell` (CR 601) with cost stubbed to
free (no `CostSpec`/`PaymentPlan` flow at all this slice — deliberate, payment is its own next
slice, see below), plus the CR 608.2m resolution half: a resolved permanent-type spell becomes a
battlefield permanent, a resolved instant/sorcery goes to its owner's graveyard. Reuses the existing
pipeline end to end — `ResolveStackTop`'s body went from a pure mock event to doing the real
stack→battlefield/GY move, the first real work that stage has ever done.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` green both pre-merge and post-merge — 9
tests in `crates/engine/tests/phase5_cast_spell.rs`, all prior suites (21 tests + scenarios)
unaffected. 30 tests total, 0 failed.

**What got implemented:**
- `StackObject::new` — real body, signature grew to take `chosen_modes`/`targets`/`x_value` as
  params (its own doc comment had already flagged this as likely).
- `commands::execute_command`'s `CastSpell` arm: rejects lands (CR 305.1 — lands use `PlayLand`,
  never go through this path) and cards not in the actor's hand under their control. Timing
  branches on card type: instants only need the actor to hold priority (CR 601.3 "any time" via
  flash-equivalent base rule); everything else needs the full `is_sorcery_speed` window (same
  predicate `PlayLand` uses). Mints a new `ObjectId` for the hand→stack move (CR 400.7, same
  convention as the `PlayLand` fix above), builds a real `StackObject` with `costs_paid: vec![]`
  (the free-cost stub), pushes it onto `state.stack`. `broke_pass_chain: true`,
  `priority_handoff: Unchanged` (CR 601.2i — the caster already held priority, so it doesn't move).
- `engine::Engine`'s `ResolveStackTop`: for `StackEntryKind::Spell` only, looks up the spell's
  `GameObject` by `stack_id`, checks whether its `CardTypeSet` intersects `INSTANT | SORCERY` to
  decide battlefield (permanent) vs. owner's graveyard (instant/sorcery), mints another new
  `ObjectId` for that move, emits a second `ZoneChange`. Non-`Spell` stack entries
  (`ActivatedAbility`/`TriggeredAbility`/`Copy`) correctly produce no `ZoneChange` — nothing on a
  zone moves when an ability resolves.

**STRUCT DEVIATIONS:** None beyond the `next_object_id` fix already folded in above.

**INTERFACE DEVIATIONS:** `StackObject::new` gaining 3 params — anticipated by its own doc comment,
not a surprise mid-slice.

**SCOPE DEVIATIONS:** None — touched exactly `stack.rs`, `commands.rs`, `engine.rs`, plus the new
test file.

**UNRESOLVED (flagged with explicit `TODO` comments in `commands.rs`/`engine.rs`, not just here):**
- Cost determination/payment (CR 601.2f-h) — entirely stubbed free at the time this slice merged.
  Implemented in the very next slice — see "Phase 5: phase5/cast-spell-payment slice (merged to
  master)" below.
- Mode legality, target legality/count, division/distribution, legality recheck (CR 601.2b-e) —
  `modes`/`targets`/`x_value` are accepted and stored on the `StackObject`, never validated.
- Flash/timing permissions beyond the base instant-or-not split (continuous effects granting
  flash, e.g. "you may cast this as though it had flash") — not modeled, same shape of gap
  `is_sorcery_speed` already flags for `PlayLand`.
- `operation: ScriptHandle(0)` on the pushed `StackObject` — no real script bound yet (same
  script-bridge gap as everywhere else). Resolution moves the card between zones for real but runs
  no actual spell effect.
- No SBA checks, no countering/fizzling on illegal-target-by-resolution-time — resolution is
  unconditional once reached.

## Phase 5: phase5/cast-spell-payment slice (merged to master)

Branch `phase5/cast-spell-payment`, off master. Same "kept, reviewed, merged `--no-ff`" treatment
as every real slice before it. Slice scope: cost determination + payment (CR 601.2f-h) for
`PlayerActionKind::CastSpell` — the free-cost stub from the cast-spell slice above is replaced with
a real `CostSpec` built from the card's `mana_cost`, and the pipeline now genuinely pauses for
payment instead of assuming it's free.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 8 new tests in
`crates/engine/tests/phase5_cast_spell_payment.rs`, plus 4 of the 9 pre-existing
`phase5_cast_spell.rs` tests updated (see SCOPE DEVIATIONS) since they now pause where they
previously ran straight through. 38 tests total, 0 failed, both pre-merge and post-merge.

**Design decision that held as planned:** CR 601.2a puts the spell on the stack *before* cost is
determined/paid (601.2f-h) — casting is genuinely mid-process while uncosted. Decided to model
that with a new pipeline pause rather than deducting mana synchronously inside the `CastSpell` arm:
`ProcessState::AwaitingPayment { stack_id, cost }`, a `StepStatus::AwaitingPayment`, and a new
`Engine::submit_payment(plan)` entry point, mirroring how `AwaitingReplacementChoice`/
`AwaitTriggerOrderChoice` already pause-and-resume mid-pipeline. Matches the design note already in
`command.rs`'s doc comment (`SubmitPayment` exists as its own `PlayerActionKind` *because* payment
is its own decision point, decoupled from `CastSpell`) and sets up the right shape for later
additional-cost decisions (sacrifice-a-creature-as-a-cost needs to ask "which one?") and for real
mana abilities once those exist. The cheaper synchronous-deduction alternative was considered and
rejected — same reasoning as picking the more-correct `ObjectId` convention over the cheaper one in
the fix above.

**Mana abilities (CR 605) are explicitly NOT part of this slice.** Tapping a land for mana
(`{T}: Add {C}`) is itself a CR 605.1a mana ability — an activated ability that doesn't go on the
stack and resolves immediately (605.3b). That needs real `RegisteredAbility` activation (CR 602)
running through the script bridge, neither of which exists yet (`state.abilities` sits empty,
there's no `activate_ability` execution path at all). That's its own future slice, underneath this
one. For payment, test fixtures will just hand-set `PlayerState.mana_pool` directly — fake,
pre-seeded floating mana, no land/tap path produces it for real. `CostPayment::Mana(ManaCost)`
stays provenance-free (no "which land was tapped" — there's no real tapping happening to attribute
the mana to).

**What got implemented (matches the planned requirements list 1:1):**
- `CostSpec::mana_only` and `PaymentPlan::validate` (`cost.rs`) filled for real. `validate` takes
  `(&self, spec: &CostSpec, pool: &ManaPool)` exactly as planned — checks colored requirements
  against matching `ManaPool` fields, generic against whatever's left over, rejects anything with
  `spec.additional`/`spec.alternative` set or a non-colored/generic symbol (Colorless/Snow/
  Phyrexian/Hybrid/X) since those aren't modeled this slice. A `{0}` cost only validates against an
  *empty* `payments` vec — explicit acknowledgment, not "nothing to check."
- New `pub(crate) cost::spend(&CostSpec, &mut ManaPool)` — the commit-side mirror of `validate`'s
  accounting (colored off first, generic off the leftover, colorless prioritized). Not in the
  original requirements list (which only said "needs to deduct from `ManaPool`") but written as its
  own function so deduction can't drift from what validation just proved affordable.
- `commands::execute_command`'s `CastSpell` arm now builds `CostSpec::mana_only(card's mana_cost)`
  and returns it via `CommandOutcome::requires_payment` instead of assuming free.
- `engine::Engine` gained `ProcessState::AwaitingPayment`, `StepStatus::AwaitingPayment`,
  `PaymentError`, and `Engine::submit_payment` exactly as planned — `CommitEvents` checks a new
  `pending_payment` field (set in `PendingAction` from the command outcome) to decide whether to
  pause here or fall through to `SbaLoop` as before.
- `GameState::player`/`player_mut` (`game.rs`) filled for real — were deliberately left `todo!()` in
  the `PlayLand` slice ("not needed by this slice"); payment needs them to reach the payer's
  `mana_pool`.
- Added `GameEventKind::CostPaid { stack_id, payer }` — the ledger fact that completes the cast
  process, not in the original event list.

**STRUCT DEVIATIONS:**
- `CommandOutcome` gained `requires_payment: Option<PendingPaymentRequest>` (new struct:
  `stack_id`/`payer`/`cost`) — every pre-existing arm needed `requires_payment: None` added.
- `Engine` gained a private `pending_payment` field, separate from `pipeline_context` — different
  lifetime (one command's outcome, not "this pipeline run" in general); `ResolveStackTop` runs never
  set it, so they fall through `CommitEvents` to `SbaLoop` unaffected.

**INTERFACE DEVIATIONS:** None beyond what requirement 5 above already anticipated
(`PaymentPlan::validate` gaining the `pool` param) — `Engine::submit_payment`'s shape matched the
design note as drafted.

**SCOPE DEVIATIONS:**
- `crates/scenario/src/lib.rs`'s `run_action` needed `StepStatus::AwaitingPayment` added to its
  pause-handling match arm — same "verification tooling has to track new states" pattern as every
  prior slice's `GameState` field additions.
- `phase5_cast_spell.rs`'s 9 pre-existing tests all use empty-`mana_cost` fixtures. CR 118.5a's
  "even a `{0}` cost needs an explicit pay" meant 4 of those 9 now stop at `AwaitingPayment` instead
  of running straight through to `AwaitingAction`. Added a `pay_free_and_drain` test helper and
  updated those 4 call sites — the same "fixtures go stale the moment a path stops being a stub"
  pattern `implementation.md` has flagged repeatedly (`schema_check.toml`, `pass_priority.toml`).

**UNRESOLVED (flagged with explicit `TODO`/doc comments in the code, not just here):**
- Additional costs, alternative costs, cost reductions/increases (CR 118.7), hybrid/Phyrexian
  payment-choice (CR 118.13a), `{X}` folding into the generic component — all exactly the stubs the
  pre-slice plan called out, left untouched.
- Mana abilities / tapping lands for mana (CR 605) — still no way to get mana into a pool other than
  a test fixture hand-setting it directly. Its own future slice, underneath this one.
- CR 601.6 (can't pay → casting is illegal, game returns to before it was proposed) — `submit_payment`
  rejecting a plan leaves the engine sitting in `AwaitingPayment` indefinitely; nothing un-pushes the
  stack object or returns the card to hand on failure. Flagged as a real gap, not silently papered
  over — same status as when the pre-slice plan first called it out.

## Fix: scenario crate (TOML)/CLI hadn't been kept current for payment/casting

The human asked directly whether the TOML scenario format had been kept updated through the last
couple of merged slices (CastSpell, then cost+payment) so they could write their own `.toml` fixtures
to exercise casting and payment by hand. It hadn't — `crates/scenario/src/lib.rs` still had a
deliberate "SubmitPayment intentionally omitted for v1" comment from Phase 4, predating both slices,
and nothing had gone back to revisit it. Concretely, three gaps in `crates/scenario/src/lib.rs`:

1. No `SubmitPayment` `ActionSpec` — no TOML shape could answer a `ProcessState::AwaitingPayment`
   pause at all.
2. `PlayerSpec` had no mana pool field — `build_initial_state` hardcoded every player's `mana_pool`
   to all-zero, so even if `SubmitPayment` existed, there was nothing to pay *with* (CR 605 mana
   abilities don't exist yet — see the cast-spell-payment slice's own notes on why fixtures must
   pre-seed the pool directly).
3. No assertion could read a `ManaPool` field, so a payment's debit couldn't be checked from TOML
   either.

**Fixed**, plus the CLI was checked and needs no changes — `crates/cli/src/main.rs` only iterates
`report.action_outcomes`/`assertion_outcomes` generically by index; it has no per-action-type logic,
so a new `ActionSpec`/`AssertionSpec` variant is invisible to it by construction.

- Added `PlayerSpec.mana_pool: ManaPoolSpec` (new struct, defaults to all-zero) — wired into
  `build_initial_state`'s `PlayerState` literal.
- Added `ActionSpec::SubmitPayment { player, mana: ManaPaymentSpec }` (new struct, same simplified
  generic+colors shape `CardSpec` already uses for a card's cost). Deliberately NOT routed through
  `to_player_action`/`run_action`/`submit_action` — `Engine::submit_payment` is its own entry point
  (mirrors `submit_replacement_choice`/`submit_trigger_order`), so `run()` intercepts this variant
  before `to_player_action` ever sees it and calls a new `run_payment` (parallel to `run_action`,
  sharing a new `drain` helper extracted from both).
- Added `AssertionSpec::ManaPool { player, color, equals }` to verify debits.
- Factored the generic+colors → `Vec<ManaSymbol>` logic (previously inline in `build_initial_state`)
  into a shared `mana_symbols` helper, used by both `CardSpec` and the new `ManaPaymentSpec`.

**Second gap found while smoke-testing the fix through the real CLI** (not just `cargo test`):
writing an actual TOML scenario that casts and pays for a spell exposed that `ObjectZone` assertions
silently broke the moment the tracked card changed zones. Reason: `PlayLand`/`CastSpell`/resolution
all mint a *new* `ObjectId` per move (CR 400.7) — `card_ids` (the TOML-key → `ObjectId` map, built
once at setup) was never updated afterward, so a key like `"bear"` kept pointing at a hand-zone id
that no longer existed once the card moved to the stack. This wasn't a side effect of this slice's
own changes — `PlayLand`/`CastSpell` were never exercised via a TOML scenario before now (only via
direct Rust integration tests, which track the new id themselves) — but it directly blocks the
human's stated goal, so fixed in the same pass: added `remap_moved_card_ids`, called in `run()`
after every action, which scans that action's newly-added ledger slice for `ZoneChange` events and
follows each one's `last_known_info` (the old id) forward to its replacement in `card_ids`.

**VERIFICATION:** `cargo test --workspace` still 38/38 (the new fixture is picked up automatically by
`tests/scenarios.rs`'s directory scan). Added `tests/scenarios/phase5_cast_spell_payment.toml` —
seeds a player's `mana_pool`, passes priority 4× to reach `Phase::Main` (CastSpell's non-instant
branch needs sorcery speed), casts a real-cost creature, pays it via `SubmitPayment`, passes once
more to resolve the stack, and asserts the pool is debited to 0 and the card landed on the
battlefield. Confirmed directly via `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/
phase5_cast_spell_payment.toml` (not just the test harness) — output: 7/7 actions ran, 3/3 assertions
passed.

## Fix: `state.step` left stale during Main phase (CR 500.4/505.1)

Found while adding `phase`/`step` assertions to the scenario format (human asked for these directly
after asking why a solo-player TOML scenario needs explicit `pass_priority` actions through
`PreGame`/`Untap` — answer: `PreGame` is engine-only bootstrap with no priority concept, and CR
502.1's "no priority during Untap" was never modeled, both flagged already in the pass-priority-loop
slice's UNRESOLVED list; `ProcessState::PriorityWindow` has no way out except a real `PlayerAction`
either way). Writing a `step = "untap"` assertion against the cast-spell-payment fixture failed —
got `Draw` instead. Real bug, not a doc gap: `engine::Engine::advance_turn_step`'s `(Phase::Beginning,
Step::Draw) => { phase = Main; ... }` arm (and the symmetric `(Phase::Combat, Step::EndOfCombat) =>
{ phase = Main; ... }` arm) never touched `state.step` — `Step` had no variant for "in Main," so
whatever the prior real step was just sat there stale for the entire Main phase, because nothing
needed to read `step` during Main until now.

**Human's fix, taken as-is:** add `Step::PreCombatMain`/`Step::PostCombatMain` and set them on
entering Main, rather than reading the existing `GameState.postcombat_main: bool` (added back in the
original AdvanceTurnStep slice) to disambiguate. This subsumes that field entirely — `Step` itself
now encodes CR 505.1's "Main happens twice per turn" ambiguity that field existed to resolve, so
`postcombat_main` was removed from `GameState` rather than kept alongside the new `Step` variants
(two fields saying the same thing would just be the `zone_position` mistake again — see that note
near the top of this doc).

**STRUCT DEVIATIONS:** `GameState.postcombat_main: bool` removed — rippled into every `GameState`
struct-literal site (`game.rs`'s own test, `engine.rs`'s test, `crates/scenario/src/lib.rs`, all four
`tests/phase5_*.rs` files) the same way every prior field addition/removal has.

**SCOPE DEVIATIONS:** `crates/scenario/src/lib.rs`'s `parse_step` needed `"pre_combat_main"`/
`"post_combat_main"` added (new `Step` variants need TOML names); `docs/scenario_format.md`'s `step`
assertion docs/example updated to match (the example's step assertion was about to document the
*old*, stale-on-purpose behavior — caught before that shipped, fixed to assert the new real value
instead).

**VERIFICATION:** `cargo test --workspace` still 38/38. Re-ran `cargo run -p stackfish-cli --
crates/engine/tests/scenarios/phase5_cast_spell_payment.toml` with its `step` assertion now asserting
`"pre_combat_main"` (previously would have needed to assert the stale `"draw"` to pass) — 3/3
assertions pass for real, not against the bug's behavior.

## Phase 5: phase5/mana-payment-choice slice

Branch `phase5/mana-payment-choice`, off master. Slice scope: CR 118.13a's actual payment choice —
which floating mana pays a colored pip vs. the generic component is the *payer's* decision, not the
engine's. Came up directly from the human asking how the previous slice's hardcoded
colorless-then-WUBRG spend order could instead be a real choice; the answer (discussed before any
code changed) was that `PaymentPlan` should record the payer's literal choice of units, not an echo
of the cost's shape — this slice is that design, implemented.

**What changed:**
- New `ManaUnit { Colored(Color), Colorless }` (`mana.rs`) — deliberately narrower than
  `AtomicManaSymbol`: only the things that can actually sit in a `ManaPool` (no `Generic`/`Snow`/
  `Phyrexian`, which describe cost shapes or payment-choice mechanics, not pool contents).
- `CostPayment::Mana(ManaCost)` → `CostPayment::Mana(Vec<ManaUnit>)` — a flat list of the exact
  units spent, one entry per unit, covering both colored pips and whatever was chosen to cover the
  generic component.
- `PaymentPlan::validate` (`cost.rs`) rewritten: every colored pip must be paid by that exact color
  (no substitution — checked first), then whatever's spent *beyond* each pip's exact requirement
  (extra of a required color, a different color, or colorless) must sum to *exactly* the generic
  amount, then the pool must actually hold what's being spent. `is_symbol_multiset_equal` (the old
  "echo the cost's shape" check) is gone — superseded by this.
- `cost::spend` rewritten: no priority order anymore (colorless-then-WUBRG is gone entirely) — it
  just decrements the pool by literally what `units` says, since the payer already chose.
- `engine::Engine::submit_payment` updated to extract `units` from the plan's single
  `CostPayment::Mana` entry and pass it to `spend`, instead of passing the `CostSpec`.

**STRUCT/INTERFACE DEVIATIONS:** None beyond the deliberate `CostPayment::Mana` shape change above
— this *is* the slice, not a deviation from it.

**SCOPE DEVIATIONS:**
- `crates/scenario/src/lib.rs`'s `ManaPaymentSpec` (the `SubmitPayment` TOML action's `mana` field)
  redesigned from `{ generic_cost, colors }` to a single flat `units: Vec<String>` field (`"W"`/
  `"U"`/`"B"`/`"R"`/`"G"`/`"C"`) — mirrors the new `CostPayment::Mana` shape directly. New
  `parse_mana_unit` helper.
- `phase5_cast_spell_payment.rs`'s 8 existing tests all constructed `CostPayment::Mana(ManaCost {
  ..})` directly — every one needed rewriting to build explicit `Vec<ManaUnit>` payments instead.
  Added 2 new tests demonstrating the actual feature this slice adds:
  `colored_pip_cannot_be_substituted_by_a_different_color` and
  `generic_can_be_paid_with_a_mix_of_colors`.
- `tests/scenarios/phase5_cast_spell_payment.toml` and `docs/scenario_format.md`'s worked example
  both updated to the new `mana = { units = [...] }` shape.

**VERIFICATION:** `cargo test --workspace` — 40 tests, 0 failed (38 prior + 2 new). Re-confirmed via
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_cast_spell_payment.toml` after
the TOML schema change — 7/7 actions ran, 5/5 assertions passed.

**UNRESOLVED (unchanged from the prior slice, not touched here):** mana abilities (CR 605, still no
real way to get mana into a pool), additional/alternative costs, hybrid/Phyrexian/snow/X-cost
payment (a colored-pip-only `ManaUnit` can't represent paying a Phyrexian pip with life, for
instance — that's still a real gap, just not this slice's), CR 601.6 (failed payment leaves the
engine stuck in `AwaitingPayment` rather than unwinding the cast).

## Phase 5: phase5/activate-ability-mana slice

Branch `phase5/activate-ability-mana`, off master. Slice scope: baseline `ActivateAbility` (CR
602), implemented far enough for one concrete case end-to-end: a mana ability (CR 605) that taps
its source and adds mana to the pool. CR 605.3b's mana-ability exception (no target, never uses
the stack, resolves immediately as part of activation) is the whole point of this slice — it's
the first `PlayerActionKind` variant that deliberately does *not* follow the cast-a-spell-shaped
pipeline (no stack push, no `AwaitingPayment` pause). Non-mana activated abilities get structural
lookup/validation only; effect resolution stays `todo!()` (same script-bridge gap as everywhere
else).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 8 new tests in
`crates/engine/tests/phase5_activate_ability_mana.rs`, all 40 prior tests unaffected. 48 tests
total, 0 failed.

**What got implemented:**
- `ability.rs`: `AbilityEffect::AddMana(Vec<ManaUnit>)` — structural stand-in for "this ability's
  effect is exactly CR 605 mana addition," reusing `ManaUnit` (the same "literal units, not a cost
  shape" vocabulary `CostPayment::Mana` already uses). `RegisteredAbility` gained `effect:
  Option<AbilityEffect>`. `AbilityKind` gained `Clone, Copy, PartialEq, Eq, Debug` — needed to
  compare/match it for the first time.
- `mana.rs`: `ManaPool::add_unit` — real body, the inverse of `cost::spend`.
- `event.rs`: new `GameEventKind::Tapped { object }` (first real tap producer in the codebase).
  `GameEventKind::ManaAdded` retargeted from `amount: ManaCost` (unused anywhere) to `units:
  Vec<ManaUnit>`, matching `CostPaid`/`CostPayment::Mana`'s convention instead of introducing a
  second, inconsistent mana vocabulary.
- `commands.rs`'s `execute_command`: new `ActivateAbility` arm. Validates, in order: actor holds
  priority (CR 117.3c) → source exists and actor controls it → the `ability_index`th ability on
  that source exists and is `AbilityKind::Activated`. Branches on `ability.effect`:
  `Some(AddMana(units))` rejects a non-empty `targets` (CR 605.1a), rejects an already-tapped
  source, then taps the source and adds `units` to the actor's pool — `broke_pass_chain: true`,
  `priority_handoff: Unchanged`, `requires_payment: None`, no `StackObject` ever created. `None`
  (a non-mana ability) falls through to `todo!()` after the same structural checks.
- `triggers.rs`'s `collect_triggers`: the no-op fast path widened from "`state.abilities` is
  empty" to "`state.abilities` has no `Triggered`-kind entry" — see Fix below, this slice's tests
  are the first ever to populate `state.abilities` with anything, and the old guard didn't
  distinguish ability kind.

**STRUCT DEVIATIONS:** None beyond `AbilityEffect`/`RegisteredAbility.effect` (exactly as scoped).

**INTERFACE DEVIATIONS:** None — `ManaPool::add_unit`'s signature matched the plan as drafted.

**SCOPE DEVIATIONS:**
- `triggers.rs` (not in the original TODO list for this slice) — see the Fix below.

**Fix: `collect_triggers`'s no-op guard didn't distinguish ability kind.** `collect_triggers`
already had an explicit "no-op when `state.abilities` is empty" fast path (added in the
pass-priority-loop slice, with a comment saying it'd need replacing "the moment any ability
actually exists"). This slice is the first to ever populate `state.abilities` with something
*non*-`Triggered` (an `Activated` mana ability) — the guard's `is_empty()` check doesn't care about
kind, so simply registering a mana ability tripped the `todo!()` below it on every `step()` that
reaches `CollectTriggers`, even though there's still genuinely nothing triggered to scan for.
Fixed by checking for at least one `Triggered`-kind entry instead of mere non-emptiness — same
"no-op when there's truly nothing to match against" rationale the original guard already
documented, just widened to the case this slice newly exercises. Real condition evaluation (the
actual `todo!()` underneath) is untouched.

**UNRESOLVED (flagged with explicit `TODO` comments in `commands.rs`, not just here):**
- CR 602.5a (summoning sickness for tap-cost abilities) — not checked; `GameObject` has no
  "controlled since" timestamp to check against, flagged in the original slice scope as
  out-of-scope.
- Non-mana activated abilities (`ability.effect == None`) — structural lookup/validation is real,
  but everything past that (cost determination beyond "tap," target validation, pushing a
  `StackObject`, resolving via `operation`) is `todo!()`, blocked on the script bridge like every
  other ability/operation gap in this codebase.
- `ability_index` is "the nth `Activated`/whatever-kind ability on this source by registration
  order in `state.abilities`," not a stable id — fine for this slice's single-mana-ability-per-card
  test fixtures, but will need revisiting once a card can have abilities added/removed at runtime
  (e.g. a granted ability) without shifting every other ability's index.
- ~~No TOML scenario yet exercising this through the CLI~~ — added in a follow-up pass (same
  branch): `CardSpec.mana_ability: Vec<String>` (`crates/scenario/src/lib.rs`) registers one
  `RegisteredAbility { kind: Activated, effect: AddMana(...) }` per card instance with a non-empty
  `mana_ability` list, activated via the existing `activate_ability` `ActionSpec` at
  `ability_index = 0`. New `AssertionSpec::Tapped { card, equals }` checks `GameObject.tapped`.
  `crates/engine/tests/scenarios/phase5_activate_ability_mana.toml` exercises it end-to-end —
  confirmed via `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/
  phase5_activate_ability_mana.toml` (1/1 actions ran, 3/3 assertions passed), not just `cargo
  test`. `docs/scenario_format.md` updated to match. Only one mana ability per card this way
  (`ability_index` is hardcoded to 0 by convention, not a list) — fine for this slice's scope, would
  need a real per-card abilities list in `CardSpec` to support more.

## Phase 5: phase5/declare-attackers-blockers slice

Branch `phase5/declare-attackers-blockers`, off master. Slice scope: baseline
`DeclareAttackers`/`DeclareBlockers` (CR 508.1/509.1, declarations only - no combat damage, no
triggered-ability collection beyond the new events themselves), plus CR 302.6/602.5a summoning
sickness folded in as planned (gates attacking here; retrofits the mana-ability tap-cost path from
the previous slice).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 22 new tests in
`crates/engine/tests/phase5_combat_declarations.rs`, 1 new regression test in
`phase5_activate_ability_mana.rs` (summoning sickness now gates that slice's mana ability too,
closing the TODO it left open), all prior suites unaffected. Confirmed via the real CLI too:
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_combat_declarations.toml` (16/16
actions ran, 3/3 assertions passed).

**Summoning sickness, as planned (derived fact, not a bool):**
- `GameObject.controlled_since_turn: u32` — set at creation (test/scenario fixtures set it
  directly; no "gain control" effect exists yet to retrofit, flagged TODO same as everywhere else).
- `GameState.last_active_turn_number: HashMap<PlayerId, u32>` — written by
  `engine::Engine::advance_turn_step` whenever active player rotates, plus once for the very first
  turn (the `PreGame` arm).
- `commands::has_summoning_sickness(state, object) -> bool` — pure predicate, `controlled_since_turn
  >= last_active_turn_number[controller]`. `None` (no recorded turn start) treated as sick, not as
  "unknown, so allow it." No haste override — not modeled in this engine yet, so this is the entire
  check.
- Consulted by both `DeclareAttackers` (CR 508.1a) and the existing `ActivateAbility` mana-ability
  arm (CR 602.5a) — one predicate, two call sites, as planned.

**`DeclareAttackers`/`DeclareBlockers`, as planned:**
- New `CombatState { attackers, blocks, blockers_declared }` (`types/combat.rs`), reusing the exact
  tuple shapes `PlayerActionKind::DeclareAttackers`/`DeclareBlockers` already carried since Phase 2
  — no separate wrapper type needed. Lives as `GameState.combat: Option<CombatState>`.
- `DeclareAttackers`: validates the whole batch before mutating anything (CR 508.1's atomicity) —
  actor is active player, correct step, no re-declaration, each attacker controlled/creature/
  not-a-battle/untapped/not-sick, each target a valid opponent or their planeswalker/battle. Then
  taps every attacker and records `CombatState`. New `GameEventKind::AttackerDeclared` per attacker
  (alongside the existing `Tapped`).
- `DeclareBlockers`: same atomicity discipline — actor is the defending player (2-player only;
  multiplayer's "which defending player" ambiguity isn't modeled), correct step, attackers already
  declared and blockers not yet declared this combat, each blocker controlled/creature/not-a-battle/
  untapped, no blocker assigned twice, each referenced attacker actually attacking this defender.
  New `GameEventKind::BlockerDeclared` per pairing. Blocking doesn't tap.
- `advance_turn_step`'s `EndOfCombat` arm clears `state.combat = None` (CR 506.4) so it never leaks
  into the next combat.

**STRUCT DEVIATIONS:** None beyond what the pre-slice plan called for (`CombatState`,
`GameObject.controlled_since_turn`, `GameState.combat`/`last_active_turn_number`) — this *is* the
plan, not a deviation from it.

**INTERFACE DEVIATIONS — one real, unplanned fix:** `Engine::submit_action`'s blanket "actor must
hold priority" gate turned out to be wrong for these two actions. CR 508.1/509.1 explicitly call
them turn-based actions that "don't use the stack" — they aren't a priority decision at all. The
defending player has no reason to hold priority when the declare-blockers step opens (508.2: the
active player keeps priority through the declare-attackers step) — discovered when every
`DeclareBlockers` test failed with `NotYourTurn` despite `execute_command`'s own actor check being
correct. Fixed by special-casing `DeclareAttackers`/`DeclareBlockers` in `submit_action` to skip the
priority check, leaving `execute_command`'s active-player/defending-player check as the sole gate.
Documented as a deliberate simplification, not the fully correct model — CR-accurate behavior would
have these fire automatically on entering their step, before any priority window opens, which this
engine's `ProcessState` pipeline has no stage for (every step currently opens a generic priority
window, combat steps included).

**SCOPE DEVIATIONS:**
- `crates/scenario/src/lib.rs`: `SetupEntry.controlled_since_turn: u32` (default `0`, always
  "not sick" since the first turn seeds `last_active_turn_number` to `1`) and `GameState.combat`/
  `last_active_turn_number` added to `build_initial_state` — same "harness has to track new fields"
  ripple as every prior slice, plus the harness has to seed `last_active_turn_number` itself since
  it builds `GameState` directly rather than stepping through `PreGame`'s real transition.
- Every existing `GameObject` struct literal (4 test files + scenario crate) and `GameState` literal
  (5 test files + scenario crate + `game.rs`/`engine.rs`'s own unit tests) needed the new fields
  threaded through — anticipated ripple, not a surprise (flagged in the pre-slice scoping).
- `docs/scenario_format.md` updated: `controlled_since_turn` under `[[setup]]`, real (not
  "unimplemented") behavior documented for `declare_attackers`/`declare_blockers`, and the
  no-priority-required note.

**UNRESOLVED (flagged with explicit `TODO`/doc comments in the code, not just here — all called out
in the pre-slice scoping, none new):**
- Combat damage resolution (CR 510), first/double strike extra damage step.
- Triggered abilities actually firing on attack/block declaration — same `collect_triggers`
  script-bridge gap as everywhere else; `AttackerDeclared`/`BlockerDeclared` exist so that future
  pass has something to match against.
- Costs to attack/block (508.1g-j/509.1d-f), banding (508.1e), reselecting attack target (508.7).
- Restrictions/requirements on attacking or blocking — needs a `continuous_effects` query API that
  doesn't exist (`is_sorcery_speed` already flags this same gap for `PlayLand`).
- Multiplayer (attack-multiple-players, range of influence) — 2-player only.
- Haste — sickness check has no override.
- Step-skip on zero attackers (CR 508.8) — `advance_turn_step` stays an unconditional table,
  doesn't skip `DeclareBlockers`/`CombatDamage` even with no attackers.
- `controlled_since_turn` is never updated past object creation — no "gain control" effect exists
  in this engine yet to retrofit it.

## Phase 5: phase5/sba-basics slice

Branch `phase5/sba-basics`, off master. Slice scope: baseline CR 704 state-based actions, narrowed
to exactly three — 704.5a (player at 0 or less life loses), 704.5f (creature toughness 0 or less
dies), 704.5g (creature with lethal marked damage dies). Deliberately *not* combat damage (CR 510)
— that's its own next slice; this one only makes `sba::check_state_based_actions` stop being a
hardcoded no-op, using `damage_marked`/`life` values pre-seeded directly on fixtures (no real
producer exists yet for either, same "fixture stands in for a producer that doesn't exist yet"
pattern `mana_pool`/`controlled_since_turn` used before their real producers landed).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 10 new tests in
`crates/engine/tests/phase5_sba_basics.rs`, all 71 prior tests unaffected. 81 tests total, 0
failed. Confirmed via the real CLI too: `cargo run -p stackfish-cli -- crates/engine/tests/
scenarios/phase5_sba_basics.toml` — 1/1 action ran, 3/3 assertions passed.

**What got implemented:**
- `PTValue::as_fixed` (`card.rs`) — real body, exactly matching its own pre-existing TODO
  (`Fixed(n) -> Some(n)`, `CharacteristicDefining(_) -> None`). Needed directly by the toughness
  check below; filling it was in scope since nothing else could read a creature's toughness as a
  plain number otherwise.
- `sba::check_state_based_actions` — real body, split into two private helpers:
  - `apply_player_loss` (704.5a): scans `state.players` for `!eliminated && life <= 0`, flips
    `eliminated` (field already existed on `PlayerState`, unused until now), emits
    `GameEventKind::PlayerEliminated` (event already existed, unused until now too). Skips
    already-eliminated players so the CR 704.3 fixed-point loop actually terminates instead of
    re-emitting the same elimination every pass.
  - `apply_creature_death` (704.5f/g): scans `state.objects` for battlefield creatures, reads
    toughness via the new `PTValue::as_fixed` (skipping — not guessing — anything with
    `CharacteristicDefining` toughness, since resolving a CDA needs the script bridge), flags
    `toughness <= 0` or `damage_marked >= toughness` as dying. Moves each via the exact same
    new-`ObjectId`/`ObjectLineage`/`ZoneChange` convention `PlayLand`/`CastSpell`-resolution
    already established (CR 400.7) — first SBA-driven zone move in the codebase, deliberately
    reusing the convention rather than inventing a second one.
  - Both helpers build `GameEvent` directly via struct literal (`caused_by_command`/
    `parent_event: None`) rather than `GameEvent::new` — matches `CommitEvents`'s own existing
    construction style, and SBA events have no causing command to thread through anyway.

**STRUCT DEVIATIONS:** None — `PlayerState.eliminated`, `GameObject.damage_marked`,
`CardDefinition.toughness` were all already in place from earlier phases, just unused until this
slice gave them a real reader/writer.

**INTERFACE DEVIATIONS:** None — `check_state_based_actions`'s signature (`&mut GameState ->
Vec<GameEvent>`) was already fixed by the engine-pipeline wiring (`SbaLoop` already calls it and
extends the ledger with the result directly); this slice filled the body to match, didn't change
the shape.

**SCOPE DEVIATIONS (not in the original TODO list, needed anyway):**
- `GameEventKind`/`GameEvent` (`event.rs`) gained `#[derive(Debug)]` — needed for
  `assert_eq!`/panic-message formatting in the new tests, same "needed for test assertions" reason
  every prior slice's `Debug` derives were added (`StepStatus`, `PendingTrigger`, the id newtypes).
- `crates/scenario/src/lib.rs`: `SetupEntry.damage_marked: u32` (default 0, mirrors
  `controlled_since_turn`'s "fixture pre-seeds what no real producer can make yet" shape) and
  `AssertionSpec::Eliminated { player, equals }` (mirrors `Tapped`'s shape) — needed so the TOML
  scenario could exercise both new SBAs without dropping to raw Rust. `docs/scenario_format.md`
  updated to match (new `damage_marked` setup field, new `eliminated` assertion).

**UNRESOLVED (flagged with explicit `TODO` comments in `sba.rs`, not just here):**
- 704.5b (empty-library draw attempt), 704.5c (10+ poison) — no tracking machinery exists for
  either (a failed-draw flag, poison-counter increments).
- 704.5h (deathtouch lethal regardless of amount) — deathtouch isn't modeled, no keyword-ability
  system exists yet.
- CDA toughness (`PTValue::CharacteristicDefining`) and continuous-effect P/T modification (CR 613
  layer 7c) — both still blocked on `layers.rs::effective_characteristics` being full `todo!()`;
  `apply_creature_death` reads the printed `CardDefinition.toughness` directly and skips anything
  it can't resolve as a fixed number, rather than guessing or invoking the still-panicking layer
  engine.
- Regeneration (CR 701.16) — not modeled, 704.5g always destroys outright, nothing can replace it.
- Legend rule (704.5j), world rule (704.5k), planeswalker loyalty 0 (704.5i — not v1 scope per
  DESIGN.md), commander damage ≥ 21 (`PlayerState::commander_damage_from` already exposed for this
  exact check, just not wired into `check_state_based_actions` yet), and the rest of CR 704.5 — no
  prerequisite subsystem exists yet for any of them.
- What happens *after* a player is eliminated (removal from `turn_order`, game-over detection when
  only one player remains) is untouched — this slice only flips the flag and emits the event, same
  boundary the original TODO already drew ("PlayerEliminated exists so a future trigger-collection
  pass has something to match against" — ditto here for whatever ends the game).

## Phase 5: phase5/combat-damage slice

Branch `phase5/combat-damage`, off master. Slice scope: combat damage (CR 510) — assign and deal,
collapsed into one `PlayerActionKind::AssignCombatDamage` (510.2 explicitly forbids a priority
window between assign and deal, so there's no reason to split them into two actions). Now that
`check_state_based_actions` does real work (the prior slice), a lethally-damaged creature actually
dies in the same `step()` call that dealt the damage.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 10 new tests in
`crates/engine/tests/phase5_combat_damage.rs`, all 81 prior tests unaffected. 91 tests total, 0
failed. Confirmed via the real CLI too: `cargo run -p stackfish-cli -- crates/engine/tests/
scenarios/phase5_combat_damage.toml` — 19/19 actions ran, 5/5 assertions passed (attack, double
block, explicit split, lethal/non-lethal outcomes, defending player untouched).

**Design finding that shaped the whole slice (confirmed before writing code, not after):**
re-examined CR 510.1 against this engine's existing block model — `CombatState.blocks: Vec<(ObjectId,
ObjectId)>` is one blocker/one attacker, no banding (already locked in by the declare-blockers
slice). Consequence: the defending player never has a real combat-damage choice under this model.
510.1c (an attacker splitting its damage across 2+ blockers) is the *only* real choice anywhere in
this step — 0 blockers sends everything to the original target, exactly 1 blocker sends everything
there, and 510.1d (a blocker splitting across what it blocks) is never ambiguous since a blocker can
only ever block one attacker. So `AssignCombatDamage { assignments: Vec<(ObjectId, Vec<(ObjectId,
u32)>)> }` is submitted by the active player only, covering *only* attackers with 2+ blockers — the
engine computes every other attacker's and every blocker's damage automatically. One action, not two
(per-attacker was the alternative considered and rejected — this is simpler and matches
`DeclareAttackers`/`DeclareBlockers`'s own "validate the whole batch atomically" precedent).

**What got implemented:**
- `command.rs`: new `PlayerActionKind::AssignCombatDamage` variant, doc comment explains the
  one-action-not-two reasoning above.
- `player.rs`: `PlayerState::lose_life` — real body (`self.life -= amount`), exactly matching its
  own pre-existing TODO. Not clamped to 0 - CR 119 doesn't special-case negative life, and 0-or-less
  is what the 704.5a SBA already checks for.
- `commands.rs`'s `execute_command`: new `AssignCombatDamage` arm. Validates (in order): actor is
  active player, correct phase/step, combat declared. Computes `blockers_by_attacker` from
  `combat.blocks`, derives the multi-blocked subset (2+ blockers), and requires `assignments` to
  cover *exactly* that subset (510.1e atomicity — no extra entries for a 0/1-blocker attacker, no
  missing entry for one that needs a split). For each submitted split: every blocker named is
  actually blocking that attacker, no blocker repeated, every one of the attacker's blockers is
  named exactly once, amounts sum to exactly the attacker's power. Only after every multi-blocked
  attacker's split validates does it build events and mutate state — an illegal batch marks nothing
  (same atomicity discipline `DeclareAttackers` already established). CDA power
  (`PTValue::CharacteristicDefining`) is skipped, not guessed, same stance `sba.rs`'s toughness
  check already takes. Damage to a player target calls the now-real `lose_life`; damage to an
  object target marks `damage_marked` only if the target is a creature (planeswalker/battle damage
  is a structurally-allowed `Target::Object` that does nothing yet — see UNRESOLVED).
- `engine.rs`'s `submit_action`: `AssignCombatDamage` added to the same priority-check exemption
  `DeclareAttackers`/`DeclareBlockers` already have (CR 510.1 is a turn-based action, not a priority
  decision either) — renamed the local `is_combat_declaration` flag to `is_combat_turn_based_action`
  since it now covers a non-declaration action too.
- `engine.rs`'s `advance_turn_step`: the `(Phase::Ending, Step::End) => { step = Cleanup }` arm now
  zeroes `damage_marked` on every battlefield object (CR 514.2) — this slice is the first real
  `damage_marked` producer in the codebase, so without this fix marked damage would persist across
  turns and falsely re-trigger the lethal-damage SBA the following turn. Placed on entering Cleanup
  (not leaving it) since 514.2 is itself one of cleanup's turn-based actions, not a next-turn reset
  (contrast `lands_played_this_turn.clear()`, which correctly lives on the Cleanup->next-Untap
  transition since CR 305.2's count resets with the *turn*, not within cleanup itself).

**STRUCT DEVIATIONS:** None — `GameObject.damage_marked`, `PlayerState.life`,
`GameEventKind::DamageDealt` were all already in place from earlier phases, just unused/stubbed
until this slice gave them a real producer/consumer.

**INTERFACE DEVIATIONS:** None beyond `PlayerActionKind::AssignCombatDamage` itself, which the
pre-slice design finding already anticipated exactly as implemented.

**SCOPE DEVIATIONS:**
- `crates/scenario/src/lib.rs`: `ActionSpec::AssignCombatDamage { player, assignments }` (new
  `DamageAssignmentSpec`/`DamageSplitSpec` structs, mirroring `AttackerSpec`/`BlockSpec`'s shape) —
  needed so a TOML scenario could exercise the double-block-with-a-split case end-to-end.
  `docs/scenario_format.md` updated to match (new action's worked example + semantics, referencing
  the new TOML fixture).

**UNRESOLVED (flagged with explicit `TODO` comments in `commands.rs`, not just here — all called
out in the pre-slice scoping, none new):**
- First/double strike (CR 702.7) — no extra early-damage step, no keyword-ability system to gate it.
- Deathtouch (CR 702.2) — not modeled; `apply_creature_death` (sba.rs) is still plain-lethal-only.
- CDA power — skipped (not guessed) same as CDA toughness already is, blocked on the same
  `layers.rs::effective_characteristics` gap.
- Continuous-effect power pump (CR 613 layer 7c) — printed base power only, `layers.rs` still
  mostly `todo!()`.
- Damage prevention/replacement (CR 615/616) — `replacement.rs` is still mostly `todo!()`, damage
  applies unconditionally.
- Planeswalker/battle as a damage target — `Target`/`AssignCombatDamage` already allow it
  structurally (an unblocked attacker can target one), but nothing converts that damage into
  loyalty/defense loss — only player and creature targets are functional this slice.
- Multiplayer (an attacker facing more than one defending player) — 2-player only, same restriction
  every combat slice so far has carried.

## Phase 5: phase5/beginning-phase slice (merged to master)

Branch `phase5/beginning-phase`, off master. Slice scope: CR 502-504, the Beginning phase (Untap,
Upkeep, Draw). Untap gets two real pieces: the turn-based action itself (502.3 — untap the active
player's tapped battlefield permanents) and a real pipeline fix (502.4 — no priority window opens
during untap). Upkeep needed no code change, just confirming tests (already correct generically as
a bare priority window). Draw gets one real piece: the turn-based action (504.1 — draw one card,
same CR 400.7 new-`ObjectId` convention `PlayLand`/`CastSpell`-resolution/SBA-death already use),
plus CR 103.8a (2-player starting player skips their first draw).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 5 new tests in
`crates/engine/tests/phase5_beginning_phase.rs`, all prior suites unaffected (after the pass-count
fixture edits below). Confirmed via the real CLI too: `cargo run -p stackfish-cli -- crates/engine/
tests/scenarios/phase5_beginning_phase.toml` — all actions ran, 5/5 assertions passed (a real
attack tapping a creature on turn 1, full pass-chain through Cleanup into turn 2, active-player
rotation, and a real draw off the new active player's library).

**What got implemented:**
- `event.rs`: new `GameEventKind::Untapped { object: ObjectId }`, mirroring `Tapped`'s shape — first
  real untap producer.
- `engine.rs`'s `advance_turn_step`: fixed a latent bug surfaced by this slice rather than created by
  it — `PhaseStarted` was being re-emitted on every step transition, even within the same phase
  (Untap→Upkeep, both `Beginning`). Now captures `phase_before` and only pushes `PhaseStarted` when
  `state.phase` actually changed.
- `engine.rs`'s `Engine`: new private `untap_step`/`draw_step` methods (same convention as
  `advance_turn_step` — return `Vec<GameEventKind>` for the caller to log) and a `log_events` helper
  factored out of the `AdvanceTurnStep` arm's existing wrap-and-push loop so it isn't tripled across
  the base transition, untap, and draw event lists.
  - `untap_step`: untaps every battlefield permanent the active player controls. Collects matching
    ids then sorts by `ObjectId` before mutating/emitting — `state.objects` is a `HashMap`, so
    iteration order is otherwise nondeterministic and would make the ledger's `Untapped` order
    flaky.
  - `draw_step`: CR 103.8a check first (`turn_order.len() == 2 && turn_number == 1` — turn 1 always
    belongs to the starting player since `active_player` only rotates at Cleanup, so this single
    condition correctly scopes "starting player's first turn, 2-player only"). Peeks library top via
    `.contents.front()` directly on the zone rather than the still-`todo!()` `Zone::top()` stub —
    confirmed nothing in the codebase calls that stub, every existing site reaches into `.contents`
    directly. Empty library is a silent no-op (CR 704.5b empty-library-loss isn't modeled — that's
    the SBA loop's job, not this turn-based action's). Otherwise mirrors `PlayLand`'s exact CR 400.7
    pattern: snapshot before mutating, mint a new `ObjectId`, move library→hand, emit `ZoneChange`.
  - `AdvanceTurnStep` arm: on landing on `Step::Untap`, runs `untap_step` then loops straight back
    into `ProcessState::AdvanceTurnStep` and returns `Progressed` **without** opening
    `PriorityWindow` (CR 502.4) — the next `step()` call re-enters the same arm, runs
    `advance_turn_step()` again (Untap→Upkeep), and opens priority normally there. On landing on
    `Step::Draw`, runs `draw_step` then falls through to the existing unconditional
    priority-opening code (504.2 — priority still opens after the draw).

**STRUCT DEVIATIONS:** None — every field `untap_step`/`draw_step` touch (`tapped`, `controller`,
`zone`, `lineage`, `timestamp`) already existed from earlier phases.

**INTERFACE DEVIATIONS:** None beyond the two new private `Engine` methods and the `log_events`
refactor, both internal to `engine.rs` — no public signature changed shape.

**SCOPE DEVIATIONS (anticipated during scoping, not a surprise mid-slice):**
- Three existing scenario fixtures had their leading `pass_priority` counts reduced by one
  step-transition's worth, since Untap no longer pauses for its own priority window:
  `phase5_cast_spell_payment.toml` (4→3), `phase5_combat_declarations.toml` and
  `phase5_combat_damage.toml` (12→10 leading passes each).
- `phase5_pass_priority_loop.rs::all_pass_with_empty_stack_advances_turn_step` updated — the same
  pair of passes that used to land on `Step::Untap` now auto-chains through into `Step::Upkeep`
  (ledger gained one more `StepStarted{Upkeep}` entry, `5`→`6`).

**UNRESOLVED (flagged with explicit `TODO`/doc comments in the code, not just here):**
- 502.1 (phasing) — `GameObject.phased_out` stays unused, no phasing schedule modeled.
- 502.2 (day/night) — no day/night state on `GameState`.
- 503.1a (upkeep triggers onto the stack) — same script-bridge gap as every other trigger TODO.
- CR 704.5b (empty-library loss) — draw-from-empty is `draw_step`'s silent no-op, not a loss; that
  SBA isn't modeled (same gap `phase5/sba-basics` already flagged).
- 103.8b/c (Two-Headed Giant / multiplayer draw-skip variants) — 2-player only, same restriction
  every combat slice so far has carried.
- "Doesn't untap" effects (e.g. a continuous effect locking a permanent tapped) — `untap_step` is
  unconditional; no `continuous_effects` query API exists yet (same gap `is_sorcery_speed` already
  flags for `PlayLand`).

## Phase 5: phase5/ending-phase slice (merged to master)

Branch `phase5/ending-phase`, off master. Slice scope: CR 511-514, the Ending Phase (End of
Combat, End Step, Cleanup Step). End of Combat and End Step are bare priority windows — no new
engine code required. Cleanup gets two real pieces: the discard-to-hand-size turn-based action
(CR 514.1) and the "until end of turn" continuous effect expiry (CR 514.2). CR 514.3's
no-priority rule was also implemented: cleanup now auto-advances without opening a priority window
unless SBAs fire during the post-discard SBA check.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — all 9 new tests in
`crates/engine/tests/phase5_ending_phase.rs` green, all prior suites unaffected (after the
`phase5_combat_damage.rs` fix below). Confirmed via CLI too: `cargo run -p stackfish-cli --
crates/engine/tests/scenarios/phase5_ending_phase.toml` — 22 actions ran, 3/3 assertions passed
(zone-change for both discarded cards, hand-size == 7).

**What got implemented:**

- `zone.rs`: `Zone::len()` and `Zone::contains()` — both were `todo!()`, needed for hand-size
  checking and `submit_discard`'s "card in hand" validation.

- `engine.rs` — new types:
  - `DiscardError`: `NotAwaitingDiscard`, `WrongCount { expected, got }`, `DuplicateCard`,
    `CardNotInHand`.
  - `StepStatus::AwaitingDiscard { player, hand_size, max_hand_size }` — surfaces the pause to
    callers (mirrors `AwaitingPayment`).
  - `ProcessState::AwaitingDiscard { player, excess }` — engine state while waiting for
    `submit_discard`.
  - `ProcessState::CleanupSbaCheck` — post-discard/expiry gate that decides whether to open
    priority (SBAs fired) or rotate the turn silently (normal case).

- `engine.rs` — new methods:
  - `Engine::submit_discard(Vec<ObjectId>)`: validates exact count, no duplicates, all ids in
    active player's hand; moves each card hand→graveyard per CR 400.7 (mints new `ObjectId`,
    emits `ZoneChange` event with `last_known_info`); sets `CleanupSbaCheck`. Same "own entry
    point" design as `submit_payment`.
  - `Engine::begin_cleanup_step()` (private): called from `AdvanceTurnStep` when `step ==
    Cleanup`. (1) Drains `continuous_effects` with `EffectDuration::UntilEndOfTurn` (CR 514.2).
    (2) Checks active player hand size — if excess, pauses on `AwaitingDiscard`; if not, jumps
    straight to `CleanupSbaCheck`. Damage clearing (`damage_marked = 0`) was already handled by
    `advance_turn_step`'s `(Ending, End) → (Ending, Cleanup)` arm, not moved.

- `engine.rs` — `AdvanceTurnStep` arm: added `if self.state.step == Step::Cleanup { return
  self.begin_cleanup_step(); }` before the generic priority-open fallthrough. `AwaitingDiscard`
  arm is idempotent (re-reads hand size, re-surfaces the pause without re-advancing state).
  `CleanupSbaCheck` arm: runs one `sba::check_state_based_actions` pass; if events fire, opens
  priority; if not, sets `AdvanceTurnStep` to rotate the turn silently.

- `scenario/src/lib.rs`:
  - `ActionSpec::SubmitDiscard { player, cards: Vec<String> }` — routes through new `run_discard`
    helper (same panic-catch pattern as `run_payment`).
  - `AssertionSpec::HandSize { player, equals }` — checks hand zone `contents.len()`.
  - `drain()` updated to pause on `StepStatus::AwaitingDiscard` (same as `AwaitingPayment`).

- `docs/scenario_format.md`: documented `submit_discard` action and `hand_size`/`assert_hand_size`
  assertion.

**STRUCT DEVIATIONS:** None.

**INTERFACE DEVIATIONS:** `Zone` gained two new public methods (`len`, `contains`). `Engine`
gained `submit_discard` as a new public entry point (alongside `submit_action`, `submit_payment`).
`StepStatus` gained `AwaitingDiscard` variant — callers that `match` exhaustively need a new arm.
`DiscardError` is new public type in `engine` module.

**SCOPE DEVIATIONS:**

- `phase5_combat_damage.rs::damage_marked_is_cleared_entering_cleanup`: this test's old loop
  looked for `engine.state().step == Step::Cleanup` expecting the engine to pause for priority at
  Cleanup — that was incorrect per CR 514.3. With the new implementation, Cleanup auto-advances
  without a priority window (no SBAs in that fixture), so the engine was past Cleanup by the time
  the test checked. Fixed: removed the loop's `step == Cleanup` break condition and step assertion;
  now just submits 2 passes (one per player at End step) and checks `damage_marked == 0` directly.
  The damage IS cleared (by `advance_turn_step`'s transition code, unchanged), the test was just
  checking the wrong architectural invariant.

**UNRESOLVED (flagged with explicit `TODO` in code):**

- `ReplacementEffect` has no `duration` field — UntilEndOfTurn replacement effects can't be
  drained at cleanup yet. Only `continuous_effects` are drained. Marked `TODO` in
  `begin_cleanup_step`.
- CR 514.3 full loop: if SBAs fire and priority opens during cleanup, and then triggers resolve,
  a *second* cleanup should begin (not just return to Upkeep). `CleanupSbaCheck` currently just
  opens `PriorityWindow` and doesn't re-enter `begin_cleanup_step` after it resolves. Marked TODO.
- Max hand size hardcoded to `7` — CR 402.2 continuous effects (e.g. "you have no maximum hand
  size") aren't queried. Marked TODO.
- End of Combat step (CR 511): any "until end of combat" effects would need their own expiry pass
  here. Not modeled — no such duration exists in `EffectDuration` yet.

## Phase 5: phase5/script-bridge-trigger slice

Branch `phase5/script-bridge-trigger`, off master. Slice scope: a deliberately narrow first cut of
the script bridge (Phase 2 part 3's `script.rs`/`script_api.rs` were signatures-only until now) —
one concrete triggered ability (CR 603), real Lua condition + operation callbacks, invoked for real
via `ScriptRuntime`, powering an ETB "gain 1 life" trigger end to end. Registration itself stays
Rust-side this slice: a `RegisteredAbility` is built directly via a struct literal (test/scenario
fixtures call the new `Engine::load_script` to get real `ScriptHandle`s first), not via a card
script self-registering through `Game.RegisterTriggeredAbility` — that still needs the `Game.*` Lua
table bound into the VM plus a registration-time mutability question sorted out (a Lua callback
reaching `&mut GameState` mid-script), both flagged as explicit follow-up work rather than solved
here. The keyword-primitives slice (merged just before this one, not yet written up in this file)
laid the groundwork this builds on — `state.abilities`/`triggers.rs`'s plumbing already existed from
the pass-priority-loop and activate-ability-mana slices.

**Design lock-in, decided before writing code (see chat discussion, not just this doc):** the
operation callback never mutates `GameState` itself — it returns a plain Lua table describing
*intent* (`{ gain_life = 1 }`), and `ScriptRuntime::invoke_operation` decodes that table and applies
the mutation on the Rust side. This is "Lua asks, Rust decides" (already documented as the design
philosophy in `script_api.rs`'s header) applied to operation resolution specifically — it sidesteps
needing a Lua callback to call back into `&mut GameState` mid-script, which `invoke_operation`'s own
TODO had flagged as an unsorted mutability question since Phase 2 part 3. That harder problem (a
script calling into `Game.*` itself, needing interior mutability during registration/resolution) is
deferred to the follow-up slice that adds `Game.*` self-registration.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 7 new tests in
`crates/engine/tests/phase5_script_bridge_trigger.rs` (unit-level `collect_triggers` condition
true/false/none-always cases against a real `ScriptRuntime`, plus two full-pipeline end-to-end
tests), all 151 prior tests unaffected. 158 tests total, 0 failed. Confirmed via the real CLI too:
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_script_bridge_trigger.toml` —
7/7 actions ran, 3/3 assertions passed (life 20→21, bear resolved to the battlefield, ledger_len 19).

**What got implemented:**
- `script.rs`: `ScriptRuntime` gains real storage (`HashMap<ScriptHandle, mlua::RegistryKey>` +
  a monotonic handle counter). `new()` constructs a real `Lua::new()` (the `Game.*` global table
  binding is explicitly deferred — noted as a TODO on `new()` itself, not silently skipped). New
  `load_function(&mut self, source: &str) -> Result<ScriptHandle, ScriptError>` — loads a standalone
  Lua chunk that must evaluate to a function value, stores it via `create_registry_value`, mints a
  handle. Narrower than `load_card_script` (still `todo!()` — that one needs a script to
  self-register via `Game.*`, blocked on the same gap). `invoke_condition` real body: builds a
  minimal Lua table from the event (`event_to_lua`, see below), calls the loaded function with
  `(source, controller, event)`, returns the bool result. `invoke_operation` signature changed from
  `&mut ScriptContext` to `(&self, handle, state: &mut GameState, source, controller)` — calls the
  function with `(source, controller)`, decodes only a `gain_life` key from the returned table this
  slice, applies via `PlayerState::gain_life`. New private `event_to_lua`/`zone_kind_name` helpers —
  only `GameEventKind::ZoneChange` bridges into a Lua-visible table (`kind`/`object`/`from`/`to`);
  every other event kind arrives as `nil`.
- `types/player.rs`: `PlayerState::gain_life` real body (`self.life += amount`) — trivial fill,
  blocked the demo operation otherwise.
- `triggers.rs`: `collect_triggers` gains a `&ScriptRuntime` param, real body. `condition: None`
  matches unconditionally (documented risk, not gated further this slice); `Some(handle)` invokes it
  for real via `invoke_condition`, treating a script error as "didn't match" (TODO: no path exists
  yet to surface a script error back to a `StepStatus`). Tracks `ability_index` as "the nth ability
  on this source, any kind, in registration order" — matching the exact convention
  `commands::find_ability` (new, see below) uses for `ActivateAbility`, via a per-source counter
  walked in `state.abilities`'s registration order. A `Triggered` ability whose source no longer
  resolves via `state.object()` (e.g. it left the battlefield) is skipped — the CR 603.10
  last-known-information gap this leaves is called out explicitly, not fixed here (only ETB-shaped
  conditions are exercised).
- `commands.rs`: new `pub(crate) find_ability(state, source, ability_index) -> Option<&RegisteredAbility>`
  — extracted from `ActivateAbility`'s previously-inline lookup so `triggers.rs`'s
  `PutTriggersApnap` stack-push (below) and `ActivateAbility` share one definition of "index" rather
  than each re-deriving it.
- `engine.rs`: `Engine` gains a private `script_runtime: ScriptRuntime` field (constructed in both
  `Engine::new` and `Engine::new_game`) and a new public `load_script(&mut self, source: &str) ->
  Result<ScriptHandle, ScriptError>` — the same "no producer exists yet, caller wires it up by hand"
  shape `state_mut()`'s own doc comment already described for `state.abilities`. `CollectTriggers`
  now passes `&self.script_runtime` through to `triggers::collect_triggers`. `PutTriggersApnap`'s
  empty-`pending_groups` branch — previously a bare TODO — now pushes a real `StackObject` (kind
  `TriggeredAbility`) for each `PendingTrigger` in `ordered`, looked up via `commands::find_ability`,
  logging a `StackObjectPushed` ledger event per push (same direct-ledger-write pattern
  `submit_payment`/`submit_discard` already use for engine-internal writes outside the normal
  `ReplacementCheck` pipeline). `ResolveStackTop`'s `StackEntryKind::TriggeredAbility` case now calls
  `self.script_runtime.invoke_operation(...)` — the `Spell`-only zone-move branch is untouched;
  `ActivatedAbility`/`Copy` remain no-ops exactly as before.
- `crates/scenario/src/lib.rs`: `CardSpec` gains `trigger_condition`/`trigger_operation` (raw Lua
  source strings) — `trigger_operation` alone registers the ability, `trigger_condition` omitted
  means "always triggers." New `PendingTriggerScript { source, condition, operation }` and a shared
  `register_trigger_ability` helper (loads both scripts via `Engine::load_script`, pushes the
  `RegisteredAbility`) used by both scenario-build paths.
- `docs/scenario_format.md`: documented the two new `CardSpec` fields with a worked ETB example,
  noting the deliberate narrowness (no self-registration, no targeting, no continuous effects).

**STRUCT DEVIATIONS:** None beyond what was scoped (`ScriptRuntime.scripts`/`next_handle`,
`Engine.script_runtime`) — anticipated in the pre-slice requirements report, not a surprise.

**INTERFACE DEVIATIONS:**
- `crates/scenario::build_initial_state` now returns a 3-tuple (`(GameState, HashMap<String,
  ObjectId>, Vec<PendingTriggerScript>)`) instead of a 2-tuple — not anticipated in the original
  report. Needed because a `RegisteredAbility`'s `condition`/`operation` need *real* `ScriptHandle`s,
  which only exist once an `Engine` (and its `ScriptRuntime`) is constructed — but
  `build_initial_state` only builds a `GameState`, before any `Engine` exists. `run()`'s
  `build_initial_state` branch now loads the pending scripts and registers the abilities right after
  `Engine::new(state)`, mirroring the pattern `build_new_game_engine`'s own post-construction
  `mana_ability` loop already established.
- `invoke_operation`'s signature change (`&mut ScriptContext` → `state: &mut GameState` directly)
  was flagged as likely in Phase 2 part 3's own notes, not a surprise mid-slice.

**SCOPE DEVIATIONS:** None beyond the scenario-crate plumbing already scoped in the pre-slice
requirements report.

**UNRESOLVED (all flagged with explicit `TODO` comments in code, all anticipated in the pre-slice
requirements report — none new):**
- `Game.*` Lua global table binding and card-script self-registration (`Game.RegisterTriggeredAbility`
  actually being called *from* Lua) — `script_api.rs`'s functions still aren't bound into the VM.
  This is the next planned slice, along with a `Game.Conditions.*`/`Game.Ops.*` primitive-tag fast
  path (discussed in chat, not yet built) so common shapes like "ETB" don't need a full condition
  script written out, while a hand-written condition/operation function remains fully supported as
  the general case.
- `ScriptContext.state`'s hidden-information leak (full `&GameState` visible to a condition script)
  — untouched, same DESIGN.md open question as before this slice.
- `invoke_target`'s synchronous signature / Lua coroutine design for player-input-mid-script —
  untouched.
- Only `GameEventKind::ZoneChange` bridges into a Lua-visible event shape; every other event kind
  reaches a condition script as `nil`. Extend `event_to_lua` as real conditions need more.
- Only a `gain_life` key is decoded out of an operation's returned table; other effect shapes
  (draw, damage, counters, ...) need their own key + apply arm in `invoke_operation`.
- CR 603.10/603.3d: a triggered ability whose source has left the battlefield (or otherwise no
  longer resolves via `state.object()`) is silently dropped rather than evaluated/placed using
  last-known information — both `collect_triggers` and `PutTriggersApnap`'s stack-push flag this
  explicitly. Only ETB-shaped (source-still-present) triggers are exercised this slice.
- CR 603.2-603.3's once-per-turn/range/timing legality beyond the condition callback itself — not
  modeled, same gap flagged since the pass-priority-loop slice's original `collect_triggers` TODO.

## Phase 5: phase5/script-bridge-primitives slice

Branch `phase5/script-bridge-primitives`, off master. Slice scope: `Game.Conditions.*`/
`Game.Ops.*` as real Lua values, so a condition/operation source string can be a one-liner
(`return Game.Conditions.Enters`, `return Game.Ops.GainLife(1)`) instead of a full
hand-written function — resolved once at *load* time into a Rust-native fast path with zero
further Lua VM calls at trigger-check/resolve time. A full custom function remains fully
supported (unchanged fallback). Full `Game.RegisterTriggeredAbility` self-registration is
explicitly still out of scope — registration stays Rust-side (test/scenario fixtures build a
`RegisteredAbility` directly), same as the prior slice.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 10 new tests in
`crates/engine/tests/phase5_script_bridge_primitives.rs`, all 158 prior tests unaffected. 168
tests total, 0 failed. Confirmed via the real CLI too: `cargo run -p stackfish-cli --
crates/engine/tests/scenarios/phase5_script_bridge_primitives.toml` — 7/7 actions ran, 3/3
assertions passed (life 20→21, bear resolved to the battlefield, ledger_len 19) — same
ledger shape as the prior slice's hand-written-script scenario, confirming the primitive path
produces identical observable behavior.

**What got implemented:**
- `types/ability.rs`: `AbilityEffect::GainLife(i32)` — the `Game.Ops.GainLife(n)` primitive.
  `ConditionPrimitive { EntersBattlefield, Always }` — a Rust-native `matches(source, &GameEventKind)
  -> bool`, mirroring the hand-written Lua condition's exact semantics (`event.object == source
  && event.to == "battlefield"` for `EntersBattlefield`; unconditional for `Always`).
  `ConditionSpec { Primitive(ConditionPrimitive), Script(ScriptHandle) }` replaces
  `RegisteredAbility::condition`'s old `Option<ScriptHandle>` — now `Option<ConditionSpec>`.
  `Always` is a real variant (not just reusing `None`) so a Lua author has an explicit, correct
  spelling for "no filter" — loading rejects a bare Lua `nil` result as an error instead of
  silently treating a typo'd `Game.Conditions.Typo` as unconditional (closes the footgun flagged
  in the pre-slice design chat).
- `types/stack.rs`: `StackObject` gains `effect: Option<AbilityEffect>`, baked in at push time
  (`PutTriggersApnap`) from the triggering `RegisteredAbility`, same "primitive stand-in when
  Some, script fallback when None" contract `RegisteredAbility::effect` already had for CR 605
  mana abilities. `StackObject::new` gained a matching trailing parameter — both existing call
  sites (`commands.rs`'s `CastSpell`, `engine.rs`'s `PutTriggersApnap`) updated.
- `script.rs`: two small Lua sugar tables built into globals at `ScriptRuntime::new()` —
  `Game.Conditions = { Enters = <ConditionTag>, Always = <ConditionTag> }`, `Game.Ops = {
  GainLife = <function(amount) -> OperationTag> }`. `ConditionTag`/`OperationTag` are thin
  `impl mlua::UserData` wrappers around `ConditionPrimitive`/`AbilityEffect` respectively — this
  is *not* the same as binding `script_api.rs`'s functions (`Game.RegisterTriggeredAbility` etc
  stay unbound). New `ScriptRuntime::load_condition`/`load_operation`: each evaluates the chunk
  to an `mlua::Value` and branches — `UserData` decodes the tag into the primitive spec/effect,
  `Function` registers via the same registry-key path `load_function` already used (factored the
  handle-minting into a shared `next_script_handle` helper), `Nil`/anything else is a load error.
  `Engine` gained matching `load_condition`/`load_operation` wrappers alongside `load_script`.
- `triggers.rs`'s `collect_triggers`: matches on `ConditionSpec` now — `Primitive` calls
  `ConditionPrimitive::matches` directly (zero Lua calls), `Script` still calls
  `invoke_condition` exactly as before.
- `engine.rs`'s `ResolveStackTop`: the `TriggeredAbility` branch now checks `top.effect` first —
  `Some(GainLife(n))` applies directly via `PlayerState::gain_life`, falling back to
  `invoke_operation` only when `effect` is `None`. `Some(AddMana(_))` is a defensive no-op (that
  variant is only ever set on an `Activated` mana ability, which never uses the stack per CR
  605.1a — a `TriggeredAbility` `StackObject` should never actually carry it).
- `commands.rs`'s `ActivateAbility` arm: its exhaustive `match effect` needed a new arm for the
  added `AbilityEffect::GainLife` variant — folded into the same `todo!()` catch as `None`, since
  a non-mana activated ability (script-invoked *or* a recognized primitive) both need the
  not-yet-built "`ActivateAbility` pushes a real `StackObject`" machinery before either effect
  shape can resolve; out of scope for this slice regardless of which effect is waiting behind it.
- `crates/scenario/src/lib.rs`'s `register_trigger_ability` helper: now calls
  `Engine::load_condition`/`load_operation` instead of `load_script` twice, threading the decoded
  `effect` through onto the pushed `RegisteredAbility` — supports both source-string shapes
  transparently for TOML fixtures.

**STRUCT DEVIATIONS:** None beyond what the pre-slice requirements report anticipated exactly
(`ConditionPrimitive`, `ConditionSpec`, `AbilityEffect::GainLife`, `StackObject::effect`).

**INTERFACE DEVIATIONS:** None beyond the two new `Engine`/`ScriptRuntime` method pairs
(`load_condition`/`load_operation`) already scoped in the requirements report.

**SCOPE DEVIATIONS:**
- A genuine Rust borrow-checker wrinkle, not anticipated: `mlua::Value`'s `Table`/`Function`/
  `UserData` variants hold an actual `&'lua Lua` reference internally (not just a phantom
  lifetime), and Rust's drop elaboration for a `let value = ...; match value { ... }` pattern
  defers `value`'s (mostly-no-op, since fully moved-out) destructor call to `value`'s *lexical*
  scope end rather than immediately after the match — which pinned the borrow of `self.lua` all
  the way to the end of `load_condition`/`load_operation`, conflicting with the `&mut self` call
  to mint a handle afterward. Fixed by nesting the eval+match in an inner block that ends before
  any `&mut self` call, so `value`'s lexical scope (and the borrow tied to it) closes first.
- Four pre-existing test files needed mechanical updates for the two type/shape changes (not new
  gaps, just call sites the type system caught): `phase5_script_bridge_trigger.rs`'s five
  `condition: Some(condition)` sites now read `condition: Some(ConditionSpec::Script(condition))`;
  `phase5_cast_spell.rs`/`phase5_play_land.rs`/`phase5_pass_priority_loop.rs`'s direct
  `StackObject { .. }` struct literals each needed `effect: None` added.

**UNRESOLVED (all anticipated in the pre-slice requirements report, none new):**
- `Game.*` self-registration (`Game.RegisterTriggeredAbility` actually callable *from* Lua) —
  still out of scope, blocked on the registration-time mutability question.
- Only `Game.Ops.GainLife` and `Game.Conditions.{Enters,Always}` are wired — `DrawCards`/`Leaves`/
  etc need their own factory/tag entries as real cards need them.
- `ScriptContext.state`'s hidden-information leak, `invoke_target`'s synchronous signature, and
  CR 603.10's last-known-information gap are all untouched, same as before this slice.

## Phase 5: phase5/script-self-registration slice

Branch `phase5/script-self-registration`, off master. Slice scope: real card-script
self-registration — a card's own Lua `initial_effect(c)` calling `Game.CreateTriggeredAbility(c)`
to register its own triggered ability, replacing the "harness builds `RegisteredAbility` by hand"
pattern (`trigger_condition`/`trigger_operation`, still fully supported, unaffected) with the
genuine article for triggered abilities specifically.

**Design lock-ins from the pre-slice discussion (chat, not just this doc):**
- **Mutability fix: queued registration, not shared `GameState` access.** The actual blocker
  since Phase 2 was never "how do you bind a function," it was "a Lua callback can't hold `&mut
  GameState` mid-script." Fixed by giving `ScriptRuntime` a `Rc<RefCell<ScriptRegistry>>` — a
  small side-channel (loaded-script storage plus a `pending_triggered: Vec<RegisteredAbility>`
  queue) that a Lua-bound closure can reach without ever touching `GameState`. `:Register()`
  pushes onto that queue; `ScriptRuntime::run_card_script` drains it into `state.abilities` only
  *after* the whole script call returns. Same "Lua asks, Rust decides" split already proven for
  operation resolution, applied to registration instead.
- **Call shape: a builder object, not a flat function call.** `Game.CreateTriggeredAbility(c)`
  returns a `TriggeredAbilityBuilder` (`mlua::UserData`) with `:SetCondition`/`:SetOperation`/
  `:Register()` methods — deliberately mirroring ygopro's `Effect.CreateEffect(c)`/
  `e1:SetCondition(...)`/`c:RegisterEffect(e1)` convention (`docs/ygo_pro_reference.md`) over a
  single call with a growing pile of positional/nil args. Side effect: this *permanently* resolves
  `register_triggered_ability`'s old "no `AbilityKind` parameter" gap, not just defers it — kind
  is implied by which constructor was called, never a data field. `Game.CreateActivatedAbility`/
  `Game.CreateStaticAbility` are the natural future siblings, not built this slice.
- **Registration timing: unconditional, every mint, only at real battlefield-entry points.**
  Re-runs a card's script at exactly two places — `PlayLand`'s hand→battlefield move and
  `ResolveStackTop`'s permanent-entry branch (stack→battlefield) — every single time, no "first
  time only" bookkeeping, matching CR 400.7's "no memory of its previous existence" literally.
  Cast-triggers/graveyard-sourced abilities (needing a source before the battlefield) stay out of
  scope, same framing every prior script-bridge slice has used for "only ETB-shaped triggers
  exercised."
- **Purge stale abilities everywhere an id retires, not just at the 2 new sites.** A real,
  pre-existing (not new) gap surfaced while scoping this: `commands::remap_abilities` (added
  during `phase5/cast-spell`, carries a `RegisteredAbility.source` forward across a CR 400.7 id
  mint) was already called at 6 of 9 `state.objects.remove(&old_id)` sites — `PlayLand`,
  `CastSpell`, both mulligan bottom/redraw paths, and `ResolveStackTop`'s permanent branch — but
  **not** at discard (hand→graveyard), `draw_step` (library→hand), or SBA-driven death
  (battlefield→graveyard). Those 3 had no remap and no purge — any `RegisteredAbility` registered
  against a card that got discarded, drawn, or died just sat dead in `state.abilities` forever,
  pre-dating self-registration entirely. New `commands::retire_object(state, old_id)` — removes
  the object *and* purges `state.abilities.retain(|a| a.source != old_id)` — replaces the bare
  `.remove()` at exactly those 3 sites. The other 6 sites are deliberately **untouched**: calling
  `retire_object` there too would be wrong, since it would delete what `remap_abilities` just
  correctly carried forward (e.g. a mana ability registered against a land while still in hand
  must survive the hand→battlefield move) — `retire_object`'s own doc comment spells out this
  "remap XOR retire, never both" contract explicitly so it doesn't get miscopied later.
- **`ByKey` stays out of scope, by design, likely permanently.** Confirmed in the pre-slice
  discussion: the real card-database crate is expected to resolve a key to source *before*
  handing this engine a `CardDefinition` — `ScriptReference::ByKey` may end up practically unused
  from this engine's side forever. `run_card_script` returns a real error for it rather than
  guessing at a resolution convention.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 1 new lib unit test
(`commands::tests::retire_object_purges_only_the_retired_source`, `pub(crate)` visibility means
it can't live in an integration test) + 9 new tests in
`crates/engine/tests/phase5_script_self_registration.rs`, all 168 prior tests unaffected. 178
tests total, 0 failed. Confirmed via the real CLI too: `cargo run -p stackfish-cli --
crates/engine/tests/scenarios/phase5_script_self_registration.toml` — 7/7 actions ran, 3/3
assertions passed (life 20→21, bear resolved to the battlefield, ledger_len 19) — same ledger
shape as the harness-wired version, confirming self-registration produces identical observable
behavior.

**What got implemented:**
- `script.rs`: `ScriptRegistry` (new private struct) bundles `scripts`/`next_handle` (moved out of
  `ScriptRuntime` directly) plus the new `pending_triggered` queue, all behind one
  `Rc<RefCell<_>>` `ScriptRuntime` now holds instead of owning those fields itself. `load_function`/
  `load_condition`/`load_operation`/`function` updated to go through `self.registry.borrow_mut()`/
  `.borrow()` instead of direct field mutation — as a side effect, this let the earlier
  nested-block NLL workaround in `load_condition`/`load_operation` (from the primitives slice) be
  removed entirely: registry mutation no longer needs `&mut self` at all (interior mutability via
  the `RefCell`), so there's no more conflict with `self.lua`'s borrow to work around. Decode logic
  for a condition/operation value factored into free functions `decode_condition_value`/
  `decode_operation_value(lua, registry, value)` — shared by `ScriptRuntime::load_condition`/
  `load_operation` (decoding a freshly-`eval`'d chunk) and the new builder's `:SetCondition`/
  `:SetOperation` (decoding a value already in hand as a call argument).
  `TriggeredAbilityBuilder` (new `mlua::UserData`) holds `source`/`condition`/`operation` plus a
  clone of the shared registry `Rc`; `:Register()` builds a real `RegisteredAbility` and pushes it
  onto `registry.pending_triggered`. `bind_game_globals` gained a `registry` parameter and binds
  `Game.CreateTriggeredAbility` as a closure capturing a clone of it. Old `load_card_script`
  (`todo!()`, unused, wrong signature for what was actually needed) replaced by real
  `run_card_script(&mut self, state: &mut GameState, definition: &CardDefinition, source:
  ObjectId) -> Result<(), ScriptError>` — evaluates `Inline` source to a table, calls its
  `initial_effect` field with `source.0`, drains `pending_triggered` into `state.abilities`
  afterward. Re-parses the source fresh every call rather than caching per `CardDefinition`
  (flagged as a deliberate simplification, not a decided design).
- `commands.rs`: new `pub(crate) fn retire_object` (see design lock-in above). `CommandOutcome`
  gained `battlefield_entries: Vec<ObjectId>` — `execute_command` can't reach `ScriptRuntime`
  (`state: &mut GameState` only), so `PlayLand`'s arm reports which id just became a new
  battlefield permanent instead of invoking the script itself; every other arm sets it to
  `Vec::new()`. Same "one place decides, one place performs the side effect" split
  `priority_handoff`/`broke_pass_chain` already established.
- `engine.rs`: `PendingAction`'s branch drains `outcome.battlefield_entries` and calls
  `self.script_runtime.run_card_script(...)` for each id, right after `execute_command` returns
  (before `CollectTriggers` runs later in the same pipeline run). `ResolveStackTop`'s permanent-
  entry branch calls it directly (already owns both `state` and `script_runtime`) — captures
  `new_object.definition.clone()` before `new_object` moves into `state.objects.insert`, calls
  `run_card_script` right after the existing `remap_abilities` call (order: remap first, so any
  ability from an earlier zone-change survives; then self-register fresh — the two don't conflict
  in practice since nothing self-registered exists before a card's first battlefield entry).
- `sba.rs`: death branch now calls `commands::retire_object` instead of a bare `objects.remove`
  (new `use crate::commands;`).
- `crates/scenario/src/lib.rs`: `CardSpec` gained `script: Option<String>` — sets
  `CardDefinition::script_ref` to `ScriptReference::Inline` directly at both construction sites,
  no post-construction `Engine` step needed (unlike `trigger_condition`/`trigger_operation`,
  self-registration runs automatically through the real `PlayLand`/`ResolveStackTop` pipeline once
  the card reaches the battlefield).
- `docs/lua_scripts.md`/`docs/scenario_format.md`: rewritten to describe self-registration as
  real (the prior slice's version was explicitly speculative/illustrative-only — the exact
  `Game.RegisterTriggeredAbility(c, condition, cost, target, operation)` positional shape guessed
  there turned out different from what got built; replaced with the actual builder-based
  `Game.CreateTriggeredAbility` API).

**STRUCT DEVIATIONS:**
- `CommandOutcome::battlefield_entries` — not anticipated by name in the pre-slice report, but the
  underlying need (`execute_command` can't reach `ScriptRuntime`) was flagged as a design question
  before writing code, and this is the direct consequence.
- `ScriptRuntime`'s internal shape changed substantially (`scripts`/`next_handle` moved into a new
  private `ScriptRegistry` behind `Rc<RefCell<_>>`) — anticipated in the pre-slice discussion
  (queued-registration was the chosen mutability fix), not a surprise.

**INTERFACE DEVIATIONS:** `ScriptRuntime::load_card_script` (never implemented, `todo!()`) is
gone, replaced by `run_card_script` with a different signature (needs `&mut GameState` and
`ObjectId`, which the old one didn't take) — anticipated, since the old signature was written
before this slice's actual requirements were known.

**SCOPE DEVIATIONS:**
- `commands::retire_object` touches 3 pre-existing call sites (`engine.rs`'s discard and
  `draw_step`, `sba.rs`'s death) that have nothing to do with self-registration directly — surfaced
  and confirmed with the human before implementing (see design lock-ins above), not discovered
  after the fact.
- `crates/scenario/src/lib.rs` and both `docs/*.md` files touched beyond the engine crate itself —
  same "verification tooling / docs need to track new capability" pattern every prior slice
  affecting the scenario harness has followed.

**UNRESOLVED (anticipated in the pre-slice requirements report, none new):**
- `Game.CreateActivatedAbility`/`Game.CreateStaticAbility` — not built; only `Triggered` can
  self-register.
- `:SetCost`/`:SetTarget` — not bound on the builder; `invoke_cost`/`invoke_target` still `todo!()`
  regardless.
- Self-registration only runs at the 2 "enters the battlefield" call sites — cast-triggers/
  graveyard-sourced abilities can't self-register yet.
- `ScriptReference::ByKey` — stays `todo!()`/an error; expected to stay that way given the real
  database crate's design (see design lock-ins above).
- `run_card_script` re-parses the card's Lua source fresh every call rather than caching the
  compiled chunk per `CardDefinition` — `CardDefinition` has no stable cache key today.
- No guard against `run_card_script` being invoked twice for the same id without an intervening
  zone change (would double-register) — not expected given the two call sites are each single-shot
  per mint, but not defensively checked.

## Slice backlog (ordered by priority toward "script real cards")

Compiled after surveying everything merged so far (roughly: full pipeline, turn loop, combat,
basic SBA, CR 702 keyword bitmask, and a first real script bridge with triggered-ability
self-registration and mana/non-mana activated abilities via the stack). Two foundational
subsystems still sit at zero: the continuous-effect layer engine and targeting — both gate a
large fraction of real card text. Order below reflects that; re-order as slices land and priorities
shift, don't treat it as fixed once written.

1. **Continuous effect / layer engine (CR 613)** — the `phase5/layer-engine-basics` slice
   (docs/implementation.md, above) landed layers 6/7b/7c with `sba.rs` as the only real consumer.
   Turned out to be big enough to split into its own piecemeal sequence rather than one item —
   sub-slices, in order (re-order as they land, same as the rest of this list):
   1. ~~**A: wire the two deferred consumers**~~ — done, `phase5/layer-engine-consumers` slice
      (docs/implementation.md, above). Combat-damage power (`AssignCombatDamage`) and Flying/
      Reach/Menace blocking legality (`DeclareBlockers`) both now consult
      `layers::effective_characteristics` via new `effective_power`/`effective_keywords` helpers in
      `commands.rs` — plus the Lifelink/Wither/Infect/Deathtouch keyword reads in the same function,
      folded in as the same category of gap. Turned out to need no new plumbing at all — the prior
      slice's TODOs claiming an `&ScriptRuntime` wall here were simply wrong (see that slice's own
      "Correction" note).
   2. ~~**B: layers 4/5** (type-changing, color-changing)~~ — done, `phase5/layer-engine-types-colors`
      slice (docs/implementation.md, below). `EffectPrimitive::AddCardType`/`AddColor` (additive
      only, overwrite deferred), applied in `layers.rs` alongside 6/7b/7c. Real consumer:
      `sba::apply_creature_death` now checks the *effective* card type, not `CardDefinition`'s, so
      a land animated into a creature (layer 4's `AddCardType` + layer 7b's `SetPT`) is actually
      subject to CR 704.5g.
   3. ~~**C: layer 7a** (characteristic-defining P/T)~~ — done, `phase5/layer-engine-cda` slice
      (docs/implementation.md, below), CDA resolution only — `AffectedFilter::Script`/
      `EffectPayload::Script` deliberately **not** paired in despite the `&ScriptRuntime` plumbing
      now being available (see that slice's "Correction to the backlog" note); still dead code,
      now its own explicitly-named follow-up (item 3a below).
   3a. ~~**Script-backed continuous effects**~~ — done, `phase5/continuous-effects-script` slice
      (docs/implementation.md, below). `AffectedFilter::Script`/`EffectPayload::Script` are real:
      `layers::applicable_effects_in_layer` evaluates a `Script` affects-predicate
      (`ScriptRuntime::invoke_continuous_affects`), `layers::resolve_primitive` decodes a `Script`
      effect payload (`ScriptRuntime::invoke_continuous_effect`, the "Lua asks, Rust decides" table
      convention `invoke_operation` established, generalized to six recognized keys). `applies_to`
      itself stays `Primitive`-only — the `&ScriptRuntime` param this note originally proposed for
      it instead lives in `layers.rs`, same separation `resolve_base_pt` already keeps for CDA.
   4. ~~**D: layer 7e** (power/toughness switching)~~ — done, `phase5/layer-engine-switching` slice
      (docs/implementation.md, below). `EffectPrimitive::SwitchPT`, applied as its own pass in
      `layers::effective_characteristics` after 7b/7c fully resolve — a `Game.Layers.
      PowerToughnessSwitching`/`Game.Effects.SwitchPT` binding pair, same as every other real layer.
   5. Layers 1/2/3 (copy/control/text-changing) and the CR 613.6-8 dependency system — each needs
      its own real subsystem (copy-object semantics, control-transfer command, text-rewrite
      representation, dependency detection), not just a query extension. Turned out to be four
      more slices, not one - split below (research pass over `layers.rs`/`types/effect.rs`/
      `types/object.rs`/`types/command.rs` before splitting, not guessed at), ordered by what's
      actually self-contained vs. what blocks on something else:
      5a. ~~**Dependency system (CR 613.6-8)**~~ — done, `phase5/layer-engine-dependency` slice
          (docs/implementation.md, below). Correction to this item's own pre-existing framing, found
          while scoping: none of today's primitives can actually produce a CR 613.8a dependency (every
          `EffectPrimitive` is a fixed value or union, and `EffectFilter`/`Script` payloads only read
          immutable base data) - real dependency needed one new "reads the current running value"
          primitive, `EffectPrimitive::DoublePT` (layer 7c, e.g. Berserk), added as part of this
          slice rather than proving the algorithm against synthetic-only fixtures with no card-text
          equivalent.
      5b. ~~**Layer 2 (control-changing effects, CR 613.1b/108.4)**~~ — done, across two slices.
          `phase5/layer-engine-control` (query half, docs/implementation.md, below): real design
          question this item's own text flagged (live query vs. durable mutation) resolved with the
          human in favor of the live query, once reading the code showed the mutation shortcut would
          leave `Layer::ControlChangingEffects` permanently dead and require hand-rolling a
          grant-stack/revert system the layer engine's own CR 613.7/613.8 ordering already solves
          generically. `phase5/layer-engine-control-consumers` (docs/implementation.md, below):
          migrated the real `GameObject.controller` read-sites (combat, targeting, cost filters,
          `has_summoning_sickness`, `EffectFilter`/`FilterSpec`, replacement effects) to consume
          `layers::effective_controller` instead of the raw field - split into its own slice rather
          than bundled with the query, matching how the base layer-engine work itself split "prove
          the query" from "wire the deferred consumers." One real design finding surfaced while
          implementing the consumer migration (not anticipated in either slice's own pre-slice
          report): `layers.rs`'s own internal filtering (`applicable_effects_in_layer`/
          `ContinuousEffect::applies_to`) could NOT be migrated to `effective_controller` - doing so
          would make layer 2's own resolution recursively call itself. Left as raw-field reads,
          deliberately, not silently forgotten. Still doesn't close 11b's `StackObject.controller`-
          as-owner-stand-in gap (`sba.rs`'s `remove_players_objects`) - `StackObject` has no owner
          field at all, unrelated to this migration.
      5c. **Layer 1 (copy effects, CR 613.1a/613.2, CR 707 "Copying Objects")** — nothing exists:
          no copy/clone command, no copiable-values snapshot distinct from `CardDefinition`, no
          "create a token that's a copy of object X" path (`AbilityEffect::CreateToken` only builds
          from its own `TokenSpec` today). **Correction to this item's own pre-existing doc
          comment** (`types/effect.rs`'s `Layer::CopyEffects`): it cites CR 706, which is actually
          the dice-rolling rule - the real governing rule is CR 707. Largest of the four sub-slices:
          CR 707.2b's "a copy freezes at copy-time, doesn't track the original afterward" needs a
          real distinct copiable-values concept, not just a pointer back to the source
          `CardDefinition`. Foundational for the other layers (1 feeds 2-7) but not blocking them
          today, since they all still read `CardDefinition` directly - a real implementation here
          would ideally sit underneath 5b/5d once both exist, but doesn't have to land first.
      5d. **Layer 3 (text-changing effects, CR 613.1c/612)** — nothing exists; `oracle_text` is a
          plain `String`, never parsed or treated as overridable anywhere in engine logic. Smallest
          of the four *if* scoped narrowly (CR 612.8's name-setting case is the common real-world
          one) - scoped to CR 612.2's general "correct usage" in-text word substitution instead, this
          is actually the most open-ended of the four, since nothing parses `oracle_text` at all
          today. Recommend narrowing hard to name-setting only when this slice's own requirements
          pass happens, not deciding the general case unilaterally now.
2. ~~**Targeting system**~~ — done, `phase5/targeting-basics` slice (docs/implementation.md,
   below). `CastSpell`/`ActivateAbility` now validate count/filter/dedup before pushing to the
   stack (CR 601.2c-e/602.2b), and `ResolveStackTop` fizzles on CR 608.2b (all targets illegal at
   resolution). Narrower than the full rules text — see that slice's own UNRESOLVED list (no
   "up to N" counts, no multi-"target"-word spells, no partial fizzle, `TargetSpec::Script` dead).
3. ~~**Static ability self-registration**~~ — done, `phase5/static-ability-self-registration`
   slice (docs/implementation.md, below). `Game.CreateStaticAbility` mirrors
   `Game.CreateTriggeredAbility`'s builder/queued-registration exactly, producing a real
   `ContinuousEffect` instead of a `RegisteredAbility`. Narrower than the full rules text — see
   that slice's own UNRESOLVED list (5 real layers only, no `Script`-backed predicate/effect,
   always `Permanent` duration, no conditional "as long as" gating).
4. ~~**General activated abilities**~~ — done, `phase5/activated-ability-costs` slice
   (docs/implementation.md, below). `RegisteredAbility.cost` generalized from the one-shot
   `AbilityCost::Tap` enum to a real `CostSpec` (`AdditionalCost::TapSelf`/`Sacrifice`/`Discard`/
   `TapPermanents`/`LifePayment`), `FilterSpec::matches`/`PaymentPlan::validate` made real, and
   `Game.CreateActivatedAbility` added alongside the triggered/static builders. Multi-ability-per-
   card stable indexing stays exactly as fragile as before (still positional by registration
   order) — orthogonal to "what can a cost contain," explicitly not bundled in.
5. ~~**Replacement/prevention effects (CR 614-616)**~~ — done, `phase5/replacement-prevention-effects`
   slice (docs/implementation.md, below). `ReplacementEffect` gained the same `Primitive`/`Script`
   split every other filter-shaped type in this codebase uses; `apply_replacements`/
   `submit_replacement_choice` are real. Narrower than the full rules text — see that slice's own
   UNRESOLVED list (two `EventFilter` shapes only, no Lua builder). Surfaced two real, pre-existing
   gaps while scoping this — split out as their own backlog items rather than silently fixed or
   silently ignored:
   5a. ~~**`AssignCombatDamage` bypasses replacement checking**~~ — done,
      `phase5/fix-replacement-bypasses` slice (docs/implementation.md, below).
   5b. ~~**SBA-driven zone changes bypass replacement checking**~~ — done, same slice. Real CR 616.1
      pause support mid-SBA (not just a deterministic first-match fallback) was built, per the
      human's explicit choice when this fork was raised.
6. ~~**Cost system completeness**~~ — done, across 5 sub-slices (6a-6e): 6a (Phyrexian/snow/hybrid
   mana payment, `ManaSymbol`/`AtomicManaSymbol` in `mana.rs`, tested in `phase5_activate_ability_costs.rs`)
   and 6b (`{X}` costs, `phase5_x_costs.rs`) are real — code and tests exist — but neither ever got
   its own session write-up appended below; a real documentation gap noticed while scoping the
   continuous-effects-script slice, flagged here rather than silently patched over with an invented
   history. 6c (`phase5/flashback-alternative-costs`, alternative costs), 6d (`phase5/cost-modifiers`,
   cost reduction/increase), and 6e (`phase5/ability-mana-cost-strings`, mana-cost components on an
   activated ability's own cost via `ManaCost::parse`) all have real write-ups below. Structured
   additional costs (sacrifice/discard/tap-permanents/life) were already real as of
   `phase5/activated-ability-costs` (above).
7. ~~**Trigger system completeness**~~ — once-per-turn/intervening-if (CR 603.2-603.3), last-known-
   info fallback for a trigger whose source already left the battlefield (CR 603.10, flagged since
   the script-bridge-trigger slice), non-ETB trigger conditions (leaves-battlefield, upkeep,
   damage-dealt, etc — only `Game.Conditions.Enters`/`Always` existed before this item started).
   ~~Damage-dealt conditions~~ — done, `phase5/damage-dealt-triggers` slice (docs/implementation.md,
   below): `ConditionPrimitive::DealsDamage`/`IsDealtDamage`. Surfaced a new, previously-undocumented
   gap while scoping this — split out as its own sub-item rather than silently left implicit:
   7a. ~~**`AdvanceTurnStep` bypasses `CollectTriggers` entirely**~~ — done, `phase5/upkeep-triggers`
      slice (docs/implementation.md, below). `AdvanceTurnStep`'s normal (non-`Untap`, non-`Cleanup`)
      step/phase transition now routes its events through the real `ReplacementCheck`/`CommitEvents`/
      `SbaLoop`/`CollectTriggers` pipeline instead of `log_events`, proven with new
      `ConditionPrimitive::BeginningOfYourUpkeep`/`BeginningOfEachUpkeep`. `Untap` (CR 502.4: no
      priority granted, so no trigger window to place one before) and `Cleanup` (its own separate
      `CleanupSbaCheck` flow, still not wired to `collect_triggers` either) are unaffected — still
      real, open gaps, not silently expanded into this fix. Cleanup's own gap is still open (see
      below).
   ~~CR 603.10 last-known-information fallback for dies/leaves-battlefield~~ — done,
   `phase5/dies-triggers` slice (docs/implementation.md, below): `ConditionPrimitive::Dies`/
   `LeavesBattlefield`, wired for the SBA-driven death path (`sba::commit_death`) via `Engine`'s
   transient `departed_triggered_abilities` stash.
   ~~Sacrifice-cost LKI wiring~~ — done, `phase5/draw-combat-end-triggers` slice
   (docs/implementation.md, below): the same stash now also captures a Sacrifice/Discard cost
   payment's departed abilities (`commands::move_to_graveyard_for_cost` → `commit_payment` →
   `Engine::submit_payment`), so a "when this dies" ability fires off a sacrificed permanent too —
   Discard's own half is captured but stays dormant (no condition matches a Hand-departure LKI yet).
   ~~"Whenever you draw a card"/beginning-of-combat/beginning-of-end-step conditions~~ — done, same
   `phase5/draw-combat-end-triggers` slice: `ConditionPrimitive::DrawsCard`/`EachDrawsCard`,
   `BeginningOfYourCombat`/`BeginningOfEachCombat`, `BeginningOfYourEndStep`/`BeginningOfEachEndStep`
   — cheap additions once `AdvanceTurnStep`'s pipeline fix (7a) had already landed, exactly as that
   slice's own UNRESOLVED list predicted.
   ~~`Cleanup`'s own trigger-collection gap~~ — done, `master` (docs/implementation.md, below,
   "wire Cleanup's own SBA pass into real trigger collection"): `CleanupSbaCheck` now routes through
   `CollectTriggers`/`PutTriggersApnap` too, via a new `PipelineExit::CleanupCheck` that decides
   priority-vs-advance (CR 514.3) only once any triggers raised by cleanup's own SBA pass have
   actually had a chance to be placed — previously a dies-trigger from a creature dying during
   cleanup's own SBA check was silently dropped.
   ~~CR 603.2h ("this ability triggers only once each turn")~~ — done, same session
   (docs/implementation.md, below, "add CR 603.2h once-per-turn trigger cap"): new
   `ConditionSpec::PrimitiveOncePerTurn`/`Game.Conditions.OncePerTurn(tag)`, enforced in
   `triggers::collect_triggers` against a new `GameState::once_per_turn_triggered` set (reset each
   turn alongside `lands_played_this_turn`) — correctly caps even two qualifying events collected in
   the *same* batch (e.g. CR 510.1c's multi-blocker damage split), not just across separate turns.
   ~~Untap-step triggers~~ — done, `master` (docs/implementation.md, below, "make Untap-step triggers
   fireable"): `AdvanceTurnStep`'s `Untap` branch routes through the real pipeline too now, via
   `PipelineExit::AdvanceTurnStep` instead of `BackToPriority` so it still doesn't open the priority
   window CR 502.4 doesn't grant — `ConditionPrimitive::BeginningOfYourUntap`/`BeginningOfEachUntap`
   proves it (rare on real cards, but real).
   ~~`submit_discard`'s CR 514.1 hand-size discard bypasses `CollectTriggers`~~ — done, `master`
   (docs/implementation.md, below, "wire submit_discard into real trigger collection"): routes through
   `CollectTriggers`/`PutTriggersApnap` via a new unconditional `PipelineExit::CleanupSbaCheck`, proven
   with new `ConditionPrimitive::DiscardsCard`/`EachDiscardsCard`. Surfaced a real bug in the
   `CleanupCheck` fix above while wiring this — its priority-vs-advance check only looked at ledger
   growth *since its own since_ledger marker*, missing a trigger placed earlier in the same cleanup
   sequence (exactly this discard case); fixed by also checking `!stack.is_empty()`. `draw_step`'s
   library-side retirement also stashes its departed abilities now, for consistency, though still
   practically unreachable (no card has a library-card ability that cares about itself leaving the
   library). Cost-payment `Discard` (`move_to_graveyard_for_cost`) already reached the real pipeline
   before this fix.
   ~~CR 603.4's intervening-if clause~~ — done, `phase5/intervening-if` slice (docs/implementation.md,
   below): new `RegisteredAbility.intervening_if: Option<InterveningIfSpec>`, orthogonal to the
   trigger-event `condition` — checked once in `collect_triggers` at trigger time, baked onto the
   resulting `StackObject`, and rechecked fresh in `ResolveStackTop` at resolution (CR 608.2a), OR'd
   into the existing target-legality `fizzles` check. One concrete `InterveningIfPrimitive` this
   slice: `ControllerAtOrBelowLife(n)`.
   ~~`Scries`/`Surveils` conditions, filter-scoped Dies/LeavesBattlefield, Script-backed
   intervening-if~~ — done, `phase5/trigger-completeness-cleanup` slice (docs/implementation.md,
   below), all three bundled into one slice: `ConditionPrimitive::Scries`/`EachScries`/`Surveils`/
   `EachSurveils` (CR 701.22d/701.25d, the deferred half of the `scry-surveil` slice's own TODO);
   `DiesMatching`/`LeavesBattlefieldMatching(FilterSpec)` (CR 700.4/603.10's "whenever a creature you
   control dies," checked in `collect_triggers`'s live-source loop against a separate watching
   permanent, not the departed/LKI fallback loop `Dies`/`LeavesBattlefield` themselves use);
   `InterveningIfSpec::Script(ScriptHandle)`, invoked via a new `ScriptRuntime::invoke_intervening_if`
   that passes a minimal `state_to_lua` snapshot (currently just per-player life totals) into the Lua
   call — the same snapshot now also reaches every ordinary `Script` condition via `invoke_condition`'s
   widened call signature (additive, non-breaking). `DiesMatching`/`LeavesBattlefieldMatching` reuse
   `cost.rs`'s `FilterSpec` (including the `supertypes`/`subtypes` fields the prior `search-tutor`
   slice added) rather than a bespoke filter shape.
8. ~~**Script API expansion**~~ — more `Game.Ops` primitives (DrawCards, Counters, Exile, Sacrifice,
   CreateToken — only `GainLife` existed before this item started; ~~DrawCards~~, ~~Counters~~,
   ~~Exile~~, ~~Sacrifice~~, ~~CreateToken~~ done — all 6 complete), and a
   ~~hidden-information-safe `ScriptContext`~~ — done, `phase5/script-state-query` slice
   (docs/implementation.md, below). Correction to this item's own framing, found while scoping:
   `ScriptContext.state: &GameState` was never actually handed to Lua directly (traced every call
   site) — the real gap was `state_to_lua`'s snapshot only ever carrying per-player life totals, and
   three `invoke_*` methods (`invoke_cda`/`invoke_continuous_affects`/`invoke_continuous_effect`)
   passing no state at all. `state_to_lua` is now the actual hidden-information boundary DESIGN.md's
   open question asked for: full public-zone contents (battlefield/graveyard/exile/command, CR 400.2)
   as card-summary tables, count-only for hidden zones (hand/library, CR 402.3) — no binding anywhere
   returns hand/library contents. All 5 state-aware `invoke_*` methods now receive it. Backlog item 8
   is fully closed. ~~Damage~~ — done, `phase5/script-ops-damage` slice
   (docs/implementation.md, below): `AbilityEffect::Damage(u32)`/`Game.Ops.Damage(n)`, reusing the
   real `ReplacementCheck`→`CommitEvents` pipeline (`commands::apply_damage_event`) combat damage
   already uses, so prevention/replacement effects and Lifelink/Wither/Infect/Deathtouch apply for
   free. Surfaced a bigger, previously-undocumented gap while scoping this — a cast spell never
   resolved *any* effect at all (only activated/triggered abilities could) — closed as part of the
   same slice rather than left silently narrower: `CardDefinition` gained an `effect: Option<
   AbilityEffect>` field, and `CastSpell` now bakes it onto the pushed `StackObject` instead of a
   hardcoded `None`. ~~Destroy~~ — done, `phase5/script-ops-destroy` slice (docs/implementation.md,
   below): `AbilityEffect::Destroy`/`Game.Ops.Destroy()` (CR 701.8/702.12b), running the real CR
   614.13 `WouldDie` replacement check and `sba::commit_death` inline in `ResolveStackTop` (no
   generic-`ZoneChange`-commit path existed to reuse the way `Damage` reused `DamageDealt`'s, so
   this mirrors `resolve_deaths`'s own per-object body instead). Narrower than the full rules text —
   CR 616.1's ordering choice (2+ replacement effects matching the same scripted destroy) isn't
   supported, silently no-ops rather than guessing an order; see that slice's own UNRESOLVED list.
9. ~~**Player-input-mid-script (Lua coroutines)**~~ — done for `Game.SelectTargets`,
   `phase5/script-coroutines` slice (docs/implementation.md, below). A spike
   (`phase5_coroutine_spike.rs`) confirmed the real mechanism against vendored mlua 0.9 before any
   `Engine`-level plumbing was built: `ScriptRuntime::invoke_operation` now runs a script's operation
   as a coroutine (`Lua::create_thread`/`Thread::resume`) instead of a plain call — transparent for
   any script that never yields — and `Game.SelectTargets` is a plain Lua function that calls
   `coroutine.yield` directly (not a Rust-bound callback; mlua 0.9 without the `async` feature can't
   yield from inside a Rust closure). A yield pauses the whole pipeline on a new
   `ProcessState`/`StepStatus::AwaitingScriptChoice`, mirroring `AwaitingReplacementChoice`/
   `AwaitingLegendRuleChoice` exactly; `Engine::submit_script_choice` resumes the suspended thread
   (stashed via the registry, same `RegistryKey` mechanism `scripts` already uses for `Function`s).
   Scoped to exactly one request shape this slice — object-only, count always 1, no player targets,
   no "any target." `invoke_target`/`TargetSpec::Script` (a *static* target-requirement predicate,
   a different concept from an operation's own mid-resolution ask) are untouched, still `todo!()`.
   **Split out, not bundled in:**
   9a. ~~**"May" abilities (CR 608.2d)**~~ — done, `phase5/script-may-abilities` slice
      (docs/implementation.md, below). `Game.MayChoose()`, same coroutine mechanism as
      `Game.SelectTargets` with a new `ScriptChoiceRequest::MayChoose` variant. **Split out, not
      bundled in:**
      9a-i. ~~**Modal spell mode-selection (CR 700.2a)**~~ — done, `phase5/script-modal-spells` slice
         (docs/implementation.md, below). Cast-time, not mid-resolution — confirmed cheaper than
         this entry originally framed it: `PlayerActionKind::CastSpell::modes` already flowed
         end-to-end as dead plumbing, no new `ProcessState` pause needed.
   9b. ~~**Player-target selection via `Game.SelectTargets`**~~ — done, `phase5/script-target-players`
      slice (docs/implementation.md, below). `{ type = "player" }`/`{ type = "any" }` widen the
      request beyond object-only; the resumed answer is now a tagged `{ kind, id }` table so a
      script can tell an object answer from a player one.
10. ~~**SBA completeness (player-loss triad)**~~ — done, `phase5/sba-player-loss` slice
    (docs/implementation.md, below): 704.5b (empty-library draw loss), 704.5c (10+ poison),
    CR 903.10a (commander damage ≥21, `PlayerState::commander_damage_from` now real) all fold into
    `sba::apply_player_loss` alongside the existing 704.5a. The commander-damage check is real but
    currently unreachable through actual gameplay — nothing increments `commander_damage_taken` yet
    (no "is this object a commander" concept exists — that's item 11 below). **Split out, not
    bundled in:**
    10a. ~~**Legend rule (704.5j) / world rule (704.5k)**~~ — done, `phase5/legend-world-rule` slice
       (docs/implementation.md, below). Legend rule is a real player choice
       (`SbaOutcome::NeedsLegendRuleChoice`/`Engine::submit_legend_rule_choice`), same
       `ProcessState`/`StepStatus` pause-and-resume shape CR 616.1's `NeedsOrderingChoice` already
       established; the losers are destroyed by feeding them into the very same `sba::resolve_deaths`
       every other SBA death uses, so a "would die, exile instead" replacement effect on a losing
       legendary still applies for free. World rule is fully deterministic (no player choice — CR
       704.5k resolves purely on timestamp) and just joins the ordinary dying-permanent list.
11. **Multiplayer/Commander generalization** — combat is 2-player only today (`CombatState`/
    `DeclareBlockers` assume one defending player); command-zone tax/damage/elimination-without-
    ending-the-game aren't wired despite being named in DESIGN.md's Commander section. Turned out to
    be big enough to split into its own sequence rather than one item, same as items 1/9/10 — sub-
    slices below, compiled after a real research pass over `combat.rs`/`commands.rs`/`triggers.rs`/
    `sba.rs`/`player.rs`/`engine.rs` (not guessed at) to separate what's already N-player-safe from
    what's actually hardcoded to 2 players or entirely unbuilt:
    1. `CombatState` itself (`attackers: Vec<(ObjectId, Target)>`) and `DeclareAttackers` are already
       N-player-safe — each attacker independently names its defender via `Target`
       (`commands.rs`'s `DeclareAttackers` arm validates each against `state.turn_order` generically).
       `triggers.rs`'s APNAP ordering (`group_apnap`) is also already generalized over
       `state.turn_order`, not hardcoded to two players. Neither needs a slice.
    11a. ~~**N-player combat: defending player per attacker**~~ — done, `phase5/multiplayer-
       defending-player` slice (docs/implementation.md, below). The real gap was narrower than the
       backlog text worried: only `DeclareBlockers`'s hardcoded "first non-active player" defender
       lookup was actually broken (CR 508.5a) — `AssignCombatDamage`'s damage-assignment was already
       per-attacker-target, confirmed by reading the code before writing any. `CombatState.
       blockers_declared: bool` became `declared_defenders: Vec<PlayerId>` (CR 509.1 as a turn-based
       action repeated once per defending player, not once total); `combat.blocks` now accumulates
       across each defending player's own submission instead of one submission overwriting another.
    11b. ~~**Player elimination actually affects the game**~~ — done, `phase5/player-elimination`
       slice (docs/implementation.md, below). `GameOver` now actually constructs (CR 104.2a/104.4a,
       covers both the 2-player and N-player case the same way); `PassPriority`/Cleanup-step turn
       rotation skip an eliminated player (CR 800.4b); their objects leave the game — per-owner
       zones drained, shared zones/stack filtered by ownership, purged via the same
       `retire_object` a normal death uses (CR 800.4a). Caught a real regression during
       verification (a solo test fixture's own empty-library-on-turn-1 quirk got treated as a real
       elimination and had its objects wiped) — fixed with a `turn_order.len() >= 2` guard, not
       worked around.
    11c. ~~**Commander marking + casting from the command zone**~~ — done, `phase5/commander-
       marking-and-casting` slice (docs/implementation.md, below). `GameObject.is_commander: bool`
       (CR 903.3, rides along automatically through every real zone-change clone in this crate);
       `CastSpell` accepts a commander cast from the command zone by its owner (CR 903.8, normal
       mana cost, tax deferred to 11d); `Engine::new_game`/`PlayerConfig.commanders` mints them into
       a real command zone (CR 903.6). Scope grew by one deliberate, human-confirmed addition beyond
       the literal text: CR 903.9a (owner may return a commander from graveyard/exile to the command
       zone) — without it a commander could only ever be cast once per game, leaving 11d/11e
       untestable through real repeated-death gameplay. 903.9b (hand/library redirect) stays
       deferred.
    11d. ~~**Commander tax (CR 903.8)**~~ — done, `phase5/commander-tax` slice (docs/implementation.md,
       below). Generic cost +{2} per prior cast from the command zone this game, tracked per
       commander by name (not `ObjectId`, which churns every cast) so Partner commanders
       (CR 702.124d) get independent counters automatically.
    11e. ~~**Commander damage production**~~ — done, `phase5/commander-damage` slice
       (docs/implementation.md, below). Closes backlog item 10's own dangling note (that slice's
       commander-damage SBA check is real but "inert until backlog item 11's Commander-format
       machinery... exists"). Routes a commander's *combat* damage into `PlayerState.
       commander_damage_taken` (already real, just never written to) — required a new
       `GameEventKind::DamageDealt.is_combat_damage` field, since nothing previously distinguished
       CR 510 combat damage from a spell/ability's own damage effect.
    **Explicitly not a slice:** color identity/deck validation — DESIGN.md already scopes it out
    ("not in scope for the engine itself unless deck validation is wanted later").
12. ~~**First/double strike (CR 702.4/702.7)**~~ — done, `phase5/first-strike` slice
    (docs/implementation.md, below): the combat damage step splits in two (first-strike step, then
    regular step) whenever any attacker/blocker has first or double strike; `FirstStrikeDamage` event
    feeds the same damage-assignment machinery `CombatDamage` already uses. Remaining
    combat-keyword edges (trample, banding, etc) still open — this item was first/double strike
    only.
13. ~~**Mana system completeness**~~ — done, `phase5/mana-add-choice` slice (docs/implementation.md,
    below). Correction to this item's original text, found while scoping: "more than one fixed mana"
    already worked mechanically (`AddMana` is a `Vec`, never tested with 2+ units) — the two real
    gaps were no `Game.Ops.AddMana`/`Game.Ops.AddManaChoice` Lua binding at all (a card script
    couldn't define a mana ability, only Rust test fixtures could) and no player-chosen-color
    mechanism. Both closed: new `AbilityEffect::AddManaChoice { count, choices: ColorSet }`, chosen
    color(s) carried on a new `PlayerActionKind::ActivateAbility::chosen_colors: Vec<Color>` field
    (CR 602.2b — the choice is locked in at activation, not a mid-resolution pause, since a mana
    ability never uses the stack, CR 605.3b).
14. ~~**Shuffle primitive**~~ — done, `phase5/script-ops-shuffle` slice (docs/implementation.md,
    below). Correction to this item's original text: `Zone::new`/`push_top`/`top` were never
    `todo!()` — verified real on inspection, same class of stale-doc gap the 6a/6b cost-completeness
    items had. `Engine::shuffle_library` also already existed (pregame-setup-only, private); this
    slice just exposed it as `AbilityEffect::Shuffle`/`Game.Ops.Shuffle()`. Split off as their own
    follow-ups, too large to bundle here:
    14a. ~~**Scry/Surveil**~~ — done, `phase5/scry-surveil` slice (docs/implementation.md, below).
    Correction to this item's original text: didn't end up needing `PlayerActionKind::MakeChoice`
    at all — resolved as a dedicated `ProcessState`/`StepStatus` pause pair, same "own entry point"
    shape the legend-rule choice-pause already established, not a generic make-a-choice action.
    14b. ~~**Search/tutor**~~ — done, `phase5/search-tutor` slice (docs/implementation.md, below).
    Correction to this item's original text: no "look at the whole library" visibility exception
    was ever needed — `GameState`/`Zone` already expose full contents freely; the project's only
    hidden-info gap is the Lua callback boundary (DESIGN.md's open question), never crossed by this
    Rust-native primitive. The real blocker turned out to be `cost.rs`'s `FilterSpec` having no
    supertype/subtype fields at all (couldn't match "basic land card") — fixed as part of this
    slice, benefiting every other `FilterSpec` caller (Sacrifice/Discard/TapPermanents costs) too.
15. ~~**CR 601.6 unwind**~~ — done, `phase5/payment-unwind` slice (docs/implementation.md, below).
    New `Engine::unwind_payment` (and matching `unwind_payment` TOML/CLI step) lets the caller
    explicitly abandon a cast/activation instead of endlessly resubmitting a `PaymentPlan` — reuses
    `move_resolving_spell` (CR 608.2b's own fizzle-path helper) to return a `Spell`'s card to hand
    under a fresh id; an `ActivatedAbility`'s stack entry is just removed, source untouched. Not a
    true CR 733 state rewind (this engine's ledger is append-only, no undo primitive exists) — an
    honest forward zone-change/removal instead, marked by a new `GameEventKind::CastingUnwound`.
16. **Planeswalkers/Battles/DFCs/split/Sagas** — explicit non-goals in DESIGN.md's v1 scope;
    lowest priority unless that scope changes.
17. ~~**Commander color identity (CR 903.4/903.5c/d)**~~ — done, `phase5/color-identity` slice
    (docs/implementation.md, below). DESIGN.md originally scoped this out entirely ("not in scope
    for the engine itself unless deck validation is wanted later") - revisited as a small,
    self-contained slice once the rest of the Commander sub-sequence (item 11) closed. Pure query
    functions (`crates/engine/src/color_identity.rs`), not enforced by `Engine::new_game` or any
    command.

## Known unresolved gaps (not their own backlog items, tracked for awareness)

Compiled after backlog item 1 (the continuous-effect layer engine) closed and items 1-15/17 were all
struck through above, leaving only item 16 (explicit non-goal) unstarted. These are real, smaller
loose threads surfaced across already-closed slices' own UNRESOLVED notes, plus a sweep of live
`TODO` markers in `crates/engine/src` cross-checked against current code (not stale doc history) - not
promoted to numbered backlog items themselves, but worth keeping visible rather than letting them
scatter across individual slices' write-ups. Re-derive/update this list rather than trusting it
verbatim once enough time has passed for the underlying code to have moved.

- **Layer 2 (control) internal filtering**: `layers.rs`'s own `applicable_effects_in_layer`/
  `ContinuousEffect::applies_to` still read the raw `GameObject.controller` field for every layer, not
  `layers::effective_controller` - a control-changed permanent's card_types/keywords won't correctly
  gate *other* continuous effects' own `EffectFilter` checks against it. Deliberate (migrating it would
  make layer 2's own resolution recursively call itself), not an oversight - see the
  `layer-engine-control-consumers` slice's own design finding.
- **Layer 1 (copy)**: no CR 707.9 exception/modification support (always a bare copy of the target's
  current values), no ETB-replacement "enters the battlefield as a copy" path (Clone itself) - only
  `AbilityEffect::BecomeCopy`/`TokenSpec::CopyOf` exist as entry points.
- **Layer 3 (text-changing)**: general CR 612.2 "correct usage" word/symbol substitution and the
  single-card rules (612.5 Exchange of Words, 612.6 Volrath's Shapeshifter, 612.7 Spy Kit, 612.9 name
  stickers, 612.10 splice) stay unmodeled - only CR 612.8 name-setting is real. "Loses all abilities"
  (a text-removal shape) isn't modeled either.
- **CR 613.6-8 dependency**: `EffectPayload::Script` effects never participate in dependency ordering
  (always treated as independent); cross-layer/cross-sublayer dependency isn't modeled (this one is by
  rule - CR 613.8 is explicitly same-layer/sublayer only - not a real gap).
- **Targeting**: no spell with more than one distinct "target" word, `TargetSpec::Script`/
  `ScriptRuntime::invoke_target` still `todo!()`, "N or more" counts unmodeled. Real multi-target
  effect resolution and CR 608.2b's actual partial-fizzle text closed separately, `phase5/multi-
  target-resolution`; "up to N" counts closed separately too, `phase5/targeting-up-to-n` (both
  docs/implementation.md, below).
- **Static abilities**: `StaticAbilityBuilder`'s own duration gap is closed, `phase5/static-ability-
  duration` (docs/implementation.md, below) — `:SetDuration(Game.Durations.*)`, same 4-tag
  `DurationTag` decode `Game.Ops.Pump` already used, defaulting to `Permanent` when unset. "As long
  as ..." conditional gating closed separately, `phase5/conditional-continuous-effects` (CR 611.2c,
  docs/implementation.md, below) — `ContinuousEffect.condition`/`Game.InterveningIf.
  ControlsAtLeast`/`:SetCondition`, one concrete shape (`ControllerControlsAtLeast`).
  `EffectDuration`'s broader inert-duration gap is closed for `UntilYourNextTurn`/`UntilEndOfCombat`
  (the drain mechanism, `phase5/effect-duration-expiration`) and a real creation path exists for a
  *resolution-time* temporary effect (`AbilityEffect::Pump`/`Game.Ops.Pump`, `phase5/pump-effect`,
  docs/implementation.md, below) as well as now a *static* ability's own duration (this bullet).
- **Activated abilities**: multi-ability-per-card indexing is still positional by registration order,
  not a stable id - fragile if a card's own ability list ever needs to change shape.
- **Replacement effects**: `EventFilter` still only has two event *shapes* (`DamageDealt`,
  `WouldDie`) - protection (CR 702.16), ETB-modifier replacements, "would draw a card"
  substitution, and any other event kind stay unmodeled. Within those two shapes,
  `Game.CreateReplacementEffect`'s Lua construction path is now general: `phase5/static-
  regeneration` (docs/implementation.md, below) proved `WouldDie`/`Regenerate` (CR 701.19b);
  `phase5/replacement-effect-shapes` (docs/implementation.md, below) generalized the same builder
  to `DamageDealt`/`PreventDamage` (CR 615) and `WouldDie`/`ReplaceZoneDestination` (CR 614.13-style
  "instead" substitution, non-regenerate), plus a settable `ConsumptionPolicy`
  (`OneShot`/`Repeatable`/`LimitedUses`) - all payload shapes `ReplacementEffect::matches`/`apply`
  already handled correctly in Rust, just previously Lua-inaccessible. `RedirectDamage(Target)`
  remains Lua-inaccessible - a redirect target isn't a static Lua literal, named as its own
  follow-up rather than guessed at.
- **Triggers**: this bullet was stale - both the player-relative shape (`DiscardsCard`/
  `EachDiscardsCard`) and the self-referential shape (`ConditionPrimitive::Discarded`, "when *this*
  card is discarded", `phase5/discarded-trigger`, docs/implementation.md below) are now real, off
  either discard source (cost payment or the cleanup hand-size discard). `draw_step`'s library-side LKI
  stash is likewise real but practically unreachable (no card has a library-card ability that cares
  about itself leaving the library) - not fixed here, same standing note.
- **Script-error swallowing (cross-cutting)**: closed for every site with `&mut Engine`/a real
  mutation pipeline to log through, `phase5/script-error-surfacing` (docs/implementation.md, below) -
  `GameEventKind::ScriptError { message }`, logged from `engine.rs`'s `ResolveStackTop` intervening-if
  recheck and `triggers.rs`'s `collect_triggers` (now returns `(Vec<PendingTrigger>, Vec<String>)`,
  logged by its one caller). `layers.rs`'s own script call sites
  (`applicable_effects_in_layer`/`resolve_primitive`) are a deliberate, named exception - pure
  `&GameState`-only characteristic-query functions called from `effective_characteristics`'s many
  call sites; logging from there would mean threading error-reporting through that entire call graph,
  out of scope for this slice.
- **`Destroy`/`Sacrifice`/`Exile` script ops' CR 616.1 ordering choice**: closed, `phase5/destroy-
  ordering-choice` (docs/implementation.md, below) - all three now pause via a real
  `AwaitingReplacementChoice` instead of silently no-oping. Exile's own arm is fixed by the same
  refactor but has no end-to-end test - `EventFilter` only has two shapes and `WouldDie` only ever
  matches `to: Graveyard`, so no fixture can construct 2 effects that both match a scripted Exile's
  own `to: Exile` proposal (same standing "two `EventFilter` shapes only" gap named elsewhere in this
  list).
- **Combat keywords beyond first/second strike/trample**: banding and the rest of CR 702's combat
  keyword list are still unbuilt - the `first-strike` slice was scoped to exactly CR 702.4/702.7;
  trample (CR 702.19a/b/d) closed separately, `phase5/trample` (docs/implementation.md, below).
  Trample over planeswalkers (702.19c/e/f/g) stays unmodeled, same as planeswalkers/battles generally.
- **Protection (CR 702.16)**: closed for "protection from color" (702.16e's shape), `phase5/
  protection-from-color` (docs/implementation.md, below) - `EffectiveCharacteristics.
  protection_colors`, `EffectPrimitive::AddProtectionColor`/`Game.Effects.AddProtectionColor`,
  enforced at damage (702.16a), targeting (702.16c), and blocking (702.16d). Protection from card
  type/subtype/a specific player/"from everything" stays unmodeled - only color. Enchant/Equip
  (702.16b) stays unmodeled too - needs real attach/enchant machinery this engine doesn't have
  (`GameObject.attached_to`/`.attachments` are inert placeholder fields, same gap `phase5/gain-
  control-effect` already named for its own aura-sourced follow-up).
- **CR 111.7/704.5d (token cessation)**: closed, `phase5/token-cessation` (docs/implementation.md,
  below) - `sba::collect_ceasing_tokens`/`commit_token_cessation`. CR 111.8 (a token that already left
  the battlefield can't move zones again) is a related but separate rule, not covered.
- **CR 402.2 (max hand size)**: closed, `phase5/max-hand-size` (docs/implementation.md, below) -
  `HandSizeModifier`/`effective_max_hand_size` (`types/player.rs`), a dedicated mechanism deliberately
  parallel to (not folded into) `state.continuous_effects`, same reasoning `cost_modifiers` already
  established. No `Game.CreateHandSizeModifier` Lua builder yet - direct-literal fixtures only, same
  bootstrap stance `CostModifier` itself took.
- **Regeneration (CR 614.8/701.19)**: closed, `phase5/regeneration` (docs/implementation.md, below) -
  `ReplacementPayload::Regenerate`, gated to genuine destruction only (CR 704.5g lethal damage/
  deathtouch, and the scripted `Destroy` op) via a per-call `is_destroy` flag, not CR 704.5f (zero
  toughness) or CR 704.5k (world rule) - neither of those is "destroy." CR 701.19c ("can't be
  regenerated") also closed, `phase5/cant-be-regenerated` (docs/implementation.md, below) -
  `KeywordSet::CANT_BE_REGENERATED`, checked at shield-creation time by the same slice's new
  `AbilityEffect::Regenerate` (the first real creation path for a shield at all - every prior test
  built one directly via a Rust struct literal). CR 701.19b (a *static* ability's own repeatable
  shield) also closed, `phase5/static-regeneration` (docs/implementation.md, below) -
  `Game.CreateReplacementEffect` self-registers a `ConsumptionPolicy::Repeatable` shield;
  `ReplacementEffect::matches` now re-checks CANT_BE_REGENERATED fresh on every application (not
  just at creation time) since a static shield's target can gain that status well after the shield
  itself was registered. Found and fixed alongside it: `retire_object`/`purge_granted_abilities`
  never purged `state.replacement_effects` - harmless before this slice (every prior entry was
  `OneShot`/`LimitedUses` and self-consumed), a real leak once a `Repeatable` entry could outlive
  its source.
- **CR 601.6/733 unwind**: `Engine::unwind_payment` is an honest forward zone-change/removal, not a
  true state rewind - this engine's ledger is append-only with no undo primitive, a permanent
  architectural choice rather than a gap expected to close later.
- **Color identity/deck validation**: `color_identity.rs` is pure query functions - nothing enforces
  deck legality anywhere (`Engine::new_game` or otherwise), matching DESIGN.md's original scoping.
- **Dead Phase-3 stub methods, zero callers anywhere**: `Subtype::new`/`Subtype::as_str`,
  `CardDefinition::has_type`/`has_supertype`/`has_subtype`, and several `mana.rs` constructors
  (`ManaCost::from_symbols`/`from_pool`, `ManaPool::add`, etc.) are still bare `todo!()` bodies left
  over from the Phase 3 stubbing pass. Confirmed via a real grep for callers, not assumed - nothing in
  `crates/engine/src` or its tests calls any of them, so they're inert today, but a landmine (an
  instant panic) if anything ever starts calling them without filling them in first.

## Phase 5: phase5/layer-engine-basics slice

Branch `phase5/layer-engine-basics`, off master. Slice backlog item 1 (above). Scope, locked in
before writing code: CR 613 layers 6 (ability adding) and 7b/7c (power/toughness set/modify) only —
copy/control/text/type/color (1-5) and 7a/7d/7e stay unbuilt, each with a doc-comment TODO on
`Layer` explaining what it needs. Timestamp-only ordering within a layer, no CR 613.6-8 dependency
system (large TODO on `Layer` explaining the gap and why it's real rules work, not a sort tweak).
`ContinuousEffect`s are built directly via Rust struct literal (test/scenario fixtures) — no
`Game.AddContinuousEffect` Lua binding this slice, same bootstrap-before-scripting pattern every
prior subsystem used. Real consumer: `sba::check_state_based_actions`'s lethal-toughness check only
(already has no `&ScriptRuntime` dependency to add, since the primitive-only path needs zero Lua
calls) — combat-damage power and blocking-legality keyword checks (`commands.rs`) are flagged
follow-up, not touched.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 11 new tests in
`crates/engine/tests/phase5_layer_engine_basics.rs` (base case, `AddKeyword` union, `ModifyPT`
additive, `SetPT` overriding base before `ModifyPT` applies on top, later-timestamp `SetPT` winning
within a layer, controller/card-type filter scoping, `self_only` filter scoping, CDA still
skipped-not-guessed, and three SBA-real-consumer tests: a boost saving a creature from lethal
toughness, the same creature dying without it, and a granted Indestructible surviving lethal
damage), all 178 prior tests unaffected. 189 tests total, 0 failed. Confirmed via the real CLI too:
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_layer_engine_basics.toml` — 1/1
action ran, 2/2 assertions passed (an anthem's `+1/+1` keeping a 2-damage-marked 2-toughness bear on
the battlefield instead of the graveyard).

**What got implemented:**
- `types/effect.rs`: `Layer` gained derives (`Clone, Copy, PartialEq, Eq, Debug`) plus the doc
  comment above explaining every unbuilt layer and the dependency-system gap. New `EffectPrimitive`
  (`SetPT`/`ModifyPT`/`AddKeyword`), `EffectPayload` (`Primitive`/`Script`), `EffectFilter`
  (`controller`/`card_types`/`self_only`), `AffectedFilter` (`Primitive`/`Script`) — same
  `Primitive`/`Script` split `ConditionSpec`/`AbilityEffect` already established, reusing
  `cost.rs`'s `FilterController` (which gained `Clone, Copy, PartialEq, Eq, Debug`) rather than a
  second controller-filter enum. `ContinuousEffect.affected_predicate: ScriptHandle` replaced with
  `affected: AffectedFilter` + a new `effect: EffectPayload` field (the same "matcher had no
  rewrite" gap `ReplacementEffect` still has, fixed here for continuous effects specifically).
  `ContinuousEffect::applies_to` real body for the `Primitive` case (zero Lua calls); `Script`
  returns `false` — nothing can construct that variant yet (no card creates a continuous effect
  through Lua this slice), so it's dead code with a clear TODO rather than a `todo!()` that would
  panic the moment a future slice's fixture accidentally hits it.
- `EffectiveCharacteristics` gained `keywords: KeywordSet`.
- `layers.rs::effective_characteristics`: real body. Starts from `CardDefinition` base (power/
  toughness netted against counters via the existing `PTValue::net_with_counters`, matching what
  `sba.rs` already did; keywords straight from `CardDefinition.keywords`), applies layer 6 then 7b
  then 7c from `state.continuous_effects` filtered by `applicable_effects_in_layer` (matches layer,
  resolves the effect's source to check its controller, sorts by `timestamp`). No `&ScriptRuntime`
  parameter — nothing evaluable this slice needs one. `colors` is read off `color_indicator`
  directly rather than through `CardDefinition::colors()` (itself still `todo!()`, depends on
  `ManaCost::colors()`/`ManaSymbol::colors()` which are also `todo!()`) — flagged as a TODO rather
  than filling three unrelated `todo!()`s incidentally; nothing this slice touches reads `colors`.
- `sba.rs`'s `apply_creature_death`: rewritten to call `effective_characteristics` for toughness and
  Indestructible instead of reading `CardDefinition`/counters directly — first real consumer, proves
  the query actually changes SBA outcomes (see the three new "real consumer" tests). Restructured
  into a two-pass (`candidates` then `dying`) rather than one `filter_map` over `state.objects.
  values()`, to keep the borrow shape simple rather than fighting the borrow checker over
  interleaving `state.objects.values()` with a `&GameState`-taking query call.
- `commands.rs`: two TODO comments added (no behavior change) at the Flying/Reach blocking-legality
  check and the combat-damage power calculation, each pointing at this slice's SBA wiring as the
  template and explaining why they weren't retrofitted too (`execute_command` has no
  `&ScriptRuntime`/layer-engine access — same wall `battlefield_entries`' deferred-outcome pattern
  solved once already for script self-registration).
- `crates/scenario/src/lib.rs`: `CardSpec.continuous_effect: Option<ContinuousEffectSpec>` (new
  struct: `layer`/`power`/`toughness`/`keywords`/`filter_controller`/`filter_card_types`/
  `filter_self_only`) and a shared `build_continuous_effect` helper, wired into both
  `build_initial_state` and `build_new_game_engine` (parity with how `mana_ability`/
  `trigger_operation` are handled in both paths). `docs/scenario_format.md` documents the new field
  with a worked anthem example, referencing the new TOML fixture.

**STRUCT DEVIATIONS:** None beyond what the pre-slice requirements report scoped exactly
(`EffectPrimitive`/`EffectPayload`/`EffectFilter`/`AffectedFilter`, `EffectiveCharacteristics.
keywords`) — this *is* the plan, not a deviation from it.

**INTERFACE DEVIATIONS:** None — `effective_characteristics(state, object)`'s signature needed no
`&ScriptRuntime` param, which simplified the pre-slice assumption that wiring a real consumer would
require adding one to `sba::check_state_based_actions` too; turned out unnecessary once the slice
was scoped to primitives-only (Script evaluation deferred, `AffectedFilter::Script`/
`EffectPayload::Script` both dead code for now).

**SCOPE DEVIATIONS:**
- `types/cost.rs`'s `FilterController` gained derives it didn't have before — needed to reuse it
  inside `EffectFilter`/match on it inside `applies_to`, not anticipated by name in the pre-slice
  report but a direct, mechanical consequence of reusing the type rather than duplicating it.
- `commands.rs`'s two TODO-only comments (Flying/Reach check, combat-damage power) — anticipated in
  the pre-slice discussion (Q4) as the explicit trade-off of scoping the real consumer to SBA only.

**UNRESOLVED (all anticipated in the pre-slice requirements report, none new):**
- Layers 1-5 (copy/control/text/type/color) and 7a/7d(-as-a-real-layer-step)/7e — each has its own
  TODO on `Layer` itself.
- CR 613.6-8 dependency system — timestamp-only ordering, large TODO on `Layer` explaining what a
  real implementation needs.
- `AffectedFilter::Script`/`EffectPayload::Script` — structurally present, unevaluated; the next
  slice to need them (static-ability self-registration, backlog item 3) will have to thread a
  `&ScriptRuntime` into `effective_characteristics` and decide what "skip vs. invoke" means once a
  real card can construct one.
- Combat-damage power and blocking-legality (Flying/Reach/Menace) keyword checks — still read
  `CardDefinition` directly, flagged in `commands.rs` itself now, not just here.
- `CardDefinition::colors()`/`ManaCost::colors()`/`ManaSymbol::colors()` — all three still `todo!()`;
  `effective_characteristics.colors` reads `color_indicator` directly as a stopgap.

## Phase 5: phase5/layer-engine-consumers slice

Branch `phase5/layer-engine-consumers`, off master. Slice backlog item 1's sub-slice A. Scope:
wire the two consumers the layer-engine-basics slice deliberately deferred — combat-damage power
(`AssignCombatDamage`) and Flying/Reach/Menace blocking legality (`DeclareBlockers`) — into
`layers::effective_characteristics`.

**Correction to the prior slice, found while scoping this one:** the layer-engine-basics slice's
TODO comments claimed these two consumers were blocked the same way `CommandOutcome::
battlefield_entries` was (`execute_command` has no `&ScriptRuntime`). That was wrong —
`effective_characteristics(state, id)` only takes `&GameState`, since this slice's primitive-only
evaluation needs zero Lua calls. `execute_command` already has `&mut GameState`, which reborrows as
`&GameState` trivially. There was no architectural wall here at all; the TODO over-pattern-matched
against a different, real wall. Wiring both consumers turned out to be a same-day mechanical change,
not a slice requiring new plumbing.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 4 new tests in
`crates/engine/tests/phase5_layer_engine_consumers.rs` (a `ModifyPT` boost increasing combat damage
dealt, the same attacker without the boost dealing only its printed power, a continuous-effect-
granted Flying still enforcing CR 702.9b blocking legality, the same attacker without the grant
blocking normally), all 189 prior tests unaffected. 193 tests total, 0 failed. Confirmed via the
real CLI too: `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/
phase5_layer_engine_consumers.toml` — 15/15 actions ran, 1/1 assertion passed (a 3/3 bear with a
`+2/+2` continuous effect dealt 5 combat damage to the defending player, not 3).

**What got implemented:**
- `commands.rs`: two new private helpers, `effective_keywords(state, id) -> KeywordSet` and
  `effective_power(state, id) -> Option<i32>` — both wrap `layers::effective_characteristics`,
  tolerating a missing `id` (return `KeywordSet::empty()`/`None`) rather than panicking the way
  `effective_characteristics` itself does, since several call sites already separately raise
  `CommandError::InvalidTarget` for a missing id and just want the safe fallback otherwise.
- `DeclareBlockers`'s Flying/Reach check and Menace check: now call `effective_keywords` instead of
  reading `CardDefinition.keywords` directly.
- `AssignCombatDamage`: all three power reads (the split-validation loop, the attacker
  damage-emission loop, the blocker damage-emission loop) now call `effective_power` instead of
  `object.definition.power.as_ref().and_then(|p| p.net_with_counters(&object.counters))` directly —
  kept identical CDA-skip semantics (`effective_characteristics` returns `None` for the same reason
  `net_with_counters` did), just adds layer 7c on top.
- **Scope addition beyond the two consumers named in the requirements discussion:** the same
  function's Lifelink/Wither/Infect/Deathtouch keyword reads (further down, in the damage-application
  loop) were left reading `CardDefinition.keywords` directly by the prior slice's TODO — fixed here
  too, via the same `effective_keywords` helper, since leaving them un-migrated while Flying/Reach
  became layer-aware in the same function would have been an inconsistent half-state. Not scoped by
  name in the pre-slice discussion, but the same category of already-flagged gap in the same
  function — judged in-scope rather than a separate slice.

**STRUCT DEVIATIONS:** None — no new types, just two new private functions in `commands.rs`.

**INTERFACE DEVIATIONS:** None — `effective_keywords`/`effective_power` are new `fn`s, not changes
to any existing signature.

**SCOPE DEVIATIONS:**
- The Lifelink/Wither/Infect/Deathtouch keyword reads (see above) — not named in the pre-slice
  discussion, judged in-scope for the reason given there.
- `docs/scenario_format.md` gained a one-line pointer to the new TOML fixture, same pattern every
  prior scenario-crate-adjacent slice has followed.

**UNRESOLVED (anticipated, none new):**
- Layers 4/5/7a/7e, the CR 613.6-8 dependency system, `AffectedFilter::Script`/`EffectPayload::
  Script` evaluation — all still exactly where the layer-engine-basics slice left them; this slice
  only widened *who calls* the existing layers 6/7b/7c query, not the query's own coverage.
- `PlayLand`'s land-limit check, `is_sorcery_speed`, and other continuous-effect-consulting gaps
  flagged in earlier slices are untouched — this slice's scope was specifically combat/blocking.

## Phase 5: phase5/layer-engine-types-colors slice

Branch `phase5/layer-engine-types-colors`, off master. Slice backlog item 1's sub-slice B. Scope,
locked in before writing code: CR 613 layers 4 (`TypeChanging`) and 5 (`ColorChanging`), additive
only (`AddCardType`/`AddColor`, mirroring `AddKeyword`'s shape) — the full overwrite shape
(`SetCardTypes`/`SetColors`, mirroring `SetPT`'s override semantics) is deferred to its own future
slice, same "narrow the first cut" discipline every prior slice used. Went with the recommended
default from the pre-slice requirements report rather than the "both shapes now" alternative (the
way P/T got `SetPT`+`ModifyPT` together) — additive is the dominant real-Magic case and keeps this
slice's surface the same size as layer-engine-basics' single-shape-per-layer 6/7c additions.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 6 new tests in
`crates/engine/tests/phase5_layer_engine_types_colors.rs` (base case mirrors `CardDefinition`,
`AddCardType` unions a type onto a land without losing Land, `AddColor` unions a color onto a
white creature without losing White, `EffectFilter` controller-scoping still applies to type
effects, a plain land is untouched by the lethal-damage SBA, and the real-consumer pair: a land
animated into a creature by `AddCardType`+`SetPT` actually dies to lethal marked damage), all 189
prior tests unaffected. 195 tests total, 0 failed. Confirmed via the real CLI too: `cargo run -p
stackfish-cli -- crates/engine/tests/scenarios/phase5_layer_engine_types_colors.toml` — 1/1 action
ran, 2/2 assertions passed (an artifact granting Creature type plus a second artifact setting 3/3,
both filtered onto any Land, animated a land that then died to 3 pre-seeded marked damage — CR
704.5g firing on an object that was never a creature by `CardDefinition` at all).

**What got implemented:**
- `types/effect.rs`: `EffectPrimitive` gained `AddCardType(CardTypeSet)` (layer 4) and
  `AddColor(ColorSet)` (layer 5), same union-only shape as `AddKeyword`. New TODO on the enum
  itself for the deferred `SetCardTypes`/`SetColors` overwrite shapes and for supertypes/subtypes
  (still untouched — `EffectiveCharacteristics.supertypes`/`.subtypes` stay a straight
  `CardDefinition` copy). `Layer`'s own doc comment updated: layers 4/5 moved from "TODO, unbuilt"
  to "real," with a note that CR 613.3's lettered sublayers don't apply to layers 4/5 at all (only
  layers 3/4/6/7 have any, and layer 4's own lettered split is about control-changing-vs-not, not
  modeled here) — a single primitive per layer is faithful to the rule, not a simplification.
- `layers.rs::effective_characteristics`: two more `applicable_effects_in_layer` passes,
  `Layer::TypeChanging` unioning `AddCardType` into a new `card_types` local and
  `Layer::ColorChanging` unioning `AddColor` into the existing `colors` local (previously computed
  inline in the return struct; pulled into a local so layer 5 has something to union onto). Slotted
  in before the existing layer 6/7b/7c passes, matching CR 613.1's real layer numbering (4, 5, 6,
  7b, 7c) for readability — apply order doesn't matter for correctness since these touch different
  characteristics than 6/7b/7c. The pre-existing `color_indicator`-as-stopgap comment (colors read
  directly rather than through the still-`todo!()` `CardDefinition::colors()`) carried forward
  unchanged — layer 5's union applies on top of whatever that stopgap produces, not blocked on
  fixing it.
- `sba.rs`'s `apply_creature_death`: real consumer. `candidates` no longer pre-filters by
  `CardTypeSet::CREATURE` off `CardDefinition` — it now takes every battlefield object, and the
  `CREATURE` check moved inside the `dying` filter against `effective.card_types` (one
  `effective_characteristics` call per candidate instead of two, as a side benefit of the
  restructure). A land with no continuous effects is filtered out here exactly as it always was;
  a land with an `AddCardType(CREATURE)` effect (plus a `SetPT` for its P/T, since a land has no
  base toughness `PTValue` to net) now passes the type check and is subject to the rest of
  704.5f/g, same as any printed creature.
- `commands.rs`: one new TODO-only comment (no behavior change) on `PlayLand`'s land-type check,
  explaining why it wasn't retrofitted this slice — that check reads a card still in
  `ZoneKind::Hand`, and every real consumer built so far (this slice's SBA type check, the
  consumers slice's combat/blocking checks) targets a battlefield permanent; wiring a hand-zone
  card through `effective_characteristics` untested felt like guessing at a shape rather than
  building the flagged gap for real.
- `crates/scenario/src/lib.rs`: `ContinuousEffectSpec` gained `types: Vec<String>` and
  `colors: Vec<String>` fields (both `type_changing`/`color_changing`-only, same
  default-empty-vec convention as `keywords`). `build_continuous_effect` gained matching arms
  (`"type_changing"` → `Layer::TypeChanging` + `AddCardType(parse_card_types(&spec.types))`,
  `"color_changing"` → `Layer::ColorChanging` + `AddColor(parse_colors(&spec.colors))`), reusing
  the existing `parse_card_types` helper (already used by `filter_card_types`) and a new
  `parse_colors` helper (reuses the existing per-letter `parse_color`, unioned into a `ColorSet` —
  `parse_color` itself stays single-`Color`-returning since `CardSpec.colors`/`ManaPaymentSpec`
  callers only ever need one color at a time per entry). `docs/scenario_format.md` documents both
  new fields and points at the new TOML fixture, same pattern every prior scenario-crate-adjacent
  slice has followed.

**STRUCT DEVIATIONS:** None beyond what the pre-slice requirements report scoped exactly
(`EffectPrimitive::AddCardType`/`AddColor`) — this *is* the plan, not a deviation from it.

**INTERFACE DEVIATIONS:** None — `effective_characteristics(state, object)`'s signature is
unchanged, same as the consumers slice found for its own two consumers.

**SCOPE DEVIATIONS:**
- `sba.rs`'s `apply_creature_death` restructuring `candidates`/`dying` into a single
  `effective_characteristics` call per object (down from a raw-definition prefilter plus a second
  per-candidate call) wasn't named in the pre-slice report as a required change — it fell out
  directly from moving the type check onto the effective value, judged in-scope as the same
  function's minimal correct shape rather than a separate cleanup slice.
- `commands.rs`'s `PlayLand` TODO-only comment — anticipated in the pre-slice discussion as the
  explicit trade-off of picking `sba.rs` as this slice's one real consumer over `PlayLand`.

**UNRESOLVED (anticipated, none new):**
- `SetCardTypes`/`SetColors` (full overwrite) — TODO on `EffectPrimitive`.
- Supertypes/subtypes (e.g. "becomes legendary", gaining a subtype) — TODO on `EffectPrimitive`,
  `EffectiveCharacteristics.supertypes`/`.subtypes` still a straight `CardDefinition` copy.
- `CardDefinition::colors()`/`ManaCost::colors()`/`ManaSymbol::colors()` — all three still
  `todo!()`; `effective_characteristics.colors` still reads `color_indicator` directly as a
  stopgap, unchanged from the layer-engine-basics slice.
- `PlayLand`'s land-type check — flagged in `commands.rs` itself now, not just here; a continuous
  effect granting Land type to a card still in hand wouldn't make it playable.
- Layers 1/2/3, 7a, 7d(-as-a-real-layer-step), 7e, and the CR 613.6-8 dependency system — each has
  its own TODO on `Layer` itself, untouched by this slice.

## Phase 5: phase5/layer-engine-cda slice

Branch `phase5/layer-engine-cda`, off master. Slice backlog item 1's sub-slice C. Scope locked in
with the human before writing code: CR 613 layer 7a (characteristic-defining power/toughness)
resolution only. `AffectedFilter::Script`/`EffectPayload::Script` (continuous effects backed by a
script rather than a primitive) stay dead code — the backlog's own text suggested pairing them in
since "the script-runtime plumbing arrives in the same slice," but they're a structurally
different Lua calling convention (a boolean affects-predicate plus an effect-shape-decoding
script, vs. this slice's single numeric CDA query) and a real design question of their own;
bundling both risked the exact scope creep every prior slice has deliberately avoided. Split into
backlog item 3a, named explicitly rather than left implicit.

**Correction to the backlog, found while scoping this one:** checked `script.rs` before writing
code — neither `invoke_condition` nor `invoke_operation` actually passes `GameState` into the Lua
call (`ScriptContext.state` is Rust-side plumbing only, never bound into a `Game.*` function a
script could call). A genuine "count cards in your graveyard"-style CDA (the flavor of the real
backlog motivation, Tarmogoyf-style cards) isn't buildable with the script bridge as it stands —
that needs `Game.*` state-query bindings, which is slice backlog item 8 (Script API expansion),
not this slice. `invoke_cda` was scoped down to match `invoke_operation`'s existing calling
convention (`(source, controller) -> value`, no state channel) rather than inventing a new one,
and every test CDA is a constant or id-derived value — same limitation `invoke_condition`'s own
existing tests already live with, not a new gap this slice introduced.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 8 new tests in
`crates/engine/tests/phase5_layer_engine_cda.rs` (no script still resolves to `None`, a
constant-returning script resolves to a real number, a CDA base is overridden by a later `SetPT`
not stacked with it, a CDA base has `ModifyPT` stacked on top of it, a CDA base nets against
`+1/+1` counters, an erroring script resolves to `None` rather than panicking, and the real-consumer
pair: a script-less CDA creature never dies to the lethal-damage SBA regardless of damage marked,
a scripted one does), all 195 prior tests unaffected. 203 tests total, 0 failed. Confirmed via the
real CLI too: `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_layer_engine_cda.toml`
— 1/1 action ran, 2/2 assertions passed (a creature with no printed toughness, only a scripted
`return 3`, died to 3 pre-seeded marked damage).

**What got implemented:**
- `types/card.rs`: `PTValue::CharacteristicDefining` changed from a bare `String` (display text
  only) to `{ display: String, script: Option<ScriptReference> }` — mirrors
  `CardDefinition.script_ref`'s own `Inline`/`ByKey` split rather than a pre-loaded
  `ScriptHandle`, keeping `PTValue` plain runtime-independent data same as `CardDefinition`
  itself. `None` (no script) behaves identically to before this slice — `as_fixed`/
  `net_with_counters` are unchanged, still unconditionally `None` for any `CharacteristicDefining`
  regardless of the new field, since CDA resolution needs `&GameState`/`&ScriptRuntime` neither
  method has access to. Extracted `net_counters(base: i32, counters: &CounterSet) -> i32` (CR
  121.3's counter-netting formula) as a shared `pub(crate)` fn — `net_with_counters` now calls it
  for a `Fixed` base, `layers.rs`'s new CDA path calls it for a script-resolved base, so the
  formula lives in exactly one place.
- `script.rs`: new `ScriptRuntime::invoke_cda(&self, source_code, source, controller) ->
  Result<i32, ScriptError>` — evaluates `source_code` fresh every call (no `ScriptHandle`/registry
  entry, same one-shot-eval convention `run_card_script` already uses for `Inline` sources),
  expects `function(source, controller) -> integer`, same argument shape `invoke_operation`
  already uses. `&self`, not `&mut self` — `Lua::load`/`.eval()`/`.call()` only need `&Lua`.
- `layers.rs::effective_characteristics`: gained a `&ScriptRuntime` parameter. New private
  `resolve_base_pt` helper resolves layer 7a (CR 613.4a) before layers 7b/7c apply — a `Fixed`
  value (or a `CharacteristicDefining` with no resolvable script) delegates straight to the
  unchanged `PTValue::net_with_counters`; a `CharacteristicDefining` with an `Inline` script gets
  `invoke_cda` called for the base, then netted against counters via the shared `net_counters`. A
  script that errors, or a `ByKey` reference (not resolvable by this engine, same stance
  `run_card_script` already takes), falls through to the same `None` as before this slice — "skip
  rather than guess" stays infallible rather than making the whole query (and every caller) return
  a `Result`. `Layer`'s doc comment updated: 7a moved from "not resolved" to real, with an explicit
  note on why `AffectedFilter::Script`/`EffectPayload::Script` are *still* not evaluated despite a
  `&ScriptRuntime` now being in scope (see the "Correction" above).
- **Ripple, threaded through as planned:** `commands::execute_command` and its two private helpers
  (`effective_power`/`effective_keywords`) gained a `&ScriptRuntime` parameter, threaded to all 7
  internal call sites (Flying/Reach/Menace blocking checks, combat-damage power reads,
  Lifelink/Wither/Infect/Deathtouch keyword reads). `sba::check_state_based_actions` and
  `apply_creature_death` gained the same parameter. `engine.rs`'s 3 call sites (one
  `execute_command`, two `check_state_based_actions`) now pass `&self.script_runtime` — `Engine`
  already owned one, no new field needed. Side effect worth calling out: `commands.rs`'s
  `effective_power` (combat-damage power, wired since layer-engine-consumers) now resolves a
  scripted CDA creature's power too, for free — no separate consumer wiring needed, same "the
  query gets more capable, existing consumers benefit automatically" pattern the types-colors
  slice already established for its two consumers.
- `crates/scenario/src/lib.rs`: `CardSpec` gained `power_script`/`toughness_script: Option<String>`
  (raw Lua source, same convention `trigger_condition`/`trigger_operation` already use). A script
  wins over a plain `power`/`toughness` i32 if both happen to be set — not rejected, "not a
  supported combination" same stance `CardSpec.script`'s own doc comment already takes for its
  overlap with `trigger_condition`/`trigger_operation`. New `build_pt` helper centralizes the
  `Fixed`-vs-`CharacteristicDefining` choice, used by both `build_initial_state` and
  `build_new_game_engine`'s card-construction paths (parity, same pattern every prior
  scenario-crate-adjacent slice has followed). `docs/scenario_format.md` documents both fields and
  points at the new TOML fixture.

**STRUCT DEVIATIONS:**
- `PTValue::CharacteristicDefining` changing shape (tuple variant → struct variant with a new
  `script` field) was the pre-slice report's own headline proposal, not a deviation from it — but
  it did require touching 3 existing test call sites mechanically
  (`phase5_keyword_primitives.rs`, `phase5_layer_engine_basics.rs` ×2, `phase5_sba_basics.rs`),
  exactly as flagged as expected churn in the report.

**INTERFACE DEVIATIONS:** None beyond what the pre-slice report explicitly flagged as expected
ripple — `effective_characteristics`/`execute_command`/`check_state_based_actions` all gained a
`&ScriptRuntime` parameter, anticipated by name in the report's "Ripple" section, not discovered
mid-implementation.

**SCOPE DEVIATIONS:**
- Every existing Rust unit test that calls `effective_characteristics`/`check_state_based_actions`
  directly (`phase5_layer_engine_basics.rs`, `phase5_layer_engine_types_colors.rs`,
  `phase5_sba_basics.rs`, `phase5_script_self_registration.rs` — 24 call sites total) needed a
  `ScriptRuntime::new()` constructed and passed through — anticipated by name in the report's
  "Ripple" section as expected mechanical churn, not a surprise.
- `commands.rs`'s and `sba.rs`'s doc comments updated in a few places to stop saying CDA
  power/toughness "can't be resolved without the script bridge" now that it sometimes can —
  accuracy upkeep, not new behavior.

**UNRESOLVED (anticipated, none new):**
- `AffectedFilter::Script`/`EffectPayload::Script` — still dead code, now backlog item 3a by name
  instead of implicitly bundled into this slice.
- `Game.*` state-query bindings (backlog item 8) — the real blocker for any CDA that reads actual
  board state (Tarmogoyf, etc.) rather than a constant; this slice's calling convention
  (`invoke_cda`) is ready for it once that binding exists, no further signature change expected.
- CR 604.3's "functions outside the game / in all zones" — `effective_characteristics` still only
  meaningfully queried for battlefield objects; not attempted.
- `ScriptContext`'s hidden-information leak (`state: &GameState` full access) — pre-existing,
  unrelated, untouched.
- Layers 1/2/3, 7d(-as-a-real-layer-step), 7e, and the CR 613.6-8 dependency system — each still
  has its own TODO on `Layer` itself.

## Phase 5: phase5/targeting-basics slice

Branch `phase5/targeting-basics`, off master (merged). Slice backlog item 2. Scope: CR 115/
601.2c-e/608.2b — a card/ability declares a fixed-count, primitive-filter `TargetRequirement`;
`CastSpell`/`ActivateAbility`'s non-mana arm validate count/filter/dedup before pushing to the
stack (an illegal proposed cast never happens, CR 601.2e); `ResolveStackTop` rechecks every target
at resolution and fizzles (CR 608.2b) if all are now illegal.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 211 tests total (203 prior + 8 new in
`phase5_targeting_basics.rs`), 0 failed. One pre-existing test
(`phase5_activate_ability_non_mana.rs`'s `targets_are_accepted_and_stored_without_legality_check`)
explicitly asserted the old no-validation stub behavior — rewritten to
`unrequested_target_is_rejected` since the behavior it tested is now correctly illegal, same
"fixture written against a stale baseline" pattern flagged in earlier slices. Confirmed via the
real CLI too: `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_targeting_basics.toml`
— 6/6 actions ran, 3/3 assertions passed (an "any target" instant resolves normally against a
still-legal battlefield creature; the reject/fizzle paths stay Rust-test-only, not forced through
TOML).

**What got implemented:**
- `types/target.rs` (new): `TargetFilter` (`Object{card_types, controller, zone}` / `Player` /
  `AnyTarget`), `TargetRequirement` (filter + exact `count`), `TargetSpec` (`Primitive`/`Script`,
  same split every other filter-shaped type in this codebase uses — `Script` stays dead code,
  nothing constructs it, same stance `ContinuousEffect::applies_to`'s own `Script` arm already
  takes).
- `CardDefinition.target_requirement: Option<TargetSpec>` (new field — `None` means CR 601.2c's
  "cast as though it did not require those targets"). `RegisteredAbility.target` retyped
  `Option<ScriptHandle>` -> `Option<TargetSpec>` (every existing construction site was already
  `None`, purely mechanical).
- `StackObject.target_requirement` — baked in at push time from whichever produced the
  `StackObject` (spell's `CardDefinition` or ability's `RegisteredAbility`), same "primitive
  stand-in decided once at push" contract `effect` already established. Threaded through all 3
  `StackObject::new` call sites (`CastSpell`, `ActivateAbility`, `PutTriggersApnap` — triggered
  abilities don't choose targets yet, see UNRESOLVED, but the field is there for when they do).
- `commands::target_is_legal`/`validate_targets` (new, `pub(crate)`) — the actual CR 115 legality
  check (zone default battlefield per 115.2, card-type/controller filter via
  `layers::effective_characteristics`, "any target" per 115.4) plus count/dedup (115.3) validation.
  Called in `CastSpell` and `ActivateAbility`'s non-mana arm before any state mutation.
- `engine.rs`: new `GameEventKind::Fizzled { stack_id }`. `ResolveStackTop` now rechecks
  `top.target_requirement` against `top.targets` before doing anything else; if a real (count > 0)
  requirement's every target is now illegal, it fizzles — no effect/script runs, and (unlike normal
  CR 608.2m routing) a spell is forced to its owner's graveyard regardless of card type. Refactored
  the zone-move logic that both the normal-resolution and fizzle paths need into a shared
  `Engine::move_resolving_spell(stack_id, destination, run_script)` helper rather than duplicating
  it.
- `crates/scenario/src/lib.rs`: `CardSpec.target_requirement: Option<TargetRequirementSpec>` +
  `build_target_requirement` helper, wired into both `build_initial_state`/`build_new_game_engine`.
  Hit a real naming collision — the scenario crate already has its own unrelated `TargetSpec`
  (TOML target-reference enum, `Player(u32)`/`Card(String)`, used by `AttackerSpec`/`CastSpell`
  actions) that shadows the glob-imported engine `TargetSpec` by Rust's name-resolution rules
  (explicit local item beats glob import) — fixed by fully-qualifying `stackfish_engine::TargetSpec`
  at its one construction site, documented inline so it isn't miscopied later.
- `docs/scenario_format.md` — new `target_requirement` section + worked example, pointing at the
  new `phase5_targeting_basics.toml` fixture.

**STRUCT DEVIATIONS:** None beyond what the pre-slice requirements report scoped exactly.

**INTERFACE DEVIATIONS:** `StackObject::new` gained one parameter (`target_requirement`) —
anticipated by the "baked at push time" plan, same pattern `effect` already established.

**SCOPE DEVIATIONS:**
- `RegisteredAbility.target`'s type change touched ~23 construction sites across the engine crate
  and its tests — all were already `None`, purely mechanical churn (verified via diff), same
  "expected ripple" pattern the CDA slice's `&ScriptRuntime` threading already established.
- Scenario/TOML support (`CardSpec.target_requirement`, the TOML fixture, the docs section) wasn't
  in the original test list — added as a follow-up at the human's request after the first
  deviation report, to keep parity with every other slice's Rust-test-plus-TOML-fixture-plus-CLI-
  confirmation convention.

**UNRESOLVED (anticipated, none new):**
- No "up to N"/variable target counts (CR 601.2c/115.6) — `TargetRequirement.count` is exact only.
- No multiple distinct "target" words per spell/ability — one `TargetRequirement` per
  spell/ability, not a list.
- No partial fizzle (CR 608.2b's "illegal targets aren't affected, other parts still are") — no
  targeted effect primitive exists yet (only untargeted `GainLife`) to even exercise this against;
  only the all-or-nothing fizzle path is implemented.
- `TargetSpec::Script` — structurally present, dead code; a script-backed target-legality predicate
  needs its own "Lua asks, Rust decides" convention, not designed yet (same shape of gap as backlog
  item 3a's continuous-effect `Script` arm).
- CR 603.10 last-known-information for a target whose source already left — pre-existing gap,
  untouched.
- Triggered abilities can't choose targets yet (`PutTriggersApnap` always pushes `targets:
  Vec::new()`) — `target_requirement` is baked on regardless for when that lands, but nothing
  populates `targets` for a `TriggeredAbility` `StackObject` today.
- CR 601.2d (division/distribution among targets), splice (601.2b/702.47) — pre-existing gaps,
  untouched.

## Phase 5: phase5/static-ability-self-registration slice

Branch `phase5/static-ability-self-registration`, off master. Slice backlog item 3. Scope: real
card-script self-registration for static abilities (CR 604/611) — a card's own Lua
`initial_effect(c)` calling `Game.CreateStaticAbility(c)` to attach a real `ContinuousEffect` to
`GameState.continuous_effects`, exact mirror of the already-real
`Game.CreateTriggeredAbility`/`TriggeredAbilityBuilder` mechanism (same queued-registration fix
for the "Lua callback can't hold `&mut GameState`" gap). Building a `ContinuousEffect` by hand
(Rust struct literal / TOML `[cards.continuous_effect]`) remains fully supported, unaffected.

**Two design decisions locked in before writing code** (both confirmed in the pre-slice
requirements report):
- **Timestamp allocated at drain time, not builder time.** The builder has no `&mut GameState` to
  call `allocate_sequence_no()` from inside a Lua callback — `run_card_script`'s existing drain
  step (already runs for `pending_triggered`) stamps each queued `ContinuousEffect.timestamp` right
  before appending it to `state.continuous_effects`.
- **`retire_object` also purges `state.continuous_effects`.** A static effect's source leaving the
  battlefield already made it inert (`layers::applicable_effects_in_layer` skips a source that
  doesn't resolve) but never removed the dead entry — same pileup gap `retire_object` was built to
  close for `state.abilities`, extended here (one more `.retain()` line, same 3 existing call
  sites: discard, `draw_step`, SBA death).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 8 new tests in
`crates/engine/tests/phase5_static_ability_self_registration.rs`, plus the existing
`retire_object_purges_only_the_retired_source` unit test extended (not just left alone) to also
cover the `continuous_effects` purge, all 211 prior tests unaffected. 219 tests total, 0 failed.
Confirmed via the real CLI too: `cargo run -p stackfish-cli --
crates/engine/tests/scenarios/phase5_static_ability_self_registration.toml` — 15/15 actions ran,
5/5 assertions passed (an anthem land's self-registered "creatures you control get +1/+1" static
ability made a vanilla 2/2 bear deal 3 combat damage instead of 2 — the layer-engine consumer
wired two slices ago picks up a script-registered effect automatically, no new consumer code
needed).

**What got implemented:**
- `script.rs`: `LayerTag`/`EffectPrimitiveTag` (new `mlua::UserData` tag wrappers, same pattern
  `ConditionTag`/`OperationTag`/`CostTag` already use), `decode_layer_value`/`decode_effect_value`
  (only a `Game.Layers.*`/`Game.Effects.*` tag accepted — no custom-function fallback, since
  neither `Layer` nor `EffectPrimitive` has a `Script` escape hatch of their own the way
  condition/operation do), `decode_affected_table` (a plain Lua table
  `{controller=.., card_types={..}, self_only=..}`, every key optional — matches how
  `EffectFilter`/the scenario TOML's `ContinuousEffectSpec` already read as a flat option bag,
  less new surface than a `Game.Filters.*` tag-factory family), and
  `validate_layer_effect_pairing` (rejects e.g. `SetLayer(TypeChanging)` +
  `SetEffect(ModifyPT(..))` at `:Register()` time with a real error, rather than silently building
  a `ContinuousEffect` `layers::effective_characteristics` could never actually apply). Small
  local `parse_card_type_name`/`parse_keyword_name`/`parse_color_letter`/
  `parse_filter_controller_name` helpers — duplicated from the scenario crate's equivalents rather
  than shared (separate crate, no common string-parsing utility today).
  `StaticAbilityBuilder` (new `mlua::UserData`, mirrors `TriggeredAbilityBuilder` exactly) with
  `:SetLayer`/`:SetEffect`/`:SetAffected`/`:Register()`. `ScriptRegistry` gained
  `pending_static: Vec<ContinuousEffect>` alongside `pending_triggered`. `bind_game_globals` gained
  `Game.Layers` (only the 5 layers with a real query path — `TypeChanging`/`ColorChanging`/
  `AbilityAdding`/`PowerToughnessSetting`/`PowerToughnessModifying`, matching `Layer`'s own doc
  comment for what's built), `Game.Effects` (`SetPT`/`ModifyPT`/`AddKeyword`/`AddCardType`/
  `AddColor`, one factory function per `EffectPrimitive` shape, same convention `Game.Ops.GainLife`
  already established), and `Game.CreateStaticAbility`. `run_card_script` now drains
  `pending_static` into `state.continuous_effects` (stamping real timestamps) right after draining
  `pending_triggered` into `state.abilities` — same call, no new call sites.
- `commands.rs`: `retire_object` extended per the design decision above; its own doc comment
  updated to describe both purges. `retire_object_purges_only_the_retired_source` (unit test)
  extended with a `dummy_continuous_effect` helper and matching assertions.
- `docs/lua_scripts.md`: new "Self-registering a static ability" section (worked anthem example,
  `Game.Layers`/`Game.Effects` reference, the layer/effect-pairing validation, the
  timestamp-at-drain-time note) mirroring the triggered-ability section's structure. "Not available
  yet" section updated — `Game.CreateActivatedAbility` is now the only missing builder;
  `Game.AddContinuousEffect` reclassified as "superseded by `Game.CreateStaticAbility`," not merely
  unbound; `AffectedFilter::Script`/`EffectPayload::Script` called out by name as backlog item 3a.
- `docs/scenario_format.md`: `script`'s doc paragraph points at the new static-ability section/
  fixture; `continuous_effect`'s doc paragraph no longer calls itself a stand-in for a "future
  slice" that's now done — reframed as a plain-Rust-struct convenience path alongside the real
  script one.

**STRUCT DEVIATIONS:** None beyond what the pre-slice requirements report scoped exactly
(`ScriptRegistry.pending_static`, `StaticAbilityBuilder`, the two tag wrappers) — this *is* the
plan, not a deviation from it.

**INTERFACE DEVIATIONS:** None — `run_card_script`'s signature is unchanged; it just does more
inside the same call.

**SCOPE DEVIATIONS:**
- `retire_object`'s extension to `state.continuous_effects` was explicitly flagged and confirmed
  in the pre-slice report before writing code, not discovered after the fact — same "surfaced and
  confirmed with the human before implementing" pattern the original `retire_object` design used
  for its own 3 call sites.
- `docs/scenario_format.md`'s `continuous_effect` paragraph rewrite — anticipated (it explicitly
  referenced "static-ability self-registration" as a future slice; that reference had to be
  updated regardless of anything else touched this slice).

**UNRESOLVED (anticipated, none new):**
- `Game.Layers` only exposes the 5 real layers — layers 1/2/3/7d/7e can't be requested by a static
  ability at all, matching `Layer`'s own doc comment.
- `AffectedFilter::Script`/`EffectPayload::Script` (backlog item 3a) — still dead code; a
  script-backed target/affects predicate needs its own "Lua asks, Rust decides" convention, not
  designed yet. `Game.Layers`/`Game.Effects`/`SetAffected`'s table shape don't touch this at all —
  they only ever build the `Primitive` case.
- Always `EffectDuration::Permanent` — no "until end of turn" static-generated effect (CR 611.3a-d's
  timing nuances around "may play/cast" effects untouched).
- No conditional static abilities (CR 606's "as long as you control a Forest"-style gating) —
  always active whenever the source is on the battlefield, no extra condition gate.
- `Game.CreateActivatedAbility` — the one remaining builder sibling; activated (non-mana)
  abilities are still wired only through the pre-existing `AbilityCost::Tap`/`cost: Some(Tap)` path
  (backlog item 4).

## Phase 5: phase5/activated-ability-costs slice

Branch `phase5/activated-ability-costs`, off master. Slice backlog item 4. Scope: CR 602
generalizes an activated ability's cost off the one-shot `AbilityCost::Tap` shortcut onto the same
`CostSpec`/`PaymentPlan` model `CastSpell` already uses (CR 602.2b: "identical to the process for
casting a spell") — `Sacrifice`/`Discard`/`TapPermanents`/`LifePayment` additional costs, in any
combination, plus `Game.CreateActivatedAbility` so a card script can build one itself.

**Design decisions locked in before writing code** (all confirmed in the pre-slice requirements
report):
- `AbilityCost` enum retired entirely. `RegisteredAbility.cost: Option<AbilityCost>` ->
  `Option<CostSpec>`; the old `Tap` variant becomes `AdditionalCost::TapSelf` (distinct from
  `TapPermanents{filter, count}`, which taps *other* permanents). Mana abilities (CR 605) are
  untouched — they never read `.cost` at all, only the mana-ability arm's own hardcoded
  tap-and-add-mana logic.
- Non-mana `ActivateAbility` routes through `AwaitingPayment`, same as `CastSpell`, even for a bare
  `{T}` cost — the tap (or sacrifice/discard/life deduction) is a payment-commit-time side effect,
  not something `execute_command` does synchronously anymore. Every pre-existing Tap-only
  activated-ability test gained a `submit_payment` step.
- `FilterSpec::matches` gained a required `viewer: PlayerId` param and a real body (`card_types`/
  `controller` checked for real; `random` doesn't affect matching; `extra` is treated as "doesn't
  match," never guessed at, since resolving it needs the script bridge which isn't wired to this
  call).
- `PaymentPlan::validate` gained `&GameState`/`payer: PlayerId`/`source: ObjectId` params. `source`
  is new — needed so `AdditionalCost::TapSelf` can confirm a `CostPayment::Tap` payment taps
  *that* object specifically, not some other one the payer happens to control. Validation is
  positional (`self.payments` mirrors `spec`: an optional `Mana` entry, then one entry per
  `spec.additional` in order, `TapPermanents`'s `count` consuming that many consecutive `Tap`
  entries) — that ordering is what disambiguates two same-shaped `Tap` payments (one for
  `TapSelf`, one for a `TapPermanents` component) from each other. A new `commands::commit_payment`
  performs the actual side effect (tap/move-to-graveyard/deduct-life) once validated — it doesn't
  need the same positional ordering, since each `CostPayment` variant is self-describing regardless
  of which requirement it happened to satisfy.
- `Game.CreateActivatedAbility`'s cost builder accumulates via `:AddCost(tag)` called once per
  component (`Game.Costs.TapSelf`/`Sacrifice(filter)`/`Discard(filter)`/`TapPermanents(filter,
  count)`/`Life(n)`), not a single `:SetCost` — a real activation can need more than one component
  at once. Mana-cost components on an activated ability's cost stay out of scope (no Lua-facing
  mana-cost-string parser exists anywhere yet, same boundary `CastSpell`'s own cost already has).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 236 tests total (219 prior + 3 new
`FilterSpec::matches` unit tests in `types/cost.rs` + 9 new tests in
`phase5_activate_ability_costs.rs` + 5 new tests in
`phase5_activated_ability_self_registration.rs`), 0 failed. One pre-existing test file
(`phase5_activate_ability_non_mana.rs`) updated in place (see SCOPE DEVIATIONS) rather than counted
as new. Not confirmed via the CLI/TOML harness this slice — see UNRESOLVED.

**What got implemented:**
- `types/cost.rs`: `FilterSpec` gained `Clone, Debug` and a real `matches(&self, object, viewer)`
  body. `AdditionalCost`/`AlternativeCost`/`CostSpec` gained `Clone, Debug` (needed to
  `ability.cost.clone()` the same way `ability.effect.clone()` already does).
  `AdditionalCost::TapSelf` added. `PaymentPlan::validate`'s signature and body rewritten per the
  design decisions above — real per-component validation for every `AdditionalCost` shape,
  `AdditionalCost::Other` and `spec.alternative` still unconditionally rejected (no `CostPayment`
  shape can express an unstructured-text cost; alternative costs stay out of scope). New
  `#[cfg(test)]` module: 3 unit tests for `FilterSpec::matches` (card_types true/false, controller
  you/opponent true/false, `extra` always failing rather than guessed).
- `types/ability.rs`: `AbilityCost` enum deleted. `RegisteredAbility.cost: Option<CostSpec>`.
- `commands.rs`: `ActivateAbility`'s non-mana arm rewritten — validates targets, then (if the
  cost's `additional` contains `TapSelf`) pre-checks "not already tapped"/"not summoning sick"
  against the *declared* cost before pushing anything (an illegal activation never happens, same
  stance `validate_targets` already takes), pushes the `StackObject` with no tap yet, and returns
  `requires_payment: Some(PendingPaymentRequest { .. })` the same way `CastSpell` does. New
  `PendingPaymentRequest.source: ObjectId` field (the object whose activation/cast this cost
  belongs to — a spell's own `stack_id` as a harmless placeholder, since a spell's `CostSpec` never
  contains `TapSelf`). New `pub(crate) fn commit_payment` — performs the real side effect of every
  `CostPayment` in a validated plan: `Mana` deducts via the existing `cost::spend`, `Tap` sets
  `tapped = true` (+ `Tapped` event), `Sacrifice`/`Discard` move the object to the graveyard via a
  new private `move_to_graveyard_for_cost` helper (mints a new id per CR 400.7, purges via the
  existing `retire_object`, same pattern `Engine::submit_discard`/`draw_step` already established),
  `Life` deducts via `PlayerState::lose_life` (+ `LifeChanged` event).
- `engine.rs`: `ProcessState::AwaitingPayment`/`commands::PendingPaymentRequest` both gained
  `source: ObjectId`, threaded through the two construction sites and `submit_payment`.
  `submit_payment`'s commit step now calls `commands::commit_payment` for every payment (mana
  included) instead of only special-casing `CostPayment::Mana` inline — `cost::spend`'s call site
  moved into `commands.rs` accordingly. `StepStatus::AwaitingPayment` (the public-facing status) is
  unchanged — the host doesn't need `source` to prompt the payer, it already knows what it
  activated.
- `script.rs`: `CostTag` now wraps `AdditionalCost` (was `AbilityCost`). New
  `decode_additional_cost_value`/`decode_filter_table` (mirrors `decode_affected_table`'s plain-
  table-of-optional-keys convention). `Game.Costs` rebuilt: `TapSelf` (plain tag),
  `Sacrifice(filter)`/`Discard(filter)`/`TapPermanents(filter, count)`/`Life(n)` (factory
  functions) — the old bare `Tap` tag is gone (retired along with `AbilityCost`). New
  `ActivatedAbilityBuilder` (mirrors `TriggeredAbilityBuilder`/`StaticAbilityBuilder`): `:AddCost`
  accumulates onto `CostSpec.additional`, `:SetOperation` mirrors `TriggeredAbilityBuilder`'s own
  setter, `:Register()` queues onto the same shared `pending_triggered` list the other two builders
  use (the drain into `state.abilities` doesn't care about `AbilityKind`, so no new queue needed).
  `Game.CreateActivatedAbility` bound alongside the other two constructors. `ScriptRuntime::
  load_cost` (unused anywhere, returned the now-gone `AbilityCost`) deleted rather than repointed.
- `docs/lua_scripts.md`: new "Self-registering an activated ability" section (worked `{T}: Gain 1
  life` example, combined-cost example, `Game.Costs.*` reference). "Not available yet" section
  updated — `Game.CreateActivatedAbility` is no longer listed as missing; the stale "Activated
  (non-mana) abilities" bullet (left over from *before* that slice's own doc update, already
  inaccurate) replaced with the real remaining gaps (`{X}`/alternative/hybrid-etc costs,
  script-backed cost/target predicates). The condition/cost/target/operation intro section's claim
  that cost "exists as an `Option<ScriptHandle>` field but is never invoked" corrected — cost is
  real now, just never a Lua-function callback (no `Script` fallback shape exists for it).

**STRUCT DEVIATIONS:** None beyond what the pre-slice requirements report scoped exactly
(`AdditionalCost::TapSelf`, `RegisteredAbility.cost: Option<CostSpec>`, `PendingPaymentRequest.
source`) — this *is* the plan, not a deviation from it.

**INTERFACE DEVIATIONS:** None beyond what the report's own design-lock-in section named —
`PaymentPlan::validate`'s new params and `FilterSpec::matches`'s new `viewer` param were both
anticipated by name before writing code.

**SCOPE DEVIATIONS:**
- `phase5_activate_ability_non_mana.rs`'s two success-path tests rewritten (not just extended) to
  pause on `AwaitingPayment` and pay via a new `pay_tap_and_drain` helper (mirrors `CastSpell`'s own
  `pay_free_and_drain`) — anticipated by name in the pre-slice report ("every existing Tap-only
  activated-ability test gets a SubmitPayment step added"). The three purely-rejecting tests
  (already-tapped, summoning-sick, wrong-controller) needed no changes — those checks stayed at
  `execute_command` time, before any push, unchanged from before this slice.
- `docs/lua_scripts.md`'s intro section correction (cost no longer "never invoked") — accuracy
  upkeep surfaced while writing the new section, not named in the pre-slice report.

**UNRESOLVED:**
- `crates/scenario` (the TOML scenario crate) — `ActionSpec::SubmitPayment`/`ManaPaymentSpec` only
  build a `PaymentPlan` with a `Mana` (or empty) payment; there's no TOML shape for
  `Sacrifice`/`Discard`/`Tap`/`Life` payments yet, so no CLI/TOML fixture was added or run this
  slice (every other slice's own verification includes one) — flagged explicitly rather than
  silently skipped. Extending `ManaPaymentSpec`-equivalent TOML support for additional-cost
  payments is a real, undone follow-up if TOML-driven coverage of this cost shape is wanted.
- Multi-ability-per-card stable indexing — still positional by registration order
  (`ability_index: u32`), exactly as fragile as before this slice; explicitly out of scope (see
  backlog item 4's own note).
- `{X}` costs, alternative costs (flashback-style), hybrid/Phyrexian/snow mana payment, cost
  reduction/increase, mana-cost components on an activated ability's own cost — all pre-existing
  `CostSpec`/`PaymentPlan` gaps, not re-litigated here (backlog item 6).
- `:SetTarget` not bound on `ActivatedAbilityBuilder` — a script-registered activated ability is
  targetless, same "nothing built yet to consume it" stance `TriggeredAbilityBuilder`'s own
  `:SetTarget` omission already takes.
- `ScriptRuntime::invoke_cost` still `todo!()` — no cost primitive needs a script-bridge fallback
  yet; every `Game.Costs.*` shape resolves to a real `AdditionalCost` with zero Lua calls at
  activation/payment time.

## Phase 5: phase5/replacement-prevention-effects slice

Branch `phase5/replacement-prevention-effects`, off master. Slice backlog item 5. Scope, locked in
with the human before writing code: `ReplacementEffect` gains the same `Primitive`/`Script` split
`ContinuousEffect` already has — `EventFilter` (matcher) covering exactly two shapes (damage about
to be dealt; a permanent about to move Battlefield→Graveyard, CR 704.5g's "dies") and
`ReplacementPayload` (rewrite) covering `PreventDamage`/`RedirectDamage`/`ReplaceZoneDestination`.
`apply_replacements`/`submit_replacement_choice` (both `todo!()` since Phase 2) become real.
`Script` stays dead code, same convention every prior slice's filter-shaped type follows — no Lua
builder for a card to construct a `ReplacementEffect` itself, unlike the triggered/static/activated
ability builders (a natural future sibling, not built here).

**Correction to the backlog, found while scoping this one:** checked `commands.rs`/`sba.rs` before
writing code — neither combat damage nor SBA-driven death actually routes through the replacement
pipeline today. `AssignCombatDamage` mutates `damage_marked`/life *directly* in `execute_command`,
before the `DamageDealt` events it returns ever reach `ReplacementCheck` (a real, pre-existing gap
`commands.rs` already flagged inline: "no replacement/prevention check yet"). `apply_creature_death`
(sba.rs) moves a dying creature straight to the graveyard and commits its own `ZoneChange` directly
— SBA events never flow through `ReplacementCheck` at all, only `execute_command`'s do. So "prevent
damage"/"if this would die, exile instead" can't be exercised end-to-end through real combat/SBA
without a separate fix to those two call sites' apply-then-report timing. Confirmed with the human
mid-report: kept this slice scoped to `replacement.rs`/`effect.rs` themselves, tested via directly-
constructed events and `Engine`-level tests that drive the real `ReplacementCheck`/
`AwaitingReplacementChoice` pipeline stages directly (not through combat/SBA producers) — split the
two bypass gaps out as their own named backlog items (5a, 5b, above) rather than silently fixing or
silently ignoring them.

**Bug caught mid-slice, before merge:** the first draft of `apply_replacements`'s CR 616
stabilization loop hung forever on two of its own new tests
(`repeatable_effect_is_never_consumed`, `stabilizes_across_two_different_replacements`) —
`cargo test` visibly stalled rather than failing cleanly. Root cause: nothing stopped an already-
applied `Repeatable`/multi-`LimitedUses` effect from matching its own just-rewritten output again
on the very next loop iteration — `PreventDamage` zeroing an already-zero `amount` doesn't change
the event in any way that stops its own `EventFilter` from matching, so the loop never reached a
fixed point. The real rule (CR 616.1: "each applicable replacement effect can apply only once to a
given event") was missing entirely — `ConsumptionPolicy` governs whether an effect survives for
*future* events, not whether it can reapply to the *same* event repeatedly. Fixed by threading a
per-event `excluded: Vec<ObjectId>` through the whole resolution — `apply_replacements` is now a
thin wrapper over `apply_replacements_from(state, event, Vec::new())`, which appends each applied
effect's source to `excluded` before looping, and `ReplacementOutcome::NeedsOrderingChoice`/
`ProcessState::AwaitingReplacementChoice` both carry `excluded` forward across a CR 616.1 pause too
(`submit_replacement_choice` pushes `chosen` onto it before resuming) — otherwise an effect applied
before the pause could reapply to the same event after it. Caught by the test suite itself doing
exactly its job, not a redesign — the shape (`Primitive`/`Script` split, `EventFilter`/
`ReplacementPayload`) was right, only the stabilization loop's termination condition was wrong.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 260 tests total (236 prior + 7 new
`ReplacementEffect::matches`/`apply` unit tests in `types/effect.rs` + 7 new `apply_replacements`/
consumption/stabilization unit tests in `replacement.rs` + 3 new `Engine`-level pipeline tests in
`engine.rs`'s own `#[cfg(test)]` module, exercising `ReplacementCheck`/`AwaitingReplacementChoice`/
`submit_replacement_choice` directly rather than through combat/SBA), 0 failed, no hangs (re-ran
under a hard `timeout` after the mid-slice bug to confirm termination, not just absence of a
panic). Confirmed the CLI still runs an unrelated existing scenario
(`phase5_static_ability_self_registration.toml`) unaffected. No new CLI/TOML fixture this slice —
same "flagged rather than silent" stance the activated-ability-costs slice already took, since
`crates/scenario` has no TOML shape for constructing a `ReplacementEffect` and the two producers
that could exercise one for real (combat, SBA) are exactly the bypasses named above.

**What got implemented:**
- `types/effect.rs`: `ConsumptionPolicy` gained `Clone, Copy, PartialEq, Eq, Debug` (needed to read/
  compare/mutate it in place during consumption bookkeeping). New `EventFilter`
  (`DamageDealt{affected: EffectFilter}` / `WouldDie{affected: EffectFilter}` — reuses
  `EffectFilter`'s existing controller/card_types/self_only shape rather than inventing a parallel
  one) and `ReplacementPayload` (`PreventDamage`/`RedirectDamage(Target)`/
  `ReplaceZoneDestination(ZoneKind)`). New `ReplacementSpec` (`Primitive`/`Script`, `Script` dead
  code). `ReplacementEffect.event_pattern: ScriptHandle` replaced with `spec: ReplacementSpec`.
  `ReplacementEffect::matches(&self, event, state)` real body (gained a `&GameState` param — needed
  to resolve the event's own object/player and the effect's source's controller; no `&ScriptRuntime`
  needed, the `Primitive` case is zero-Lua-calls same as every other primitive-only query in this
  codebase). `ReplacementEffect::apply(&self, event) -> GameEventKind` new — the "matcher had no
  rewrite" gap flagged since Phase 3, closed the same way `ContinuousEffect`'s own matcher/rewrite
  split already was. New free fn `event_target_matches` shared by both `EventFilter` arms —
  generalizes `ContinuousEffect::applies_to`'s controller/card_types/self_only check over `Target`
  (a player has no card types and can't be `self_only`, both real non-matches not guessed past).
- `replacement.rs`: `apply_replacements` real body — thin wrapper over new `pub(crate)
  apply_replacements_from(state, event, excluded)`, the actual CR 614/615/616 loop (see the bug note
  above for why `excluded` exists). New `pub(crate) apply_one_replacement` — applies one effect's
  rewrite and performs its `ConsumptionPolicy` bookkeeping (`OneShot`/exhausted `LimitedUses`
  removes the entry from `state.replacement_effects`; `LimitedUses(n>1)` decrements in place;
  `Repeatable` untouched) — shared by `apply_replacements_from`'s single-match loop body and
  `Engine::submit_replacement_choice`. New private `affected_player_for` — CR 616.1's "affected
  player" derivation (a `Target::Player` directly, a `Target::Object`'s controller), covering the
  two constructible `EventFilter` shapes; a `_ =>` fallback exists only so this stays total, not
  reachable given today's shapes. `ReplacementOutcome::NeedsOrderingChoice` gained `excluded:
  Vec<ObjectId>`.
- `engine.rs`: `ProcessState::AwaitingReplacementChoice` gained `excluded: Vec<ObjectId>`, threaded
  through both its construction sites (`ReplacementCheck`'s multi-match arm, the pause-and-reprompt
  arm) and `submit_replacement_choice`'s real body — applies `chosen` via `apply_one_replacement`,
  pushes it onto `excluded`, then resumes via `apply_replacements_from` (not the public wrapper) so
  the exclusion set survives the pause. `StepStatus::AwaitingReplacementChoice` (the public-facing
  status) is unchanged — the host doesn't need `excluded` to prompt the player, same reasoning
  `AwaitingPayment`'s `source` field wasn't added to its own public status.

**STRUCT DEVIATIONS:**
- `ReplacementOutcome::NeedsOrderingChoice`/`ProcessState::AwaitingReplacementChoice` both gaining
  `excluded` — not in the pre-slice scoping discussion (which hadn't yet discovered the
  reapplication-loop gap), a direct, necessary consequence of the mid-slice bug fix rather than
  scope creep.

**INTERFACE DEVIATIONS:**
- `ReplacementEffect::matches` gained a `&GameState` param beyond the Phase 3 TODO's guess (which
  expected a `&ScriptRuntime` instead) — the `Primitive` case turned out to need zero Lua calls,
  same "the TODO over-pattern-matched against a different, real wall" correction the
  layer-engine-consumers slice already made once for a different query.
- New `pub(crate) apply_replacements_from`/`apply_one_replacement` — not named in the pre-slice
  report, direct consequence of the CR 616.1 fix (`apply_replacements` itself keeps its original
  public signature unchanged).

**SCOPE DEVIATIONS:**
- Backlog items 5a/5b (`AssignCombatDamage`/SBA-driven death bypassing replacement checking) — both
  surfaced and confirmed with the human before writing code, not discovered after the fact; kept out
  of this slice's diff entirely per that discussion.

**UNRESOLVED (anticipated, none new beyond 5a/5b already covered above):**
- `EventFilter`/`ReplacementPayload` cover exactly two shapes (damage, "would die") — no ETB-tapped
  modifiers, no counter-addition replacements, no non-damage/non-death event shapes.
- No Lua builder (`Game.CreateReplacementEffect`) — `ReplacementSpec::Script` stays dead code; a
  card can only get a `ReplacementEffect` via a Rust struct literal (test/scenario fixtures) today,
  same bootstrap-before-scripting stance every other primitive-first subsystem in this codebase took
  before its own builder slice landed.
- `AssignCombatDamage`/SBA-driven death bypassing replacement checking — named above as backlog
  items 5a/5b, not fixed here.

## Phase 5: phase5/fix-replacement-bypasses slice

Branch `phase5/fix-replacement-bypasses`, off master. Slice backlog items 5a/5b — the two gaps
the replacement-prevention-effects slice surfaced but deliberately didn't fix: `AssignCombatDamage`
mutates state before its `DamageDealt` events reach `ReplacementCheck`, and SBA-driven death never
goes through `ReplacementCheck` at all.

**Scope fork, put to the human before writing code:** 5a is a clean timing fix (the pipeline
already exists, only *when* the mutation happens is wrong). 5b is structurally harder — SBA has no
pause mechanism anywhere, unlike `execute_command`→`CommitEvents`. Two options were on the table:
a deterministic-first-match fallback for the rare 2+-simultaneous-death-replacement case (small,
CR-616.1-incomplete for that one edge), or building SBA its own real `AwaitingReplacementChoice`-
equivalent pause (bigger, fully compliant). **The human chose the real pause.** Everything below
reflects that.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 265 tests total (260 prior + 2 new in
`phase5_combat_damage_replacement.rs` proving 5a end-to-end through real combat, + 3 new in
`phase5_sba_death_replacement.rs` proving 5b end-to-end, including the real multi-match pause and
resume), 0 failed. Run under a hard `timeout` throughout this slice (lesson from the prior slice's
own hang) — no hangs. Confirmed the CLI still runs an unrelated existing scenario
(`phase5_layer_engine_basics.toml`) unaffected.

**What got implemented (5a):**
- `commands.rs`: `AssignCombatDamage`'s arm no longer mutates `damage_marked`/life/counters at
  all — it only proposes `DamageDealt` events now, same "propose via `CommandOutcome.events`,
  mutate only at commit time" discipline `CastSpell`/`PlayLand` already followed for zone changes.
  The extracted mutation logic (Lifelink/Wither/Infect/Deathtouch, life loss, poison, marked
  damage) becomes new `pub(crate) fn apply_damage_event(state, script_runtime, source, target,
  amount)` — unchanged behavior, just relocated and parameterized off already-known values instead
  of reading straight out of the loop.
- `engine.rs`: `CommitEvents`'s loop calls `commands::apply_damage_event` for each
  `GameEventKind::DamageDealt` in `checked` — i.e., only once the event is in its final,
  post-replacement form. A `PreventDamage`/`RedirectDamage` effect registered against combat damage
  now actually stops/redirects the real mutation, not just the ledger's record of it (see
  `phase5_combat_damage_replacement.rs`).

**What got implemented (5b):**
- `sba.rs`: `apply_creature_death` split into `collect_dying_creatures` (the existing CR
  704.5f/g candidate filter, unchanged) and new `pub(crate) fn resolve_deaths(state,
  pending_deaths: VecDeque<ObjectId>) -> DeathResolution` — processes one dying creature at a
  time, proposing its Battlefield→Graveyard `ZoneChange` (using its *old*, still-on-the-battlefield
  id, since `ReplacementEffect::matches` needs to resolve it) and running it through
  `replacement::apply_replacements` *before* actually moving it. `DeathResolution::Done(Vec
  <GameEvent>)` (everything resolved, ids allocated) or `DeathResolution::NeedsOrderingChoice
  { affected_player, pending_event, candidates, excluded, pending_deaths }` (CR 616.1, same shape
  `ReplacementOutcome::NeedsOrderingChoice` already has, reached from inside an SBA pass instead of
  a command's own events). New `pub(crate) fn commit_death` — the actual zone-move + CR 400.7 new-
  id + `retire_object` purge, parameterized by the *final* `to` zone (`Graveyard` unless a
  `ReplaceZoneDestination` replacement fired) instead of the old hardcoded `Graveyard`.
  `check_state_based_actions`'s signature changed from `-> Vec<GameEvent>` to `-> SbaOutcome`
  (`Progressed(Vec<GameEvent>)` — same meaning the bare `Vec` had before, just wrapped — or
  `NeedsOrderingChoice { .., resolved_so_far }`, `resolved_so_far` being this pass's already-settled
  events, e.g. player-loss, unaffected by the pause and safe to commit immediately).
- `engine.rs`: `ProcessState::AwaitingReplacementChoice`'s `remaining`/`checked` fields (specific to
  the command-pipeline `ReplacementCheck` case) replaced with a single `resume:
  ReplacementResumeTarget` — `ReplacementCheck { remaining, checked }` (the original case) or
  `SbaDeath { pending_deaths, then: SbaResumeThen }` (new; `then` is `SbaLoop` or
  `CleanupSbaCheck`, since those two states' own "nothing more to do" transitions differ — CR
  514.3's cleanup-priority-window special case vs. the normal `CollectTriggers` transition — and the
  resume needs to know which one to finish into). `submit_replacement_choice` branches on `resume`
  after applying the chosen effect. Three new private `Engine` methods: `finish_sba_loop`/
  `finish_cleanup_sba_check` (each state's own "what happens once this pass's events are known"
  transition, pulled out so a resumed pause and a fresh pass agree on it instead of duplicating the
  branch) and `resume_sba_death` (continues `sba::resolve_deaths` after a paused choice, using
  those two `finish_*` methods once it's genuinely done). Both `SbaLoop`'s and `CleanupSbaCheck`'s
  own arms rewritten to match on `SbaOutcome` and route a `NeedsOrderingChoice` into
  `AwaitingReplacementChoice` with the appropriate `SbaDeath` resume target.

**STRUCT DEVIATIONS:**
- `SbaOutcome`/`DeathResolution` (sba.rs), `ReplacementResumeTarget`/`SbaResumeThen` (engine.rs) —
  all anticipated in the pre-slice discussion once "build the real pause" was chosen; this *is* the
  shape that discussion converged on, not a deviation from it.

**INTERFACE DEVIATIONS:**
- `check_state_based_actions`'s return type change (`Vec<GameEvent>` → `SbaOutcome`) rippled into
  every direct test caller (19 call sites across 4 test files: `phase5_layer_engine_basics.rs`,
  `phase5_layer_engine_cda.rs`, `phase5_layer_engine_types_colors.rs`, `phase5_sba_basics.rs`) —
  anticipated as expected mechanical churn once the signature change was on the table, same
  "ripple through every direct caller" pattern the `&ScriptRuntime` threading (layer-engine-cda
  slice) and `AbilityKind`-cost threading (activated-ability-costs slice) already established. Each
  file gained a small `progressed(outcome) -> Vec<GameEvent>` local helper (panics on
  `NeedsOrderingChoice`, since none of those fixtures register a replacement on a dying creature) so
  the actual call-site diff is one wrapping function call, not a rewritten assertion.

**SCOPE DEVIATIONS:** None — 5a/5b were the explicit, sole scope of this slice, confirmed with the
human before writing any code.

**UNRESOLVED:**
- `resolve_deaths`/`commit_death` only handle the CR 704.5f/g creature-death shape — a
  `ReplaceZoneDestination` fired against some other future SBA-driven zone change (none exist yet)
  would need the same treatment applied to whatever produces it.
- `CleanupSbaCheck`'s own pre-existing incompleteness (CR 514.3's "another cleanup step begins
  after this priority window" isn't modeled — `advance_turn_step` just rotates the turn) is
  untouched; `SbaResumeThen::CleanupSbaCheck` reaches the exact same incomplete transition
  `finish_cleanup_sba_check` already had, just reachable now via a resumed pause too, not a new gap.
- Every other `EventFilter`/`ReplacementPayload` gap the replacement-prevention-effects slice
  already flagged (no ETB-tapped modifiers, no counter-addition replacements, no Lua builder) is
  unchanged — this slice only fixed *when* replacement checking runs for damage/death, not *what*
  it can express.

## Phase 5: phase5/flashback-alternative-costs slice (6c, merged to master)

Branch `phase5/flashback-alternative-costs`, off master. Scoped to flashback specifically (CR
702.34a) — the cleanest, most literal "alternative cost" (CR 601.2b) — not escape/jump-start/
aftermath/mayhem, each of which layers on its own additional-cost or zone-shape wrinkle deferred
below. `CostSpec.alternative`/`AlternativeCost::Replacement` existed since Phase 1 but nothing ever
populated them — `CardDefinition` had no field to carry a card's alternative costs at all, so
`CastSpell` always built `CostSpec::mana_only(...)` and `PaymentPlan::validate`'s `if
!spec.alternative.is_empty() { return false }` guard was unreachable dead weight. This slice is
that missing data plus the cast-time plumbing to use it — `PaymentPlan::validate` itself needed
*zero* changes, same reasoning as the x-costs slice's `{X}` substitution: by the time `validate`
sees the resolved `CostSpec`, the alternative has already been swapped in wholesale, and
`spec.alternative` stays empty on that resolved spec.

**VERIFICATION RESULT:** Pass. New `phase5_flashback_alternative_costs.rs` — 7 tests (cast from
hand unaffected with no alternative announced; cast from graveyard with the index, paid with the
flashback cost, not the mana cost; graveyard without declaring the alternative rejected; resolving
lands in exile not graveyard; fizzling also lands in exile; out-of-range index rejected; declaring
the alternative while still in hand rejected) — all green. `cargo test --workspace` green
throughout, no regressions from the mechanical field-addition sweep below.

**What got implemented:**
- `CardDefinition.alternative_costs: Vec<AlternativeCost>` — the actual missing persistent data a
  card needs to say "I have flashback for `{2}{R}}`."
- `AlternativeCost::Replacement` gained `from_zone: ZoneKind` and `exile_on_leaving_stack: bool`,
  named generically (not flashback-specific) so escape/jump-start/aftermath/mayhem can reuse this
  exact shape later — all of them are "cast from graveyard/exile, replace the normal cost."
- `PlayerActionKind::CastSpell` gained `alternative_cost: Option<usize>` — an index into
  `alternative_costs`, announced at cast time, same phase `x_value` already established (6b).
- `commands.rs`'s `CastSpell` arm: resolves the announced index (rejects out-of-range before
  anything mutates), computes `required_zone` from the chosen alternative's `from_zone` (or `Hand`
  if none was announced) and rejects unless the card is actually there — the simplest correct rule
  given there's no "does this card actually have flashback" check independent of the
  `alternative_costs` data itself (see UNRESOLVED). Cost building now branches: the alternative's
  own `cost` entirely replaces the mana/X-resolution path, never mixed with it. The "remove from
  hand" zone-mutation and its `ZoneChange` event both generalized from hard-coded `ZoneKind::Hand`
  to the resolved `from_zone`.
- `StackObject.exile_on_leaving_stack: bool`, baked in at push time from the chosen alternative
  (mirrors 6b's `x_value` precedent) — `false` for every non-`CastSpell` `StackObject::new` call
  site (`ActivateAbility`, `PutTriggersApnap`'s triggered-ability push), since neither abilities nor
  triggers have an alternative-cost concept.
- `engine.rs`'s two hard-coded-`Graveyard` zone-destination sites (the fizzle branch and the
  normal-resolution instant/sorcery branch inside `ResolveStackTop`) now check
  `top.exile_on_leaving_stack` and route to `Exile` instead when it's set.

**STRUCT DEVIATIONS:**
- `move_resolving_spell`'s `destination_owner` computation (`Battlefield → None, else →
  Some(owner)`) was a latent bug this slice's own tests exposed: `Exile` is a single shared zone
  with `owner: None` (built once in `build_initial_state`), just like `Battlefield` — not per-player
  like `Graveyard`. The old two-way match happened to be correct only because `Graveyard` was the
  sole non-`Battlefield` destination that existed before this slice. Fixed to match on `Battlefield
  | Exile` both giving `None`, `_` giving `Some(owner)`. Not anticipated in the pre-slice report;
  caught by `resolving_flashback_spell_exiles_instead_of_graveyard` failing against the wrong zone
  lookup during development, not by inspection.

**INTERFACE DEVIATIONS:** None beyond what the pre-slice requirements report already called for
(`StackObject::new` gaining the bool param, `CastSpell` gaining the field) — the report's own
"four things need to change together" scoping held exactly as written.

**SCOPE DEVIATIONS:**
- `CardDefinition.alternative_costs` (last-declared field) and the `PlayerActionKind::CastSpell`
  variant rippled mechanically through every direct construction site — same "wide, not individually
  risky" mechanical-sweep shape 6a-iii's `ManaPool::default()` sweep already established, done via a
  small Python script rather than by hand across ~32 files / ~46+ occurrences. Two files' `TODO`-
  worthy edge cases the blind sweep pattern-matched onto by mistake: `phase5_play_land.rs`,
  `phase5_pass_priority_loop.rs`, and `phase5_cast_spell.rs` each construct a `StackObject` literal
  directly (which also has its own, unrelated `target_requirement: None,` line) — the sweep's regex
  added the new `CardDefinition` field there too, caught by the resulting compile error
  (`StackObject` has no field named `alternative_costs`) and hand-corrected to
  `exile_on_leaving_stack: false` instead.
- `crates/scenario`'s `CardSpec`/`ActionSpec::CastSpell` construction updated only to keep compiling
  (`alternative_costs: Vec::new()`, `alternative_cost: None`) — no TOML scenario field for either
  exists yet, flagged with a `TODO` in `lib.rs` at both sites. A flashback fixture isn't expressible
  via the TOML scenario format this slice, only via a direct Rust struct literal (the new test file)
  — same "verification tooling has to track new fields, but not necessarily expose them yet"
  pattern every earlier slice's scope-deviation section already established.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Escape (needs an `ExileFromGraveyard` additional-cost shape that doesn't exist), jump-start/
  retrace (a discard-a-card additional cost on a spell — spells have no additional-cost slot
  populated from card data any more than they had an alternative one before this slice, same
  underlying gap), aftermath/mayhem, buyback/kicker (CR 118.8/118.9 — additional, not alternative),
  cost reduction/increase (6d), ability-cost mana strings (6e). Countering a spell isn't modeled at
  all, so "exile instead of graveyard when countered" isn't a real path to handle yet — only the
  fizzle and normal-resolution zone-change sites exist today.
- `commands.rs`'s zone check is "the announced alternative's `from_zone` must match where the card
  actually is, full stop" — there's no independent check that the card's `alternative_costs` entry
  actually corresponds to a real Flashback keyword/oracle-text grant vs. an arbitrary
  `alternative_costs` entry a card happens to carry. Fine for now since nothing populates
  `alternative_costs` except deliberately-authored fixtures, but worth tightening once a real
  card-database/script-bridge path can author this field for actual printed cards.
- X combined with an alternative cost (e.g. flashback on an `{X}` spell) isn't handled —
  `x_value` is simply unchecked whenever an alternative is chosen, flagged with a `TODO` directly in
  `commands.rs`'s `CastSpell` arm.

## Phase 5: phase5/cost-modifiers slice (6d, merged to master)

Branch `phase5/cost-modifiers`, off master. Next item in backlog #6 ("cost system completeness"),
after 6a (Phyrexian/snow/hybrid mana payment), 6b (`{X}` costs), 6c (flashback/alternative costs).
Scoped to a static "spells [you cast/of some type] cost `{N}` more/less to cast" effect (Goblin
Warchief, Thalia) — flat fixed amounts only, generic-component-only, spells only (not activated
abilities), same narrowing precedent 6b/6c both already established for their own slices.

**Key design point, decided before writing code:** cost modification (CR 601.2f/118.9) is **not**
part of the CR 613 layer system, despite superficially looking like another "static effect that
changes something" case `ContinuousEffect`/`layers.rs` already handle. It's a one-time adjustment
applied while determining a spell's total cost, evaluated once at cast time, not a continuous
characteristic query re-evaluated on demand. So this needed its own mechanism (`CostModifier`, a
new flat list on `GameState`) rather than extending `EffectPrimitive`/`Layer`.

**VERIFICATION RESULT:** Pass. New `phase5_cost_modifiers.rs` — 7 tests (reduction lowers the
generic component only, colored pip untouched; increase raises it; reduction floors at `{0}`
generic rather than going negative; a non-matching filter has no effect; two modifiers both apply
in registration order; a modifier scoped to "spells your opponents cast" doesn't apply to its own
controller's spell; casting with zero registered modifiers is completely unaffected) — all green.
`cargo test --workspace` green throughout, no regressions from the mechanical sweep below.

**What got implemented:**
- `ManaCost::apply_cost_delta(delta: i32) -> ManaCost` (mana.rs) — adjusts only a bare
  `Atom(Generic(_))` component (never a colored pip, never inside a `Hybrid` — same scope
  `generic_total`'s own doc comment already draws), clamped at `{0}` (CR 118.9). If the cost has no
  generic component yet, a positive delta still creates one (a cost increase can introduce generic
  cost where none existed, e.g. Thalia taxing an otherwise-colored-only spell); a non-positive delta
  with nothing to reduce is a no-op. Mirrors `resolve_x`'s existing "filter/rebuild `symbols`"
  pattern.
- `CostModifier { source: ObjectId, controller: PlayerId, filter: FilterSpec, amount: i32 }`
  (cost.rs) — reuses the existing `FilterSpec` (card_types + relative controller) rather than
  inventing a parallel "which spells" shape: a spell pre-cast is still a plain `GameObject` (with
  `.controller` = caster) at the point `commands.rs` needs to ask "does this modifier apply," so
  `FilterSpec::matches(spell, modifier.controller)` — `viewer` being the modifier's own controller,
  whose "You"/"Opponent" perspective its filter is written from — just works with zero new type.
- `apply_cost_modifiers(base, modifiers, spell)` (cost.rs) — applies every matching modifier one at
  a time in list order via `apply_cost_delta`, not "sum every delta, then clamp once" — matters for
  the floor-at-zero case (see the doc comments on both functions for why).
- `GameState.cost_modifiers: Vec<CostModifier>` — new flat list, same "scan-friendly, not nested"
  convention as `continuous_effects`/`replacement_effects`.
- `commands.rs`'s `CastSpell` arm: calls `apply_cost_modifiers` on the resolved mana cost right
  after X-substitution, only on the plain-mana-cost path (`alternative_cost: None`) — never mixed
  with an alternative cost, same "entirely replacing, never mixing" stance 6c's cost-building branch
  already took.

**STRUCT DEVIATIONS:** None — matched the pre-slice requirements report exactly.

**INTERFACE DEVIATIONS:** None.

**SCOPE DEVIATIONS:**
- `GameState.cost_modifiers` rippled through all 33 files with a `GameState` literal — mechanical
  Python sweep anchored on the pre-existing `replacement_effects: Vec::new(),` line, same shape as
  6c's `alternative_costs` sweep. Unlike 6c's sweep, no ambiguous anchor collisions this time (the
  anchor line is unique to `GameState` literals, unlike `target_requirement: None,` which also
  appeared in unrelated `StackObject` literals) — every insertion landed correctly on the first pass.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Ability cost reduction (Heartstone-style "ability costs of creatures you control cost `{1}` less
  to activate") — `ActivateAbility`'s cost path is entirely untouched by this slice.
- Colored-pip-specific reduction/increase (e.g. "costs `{W}` less") and dynamic/count-dependent
  amounts (Affinity-style, scales with permanents controlled) — `amount` is a fixed `i32` only.
- No `Game.CreateCostModifier` script builder yet — same "Rust struct-literal fixtures only"
  bootstrap stance `replacement-prevention-effects`/`static-ability-self-registration` each took
  before their own builder slice landed.
- CR 613.7-style "affected player chooses application order" for same-timestamp modifiers isn't
  modeled — `apply_cost_modifiers` applies matches in `state.cost_modifiers`'s registration order
  instead, un-chosen.
- Interaction with 6c's alternative costs (does a cost modifier apply to a flashback cost too?) —
  real CR 601.2f says yes in principle; this slice's cost modifiers only ever touch the plain
  mana-cost path, same deferral 6c's own report already flagged from the other direction.

## Phase 5: phase5/ability-mana-cost-strings slice (6e, merged to master)

Branch `phase5/ability-mana-cost-strings`, off master. Last item in backlog #6 ("cost system
completeness") — closes it out entirely (6a Phyrexian/snow/hybrid mana payment, 6b `{X}` costs, 6c
flashback/alternative costs, 6d cost modifiers, 6e this slice).

**Key finding before writing code:** the engine-side gap this backlog item implied didn't actually
exist. `RegisteredAbility.cost: Option<CostSpec>` and `CostSpec.mana: Option<ManaCost>` were already
real (from the activated-ability-costs slice), and `PaymentPlan::validate` already validates a mana
component generically regardless of whether the `CostSpec` came from a spell or an ability — no
engine-logic changes were needed at all. The actual gap was purely on the *authoring* side:
`ActivatedAbilityBuilder::AddCost` (script.rs) only ever pushed onto `cost.additional`, since no
Lua-facing mana-cost-string parser existed anywhere to turn `"{1}{R}"` into a real `ManaCost` — that
builder's own doc comment already flagged this as deferred. So this slice is a pure string→data
parser plus one new script binding, zero engine-logic changes.

**VERIFICATION RESULT:** Pass. `ManaCost::parse` — 9 new unit tests in `mana.rs` (generic+colored,
repeated colored pips, color/color hybrid, generic/color hybrid, plain Phyrexian, hybrid-Phyrexian,
snow+colorless, `{X}`, and a batch of malformed-string rejections). `phase5_activated_ability_self_registration.rs`
gained 5 new tests: parse-and-register via `Game.Costs.Mana("{1}{R}")`, rejecting a second
`Game.Costs.Mana(...)` call on one builder, rejecting a malformed cost string, plus a full
end-to-end activate→pay→resolve test through the real `Engine` (mana + `{T}` cost paid together,
ability resolves, life gained). `cargo test --workspace` green throughout, no regressions.

**What got implemented:**
- `ManaCost::parse(s: &str) -> Result<ManaCost, ManaCostParseError>` (mana.rs) — tokenizes
  `{...}`-group cost strings into every symbol shape already modeled: digits → `Generic`; a single
  WUBRG letter → `Colored`; `C` → `Colorless` (parses fine, stays unpayable — pre-existing
  `atom_branches` gap, not fixed here); `S` → `Snow`; `X` → bare `ManaSymbol::X`; `<letter>/P` →
  plain `Phyrexian` (not a `Hybrid` — matches this type's own doc comment on why); `<A>/<B>` → color/
  color or generic/color `Hybrid`; `<A>/<B>/P` → hybrid-Phyrexian `Hybrid(Phyrexian(A), Colored(B))`,
  matching the exact convention `cost.rs`'s `hybrid_phyrexian_offers_three_real_branches` test
  already documents. Anything outside a `{...}` group, an empty/unterminated group, or an
  unrecognized shape inside one is a hard parse error.
- `script.rs`: generalized `CostTag(AdditionalCost)` → `CostTag(CostComponent)` where
  `CostComponent` is `Additional(AdditionalCost) | Mana(ManaCost)` — a single enum rather than a
  separate builder method, so `Game.Costs.Mana(...)` slots into the exact same `:AddCost(...)`
  vocabulary as `TapSelf`/`Sacrifice`/`Discard`/`TapPermanents`/`Life`. `decode_additional_cost_value`
  renamed to `decode_cost_component_value` (same shape, wider return type).
  `ActivatedAbilityBuilder::AddCost` now dispatches on the decoded `CostComponent`: `Additional`
  pushes onto `cost.additional` as before, `Mana` sets `cost.mana` — erroring (not silently
  overwriting) if called a second time on the same builder.
- New `Game.Costs.Mana(s)` binding in `bind_game_globals` — parses `s` via `ManaCost::parse`,
  surfaces a parse failure as a real Lua runtime error (not a nil/silently-ignored cost).

**STRUCT DEVIATIONS:** `CostComponent` (script.rs) is new, but directly implied by "generalize
`CostTag`" from the pre-slice plan — not a surprise discovered mid-implementation.

**INTERFACE DEVIATIONS:** `decode_additional_cost_value` renamed to `decode_cost_component_value`
with a widened return type (`AdditionalCost` → `CostComponent`) — a direct, necessary consequence of
generalizing `CostTag`, not independently scoped, but worth naming as a rename for anyone grepping
for the old name.

**SCOPE DEVIATIONS:** None — no `GameState`/`CardDefinition` literal sweep needed this time (no new
fields on either type), everything landed in `mana.rs`, `script.rs`, and the one existing
ability-self-registration test file.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- `{C}` parses fine but stays unpayable — `atom_branches`'s pre-existing gap, unrelated to this
  slice's own scope.
- `{X}` parses to `ManaSymbol::X`, but an activated ability still has no `x_value` announcement slot
  to resolve it with (flagged since the x-costs slice, 6b) — nothing consumes an ability-side `{X}`.
- `CardDefinition.mana_cost` (spells) is untouched — still sourced as a structured `ManaCost` from
  the external card database, never parsed from a string; only `ActivatedAbilityBuilder` got this
  wiring.

**Backlog item 6 ("cost system completeness") is now fully closed** — every sub-item (alternative
costs, `{X}` costs, hybrid/Phyrexian/snow mana payment, cost reduction/increase, ability mana-cost
strings) has landed on master.

## Phase 5: phase5/damage-dealt-triggers slice (backlog item 7, first sub-slice)

Branch `phase5/damage-dealt-triggers`, off master. Backlog item 7 ("trigger system completeness")
bundles three things: once-per-turn/intervening-if legality, a CR 603.10 last-known-info fallback,
and new non-ETB trigger conditions (only `Enters`/`Always` existed). Split off the narrowest,
cleanly-landable piece rather than take the whole item at once — same pattern the layer-engine and
cost-system backlog items used.

**Key finding before writing code, which reshaped the scope:** two condition shapes looked equally
easy at a glance — "beginning of your upkeep" and "whenever this deals damage" — but only one
actually is. `AdvanceTurnStep`'s `StepStarted`/`PhaseStarted`/`TurnStarted` events go through
`engine.rs`'s `log_events`, which appends straight to the ledger and **never runs `collect_triggers`
at all** — a genuine, previously-undocumented gap (now backlog item 7a). An upkeep trigger is
structurally unfireable until `AdvanceTurnStep` is rewired through the same `ReplacementCheck`→
`CommitEvents`→`CollectTriggers` pipeline `PendingAction`/`ResolveStackTop` already use — real,
separate plumbing, not a condition-matching addition. `GameEventKind::DamageDealt` (from
`AssignCombatDamage`), by contrast, already flows through that real pipeline (confirmed via the
`fix-replacement-bypasses` slice's own notes) — reachable with zero pipeline changes, only new
`ConditionPrimitive` matching logic. So this slice narrowed to damage-dealt conditions only, and 7a
became its own named, deferred backlog item instead of silently blocking this one.

**VERIFICATION RESULT:** Pass. 6 new unit tests in `types/ability.rs` (`DealsDamage`/`IsDealtDamage`
matching/non-matching cases, including a player-target non-match and an unrelated-event non-match).
New `phase5_damage_dealt_triggers.rs` — 3 end-to-end tests through real combat
(`AssignCombatDamage` → pipeline → `CollectTriggers` → stack → resolve): an attacker's `DealsDamage`
ability fires and resolves (`GainLife(1)` applied), a bystander's `DealsDamage` ability that never
actually dealt damage does not fire, and a blocked attacker's blocker's `IsDealtDamage` ability
fires on its own controller. `cargo test --workspace` green throughout, no regressions.

**What got implemented:**
- `ConditionPrimitive` (ability.rs) gained `DealsDamage`/`IsDealtDamage` — zero-Lua-call fast-path
  matches, same convention `EntersBattlefield` already established. `DealsDamage` matches a
  `DamageDealt` event whose `source` is the ability's own source; `IsDealtDamage` matches one whose
  `target` is `Target::Object(source)` — deliberately never matches a `Target::Player` (a player
  being dealt damage isn't "this" for a permanent's own ability). No combat-vs-noncombat
  distinction: `DamageDealt` itself carries no such flag, and combat damage is currently its only
  producer anyway, so this isn't a narrowing, just what already exists.
- `script.rs`: `Game.Conditions.DealsDamage`/`Game.Conditions.IsDealtDamage` bindings, same
  `ConditionTag` pattern `Enters`/`Always` already use.
- Fixed a stale doc comment on `RegisteredAbility.cost` while in the file — it still said mana-cost
  components on an activated ability's cost were out of scope, left over from before the
  ability-mana-cost-strings slice (6e) closed that gap. Comment-only, no behavior change.

**STRUCT DEVIATIONS:** None — `ConditionPrimitive::matches`'s signature turned out not to need
changing (no `controller`/`active_player` param), unlike the pre-slice report's tentative guess —
both new variants only ever compare `event`'s own fields against `source`, same shape
`EntersBattlefield` already used.

**INTERFACE DEVIATIONS:** None.

**SCOPE DEVIATIONS:** The stale `RegisteredAbility.cost` doc-comment fix (noted above) — spotted
while reading the file for this slice, not part of the original plan; small enough to fix inline
rather than opening a separate throwaway commit for it.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Phase/step-boundary conditions ("beginning of upkeep") — blocked on the newly-named backlog item
  7a (`AdvanceTurnStep` bypassing `CollectTriggers`), not fixed here.
- Leaves-battlefield/dies conditions — blocked on the pre-existing `state.object(ability.source)`
  live-lookup gap in `collect_triggers` (CR 603.10's last-known-info fallback), untouched.
- Once-per-turn/intervening-if legality (CR 603.2-603.3) — untouched.
- No "combat damage specifically" vs. "damage in general" distinction, and no "deals damage to a
  player specifically" shape — `DamageDealt` doesn't carry either distinction today.

## Phase 5: phase5/upkeep-triggers slice (backlog item 7a, second sub-slice of item 7)

Branch `phase5/upkeep-triggers`, off master. Direct follow-on to the damage-dealt-triggers slice,
which named this exact gap: `AdvanceTurnStep`'s `StepStarted`/`PhaseStarted`/`TurnStarted` events go
through `log_events`, which appends straight to the ledger and never runs `collect_triggers` at all
— a phase/step-boundary trigger (e.g. "at the beginning of your upkeep") was structurally unfireable
until this was fixed. Unlike the damage-dealt slice, this one *is* real plumbing work, not just a
condition-matching addition — the pre-slice requirements report called this out explicitly.

**VERIFICATION RESULT:** Pass. 8 new unit tests in `types/ability.rs`
(`BeginningOfYourUpkeep`/`BeginningOfEachUpkeep` matching/non-matching on active-player relativity
and step shape). New `phase5_upkeep_triggers.rs` — 3 end-to-end tests driving the real turn loop
from `PreGame` into `Upkeep` (mirroring `phase5_beginning_phase.rs`'s own
`no_priority_during_untap` proof that one full pass-round lands exactly there): a controller's own
"beginning of your upkeep" ability fires and resolves; the same condition does *not* fire for a
different controller's permanent on someone else's upkeep; "beginning of each upkeep" fires
regardless of whose turn it is. `cargo test --workspace` green throughout — critically, the full
pre-existing `phase5_beginning_phase.rs` suite (untap/draw/skip/empty-library tests) stayed green
unchanged, confirming the pipeline rewire preserved every existing turn-advancement behavior.

**What got implemented:**
- `engine.rs`'s `ProcessState::AdvanceTurnStep` arm: the normal (non-`Untap`, non-`Cleanup`) step/
  phase transition now sets `pipeline_context`/`pipeline_exit` and routes `advance_turn_step()`'s
  events (plus the draw step's own `ZoneChange`, when not skipped) through
  `ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() }` — the exact
  same target pattern `ResolveStackTop` already uses — instead of `log_events` followed immediately
  by opening `PriorityWindow`. `Untap` (CR 502.4: no priority granted there, so no trigger window to
  place one before) and `Cleanup` (routes to `begin_cleanup_step`'s own separate `CleanupSbaCheck`
  flow) both keep calling `log_events` exactly as before — deliberately unfixed, not silently
  swept into this change. The Draw step's skip-check (CR 500.7/500.8) also keeps its own
  `log_events` path unchanged: CR 500.8 says a skipped step "doesn't happen at all," so no trigger
  collection should run against it either.
- `ConditionPrimitive::BeginningOfYourUpkeep`/`BeginningOfEachUpkeep` (ability.rs) — the concrete
  condition proving the plumbing fix works, not just the fix itself. `matches` gained
  `controller: PlayerId`/`active_player: PlayerId` parameters (a real signature change, unlike the
  damage-dealt slice, which didn't need one) — `BeginningOfYourUpkeep` requires `controller ==
  active_player` in addition to the `Step::Upkeep` `StepStarted` match; `BeginningOfEachUpkeep`
  doesn't check controller at all.
- `Game.Conditions.Upkeep`/`Game.Conditions.EachUpkeep` bindings (script.rs).
- Fixed two more stale doc comments while in these files: `ConditionPrimitive`'s own "only ETB-shaped
  conditions... are exercised" note (`triggers.rs`) was already inaccurate as of the damage-dealt
  slice and is now doubly so; reworded to name the real constraint (source must still resolve via
  `state.object()`) generically instead of naming ETB specifically.

**STRUCT DEVIATIONS:** `ConditionPrimitive::matches` gaining `controller`/`active_player` was
explicitly anticipated in the pre-slice report (unlike the damage-dealt slice, where the same
tentative guess turned out to be unnecessary) — not a deviation, the report called this correctly.

**INTERFACE DEVIATIONS:** None beyond the anticipated `matches` signature change above.

**SCOPE DEVIATIONS:** The two stale doc-comment fixes (`RegisteredAbility.cost` was already fixed in
the damage-dealt slice; `triggers.rs`'s ETB-specific wording fixed here) — spotted incidentally
while reading the files this slice touches anyway, not part of the original plan.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- `Cleanup`'s own trigger-collection gap — still open, its own differently-shaped fix (a second
  `CleanupSbaCheck`-style flow would need the same `CollectTriggers` wiring, plus its pre-existing
  CR 514.3 incompleteness TODO addressed at the same time or separately).
- "Whenever you draw a card," "at the beginning of combat," "at the beginning of the end step," etc.
  — same event family as upkeep, cheap to add now that the plumbing fix has landed, but not added
  this slice (scope stayed to proving the fix with one condition, per the pre-slice report).
- Untap-step triggers (e.g. "whenever a permanent untaps") — still unfireable; CR 502.4 grants no
  priority there regardless, so there's no window to place one before even if collected.
- Leaves-battlefield/dies conditions and once-per-turn/intervening-if legality — untouched, same as
  the damage-dealt slice's own UNRESOLVED list.

## Phase 5: phase5/dies-triggers slice (backlog item 7, third sub-slice of item 7)

Branch `phase5/dies-triggers`, off master. Closes the gap named since the script-bridge-trigger
slice and repeated as UNRESOLVED in both prior item-7 sub-slices: CR 603.10's last-known-information
fallback, specifically for "when this dies"/"when this leaves the battlefield" — the single most
common real trigger shape. Bigger and more structurally invasive than the damage-dealt/upkeep
sub-slices, flagged as such in the pre-slice requirements report.

**The gap was two-layered, not one.** `collect_triggers`'s `let Some(source_object) =
state.object(ability.source) else { continue; }` looked like the whole problem, but it's the
smaller half: `commands::retire_object` (called synchronously inside `sba::commit_death`, as part
of the very battlefield→graveyard move that would produce the "dies" event) already purges the
`RegisteredAbility` from `state.abilities` entirely before `collect_triggers` ever runs. A "when
this creature dies" ability isn't just unreachable via a live lookup — it's already deleted by the
time trigger collection happens. Fixing this needed the departing object's `Triggered`-kind
abilities captured and carried forward through three separate stages: `retire_object` itself,
`collect_triggers`, and `PutTriggersApnap`'s own `find_ability` lookup (which also scans the
now-purged `state.abilities` a second time, independently of `collect_triggers`).

**VERIFICATION RESULT:** Pass. 6 new unit tests in `types/ability.rs` (`Dies`/`LeavesBattlefield`
matching/non-matching cases: battlefield→graveyard by last-known object id, a different object's
death, a non-death departure, any destination, a move that never left the battlefield). New
`phase5_dies_triggers.rs` — 5 end-to-end tests through the real engine pipeline: a "when this
creature dies" ability fires and resolves after real SBA-driven lethal combat damage; an ability on
a creature that never dies never fires; a "leaves the battlefield" ability fires on a CR 614.13
"exile instead" redirect (a real non-death departure, reached through the same wired
`sba::commit_death` path via a `WouldDie` replacement rather than a separate producer); `Dies`
specifically does *not* fire on that same exile redirect (mirror-image control case); and a
regression test proving a live `DealsDamage` trigger (damage-dealt-triggers slice) and a
departed-abilities `Dies` trigger both fire correctly out of the *same* `collect_triggers` pass, from
one real combat-damage step. `cargo test --workspace` green throughout (333 engine-crate tests),
including every pre-existing suite (`phase5_damage_dealt_triggers`, `phase5_upkeep_triggers`,
`phase5_sba_death_replacement`, `phase5_script_self_registration`, etc.) unchanged.

**What got implemented:**
- `commands::retire_object` changed from `-> ()` to `-> Vec<RegisteredAbility>` — drains and returns
  the departing object's `Triggered`-kind abilities instead of silently discarding them (still purges
  everything from `state.abilities`/`state.continuous_effects` as before). Its other 3 call sites
  (`move_to_graveyard_for_cost` in commands.rs; discard-to-hand-size and draw-step library retirement
  in engine.rs) discard the return value this slice, each with an inline comment naming the deferral —
  exactly as scoped in the pre-slice report.
- `sba::commit_death` changed from `-> GameEvent` to `-> (GameEvent, Vec<RegisteredAbility>)`;
  `sba::resolve_deaths`'s `DeathResolution::Done` and `sba::check_state_based_actions`'s
  `SbaOutcome::Progressed` both gained a second `Vec<RegisteredAbility>` field, threading the captured
  abilities up from `retire_object` through the whole SBA-death call chain. `engine.rs`'s three call
  sites (`SbaLoop`, `CleanupSbaCheck`, `submit_replacement_choice`'s `SbaDeath` resume branch) and the
  `resume_sba_death`/`finish_sba_loop`/`finish_cleanup_sba_check` helper trio were all updated to
  match — `finish_sba_loop` is where the abilities actually land (see below); `finish_cleanup_sba_check`
  deliberately discards them (see UNRESOLVED).
- `Engine` gained a new transient field, `departed_triggered_abilities: Vec<RegisteredAbility>` — same
  "process state, not part of `GameState`" contract `pipeline_context`/`pipeline_exit` already
  establish. Populated by `finish_sba_loop`/`resume_sba_death` as deaths commit (accumulates across
  every `SbaLoop` pass in one pipeline run, since CR 704.3's fixed point can take several), consulted
  by `collect_triggers` and `PutTriggersApnap`'s ability lookup, cleared by `end_pipeline_run` (the
  same "run's last stage resets it" contract `pipeline_context` follows).
- `ConditionPrimitive::Dies`/`LeavesBattlefield` (ability.rs) — matched purely by
  `last_known_info.object_id == source` (NOT the event's own `object` field, which is the *new*
  `ObjectId` `commit_death` mints for the graveyard/exile-side object per CR 400.7 — the old id only
  survives inside `last_known_info`). `Dies` additionally requires `from == Battlefield` and
  `to == Graveyard`; `LeavesBattlefield` only requires `from == Battlefield`, any destination.
- `triggers::collect_triggers` gained a fourth parameter, `departed: &[RegisteredAbility]`, and a
  second loop (after the existing live-`state.abilities` loop) that walks it — live and departed
  abilities never share a `source` (retire_object always empties `state.abilities` for a source before
  it can appear in `departed`), so this is a genuinely separate source, not a duplicate scan. Only
  `Dies`/`LeavesBattlefield` conditions are evaluated there (deliberately narrower than the live loop —
  see UNRESOLVED); `controller` comes from the event's own `last_known_info.controller` rather than a
  live lookup, since `RegisteredAbility` itself has no controller field and the source object no longer
  resolves.
- `engine.rs`'s `PutTriggersApnap` ability lookup: `commands::find_ability(...).or_else(|| self
  .departed_triggered_abilities.iter().filter(|a| a.source == trigger.source).nth(...))` — the same
  "nth ability for this source, in that list's own order" convention `collect_triggers`'s new loop
  counts by, so a `PendingTrigger` produced by the departed-abilities path resolves back to the same
  `RegisteredAbility` here.
- `Game.Conditions.Dies`/`Game.Conditions.LeavesBattlefield` bindings (script.rs), same `ConditionTag`
  pattern every prior primitive condition uses.
- Updated three stale doc comments while in these files: `collect_triggers`'s own CR 603.10 IMPORTANT
  note (previously said dies/leaves-battlefield conditions "can't be added until this gap is fixed" —
  now describes the actual fallback); the live loop's `state.object()`-miss `continue` branch (now
  correctly scoped to a hypothetical bug case, since the real dies/leaves-battlefield path goes through
  the new `departed` loop instead); `ConditionPrimitive`'s own TODO block (added a second TODO entry
  naming exactly which departure paths and filter shapes are still open).

**STRUCT DEVIATIONS:** None beyond what the pre-slice report's concrete plan already named
(`retire_object`'s return type, `Engine`'s new transient field, `PendingTrigger`'s reuse — no new
fields needed there, since "nth departed ability for this source" fit the existing `ability_index: u32`
shape without changes). `RegisteredAbility` needed `#[derive(Clone)]` added (it had no derives at all
before this slice) — not explicitly named in the pre-slice report, but a direct, obvious consequence of
needing to snapshot abilities into a `Vec` before their source is purged.

**INTERFACE DEVIATIONS:** `sba::commit_death`/`resolve_deaths`/`check_state_based_actions`'s signature
changes were anticipated by the pre-slice report's concrete plan. One deviation the report didn't
call out: `resolve_deaths`'s `NeedsOrderingChoice` pause branch already silently dropped any
`resolved: Vec<GameEvent>` accumulated earlier in the *same* `resolve_deaths` call before this slice
(a pre-existing bug, not introduced here) — extending that same function to also accumulate
`departed_triggered: Vec<RegisteredAbility>` means this slice's new data inherits that exact drop-on-
pause behavior too. Only matters when two-or-more creatures die in the same SBA pass and a later one
(not the first) needs a CR 616.1 ordering choice; none of this slice's own tests exercise that
interleaving. Left as-is and flagged inline in sba.rs rather than silently fixed mid-slice — a
follow-up, not part of this slice's scope.

**SCOPE DEVIATIONS:** None beyond the pre-slice report's own explicitly-scoped narrowing (SBA-death
path only; source-identity conditions only, not filtered "a creature you control dies" conditions).

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Sacrifice, discard-as-cost, and a resolving spell's other zone departures all still call
  `commands::retire_object` but discard its returned abilities — a "when this dies" ability on a
  sacrificed/discarded permanent won't fire yet. Same mechanism (`Engine`'s stash), just not wired at
  those three call sites this slice.
- `CleanupSbaCheck`'s own path deliberately discards `departed` rather than stashing it — that flow
  never reaches `CollectTriggers` at all (backlog item 7a's still-open cleanup half), so nothing would
  ever consume it; stashing it anyway would just leak until the next real pipeline run.
- "Whenever a creature you control dies" / any filter over the dying object's characteristics rather
  than exact source identity — `Dies`/`LeavesBattlefield` only ever match the ability's own source.
- The pre-existing `resolve_deaths` drop-on-pause bug named above (STRUCT/INTERFACE DEVIATIONS) —
  surfaced by this slice's own refactor, not fixed by it.
- Once-per-turn/intervening-if legality (CR 603.2-603.3) — untouched, same as every prior sub-slice.

## Phase 5: phase5/draw-combat-end-triggers slice (backlog item 7, fourth sub-slice)

Branch `phase5/draw-combat-end-triggers`, off master. Bundles two independent fixes named in the
dies-triggers slice's own UNRESOLVED list: extending the CR 603.10 last-known-information stash to
the Sacrifice/Discard cost-payment departure path, and adding the "cheap follow-up" condition shapes
`AdvanceTurnStep`'s pipeline fix (7a) was always going to unlock — draw-a-card, beginning-of-combat,
beginning-of-end-step.

**Key scoping finding, checked before writing code:** of the three remaining `retire_object` call
sites the dies-triggers slice left discarding their return value, only one is actually wireable
today. `move_to_graveyard_for_cost`'s `Sacrifice` arm moves Battlefield→Graveyard — the exact shape
`Dies`/`LeavesBattlefield` match — and `submit_payment` was confirmed to already resume at
`ProcessState::SbaLoop` (not `log_events`), so no 7a-style pipeline rewire was needed, just the same
stash-threading `commit_death` already does. `submit_discard`'s CR 514.1 hand-size discard happens
during `Cleanup`, which still bypasses `collect_triggers` entirely (7a's still-open cleanup half) —
wiring the stash there would be inert. `draw_step`'s library-side retirement purges an old
*library*-zone id, which can never produce a Battlefield-departure LKI either. Both were excluded
from this slice's scope rather than wired for no observable effect.

**VERIFICATION RESULT:** Pass. 18 new unit tests in `types/ability.rs` (`DrawsCard`/`EachDrawsCard`,
`BeginningOfYourCombat`/`BeginningOfEachCombat`, `BeginningOfYourEndStep`/`BeginningOfEachEndStep` —
matching/non-matching cases each, mirroring the existing Upkeep pair's own test shape). New
`phase5_draw_combat_end_triggers.rs` — 9 end-to-end tests driving the real turn loop (mirroring
`phase5_upkeep_triggers.rs`): each of the three new condition pairs fires and resolves for its own
controller, fails to fire for a different controller (the "Your" variant), and fires regardless of
whose turn it is (the "Each" variant). New `phase5_sacrifice_dies_trigger.rs` — 3 end-to-end tests
proving the LKI wiring extension: a "when this dies" ability fires and resolves after its source is
sacrificed as an activated ability's cost (not just SBA-driven combat death); a bystander creature's
own Dies ability does not fire just because *some* sacrifice happened in the same payment; and a
regression proving Discard's own half of the same code path captures departed abilities without any
behavior change (no condition matches a Hand-departure LKI, so a Dies ability on a discarded card
still doesn't fire). `cargo test --workspace` green throughout (all pre-existing suites unchanged),
including every prior trigger-slice suite (`phase5_upkeep_triggers`, `phase5_dies_triggers`,
`phase5_damage_dealt_triggers`).

**What got implemented:**
- `commands::move_to_graveyard_for_cost` changed from `-> Option<GameEventKind>` to
  `-> Option<(GameEventKind, Vec<RegisteredAbility>)>` — returns `retire_object`'s departed
  `Triggered`-kind abilities alongside the event instead of discarding them, same tuple-return shape
  `sba::commit_death` already established.
- `commands::commit_payment` changed from `-> Vec<GameEventKind>` to
  `-> (Vec<GameEventKind>, Vec<RegisteredAbility>)` — both `Sacrifice`/`Discard` arms fold
  `move_to_graveyard_for_cost`'s departed abilities into a single accumulator threaded up to the
  caller.
- `Engine::submit_payment` (engine.rs) extends `self.departed_triggered_abilities` with
  `commit_payment`'s returned abilities before resuming at `ProcessState::SbaLoop` — populated
  directly at this call site rather than via `finish_sba_loop`, since `submit_payment`'s pause/resume
  doesn't route through that function.
- `ConditionPrimitive` (ability.rs) gained 6 variants: `DrawsCard`/`EachDrawsCard` (matches
  `ZoneChange{from: Library, to: Hand}`, filtered on `last_known_info.controller` — no `matches()`
  signature change needed, that field was already present on every `ZoneChange`),
  `BeginningOfYourCombat`/`BeginningOfEachCombat` (matches `StepStarted{step: Step::BeginningOfCombat}`),
  `BeginningOfYourEndStep`/`BeginningOfEachEndStep` (matches `StepStarted{step: Step::End}`) — all
  three pairs mirror `BeginningOfYourUpkeep`/`BeginningOfEachUpkeep`'s existing Your/Each split
  exactly, a deliberate consistency choice (see STRUCT DEVIATIONS) over a parameterized single
  variant.
- `Game.Conditions.DrawsCard`/`EachDrawsCard`/`BeginningOfCombat`/`EachBeginningOfCombat`/`EndStep`/
  `EachEndStep` bindings (script.rs), same one-line-per-variant pattern every prior condition uses.
- Updated `move_to_graveyard_for_cost`/`retire_object`/`ConditionPrimitive`'s TODO block doc comments
  to describe the new state accurately (two wired departure paths instead of one, which two call
  sites remain genuinely inert, once-per-turn/intervening-if and Cleanup's gap still open).

**STRUCT DEVIATIONS:** None on the `ConditionPrimitive` enum itself. One real design fork surfaced
mid-planning (not mid-code): whether the 3 new condition families should be flat Your/Each pairs
(matching Upkeep's existing precedent) or a single variant parameterized by a `TurnScope`-style enum.
Decided flat pairs before writing code, specifically to avoid two different patterns for "the same
shape of gap" sitting side by side in `ConditionPrimitive` (Upkeep already shipped as flat pairs;
retrofitting it to match a parameterized approach would have meant touching already-merged/tested
code for a pure style change) and to avoid forking `Game.Conditions.*`'s binding convention (every
existing entry is a plain table value, not a function call — a parameterized variant would need the
latter for these specific conditions only).

**INTERFACE DEVIATIONS:** `move_to_graveyard_for_cost`/`commit_payment`'s signature changes were
anticipated by the pre-slice plan exactly (same tuple-return shape `commit_death` already
established) — not a deviation.

**SCOPE DEVIATIONS:** None — no `GameState`/`CardDefinition` literal sweep needed (no new fields on
either type), everything landed in `commands.rs`, `engine.rs`, `types/ability.rs`, `script.rs`, and
two new test files.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- `submit_discard`'s CR 514.1 hand-size discard and `draw_step`'s library-side retirement still
  discard their departed abilities outright — both excluded from this slice's scope as inert (see
  the pre-slice scoping finding above), not an oversight.
- Once-per-turn/intervening-if legality (CR 603.2-603.3) — untouched, same as every prior sub-slice.
- `Cleanup`'s own trigger-collection gap (its `CleanupSbaCheck` flow still doesn't run
  `collect_triggers` at all) — untouched, same as every prior sub-slice since 7a.
- "Whenever a creature you control dies"/"whenever a player draws their second card"/any filter over
  the departing or drawing object's characteristics rather than a fixed source identity or plain
  controller check — none of the new conditions, or `Dies`/`LeavesBattlefield` before them, support a
  `Script`-backed or filter-scoped shape yet.
- CR 121.5's "moved without the word 'draw'" distinction — `DrawsCard`/`EachDrawsCard` match by
  zone-shape only (`Library`→`Hand`); no non-draw effect produces that shape in this engine yet, so
  this is a documented limitation, not a live bug.

## Phase 5: phase5/layer-engine-switching slice (backlog item 1, sub-slice D)

Branch `phase5/layer-engine-switching`, off master. Slice backlog item 1's sub-slice D — CR 613.4d
(layer 7e in this codebase's own local sublayer numbering, since `PowerToughnessCounters`/7d is
already spoken for by the counters-as-base-simplification `layers.rs` established in the
layer-engine-basics slice). Last of the 5 real-P/T sublayers; layers 1/2/3 and the CR 613.6-8
dependency system stay explicitly last per the backlog's own ordering.

**Key finding before writing code:** because layer 7c (`PowerToughnessModifying`) already sums every
applicable `ModifyPT` into one number before this slice's new 7e pass ever runs, CR 613.4d's own
worked example (a modify effect at a *later* timestamp than a switch still contributes to the
pre-switch sum) falls out correctly for free — no timestamp-interleaving logic needed between 7c and
7e, just a second loop after the existing one.

**VERIFICATION RESULT:** Pass. New `phase5_layer_engine_switching.rs` — 5 tests: a single switch
swaps power/toughness; CR 613.4d's own first worked example (modify, switch, later modify → `4/6`,
not `9/1`); CR 613.4d's own third worked example (two switches on the same object cancel out); an
`EffectFilter` still scopes a switch effect to its intended controller, same as every prior layer
slice's filter test; a switch effect on an object with no resolvable power/toughness (e.g. a
non-creature with neither a printed value nor a `SetPT` applying to it) is a no-op, not a panic or a
fabricated value. `cargo test --workspace` green throughout, no regressions in any prior layer-engine
suite (`phase5_layer_engine_basics`/`_consumers`/`_types_colors`/`_cda`).

**What got implemented:**
- `EffectPrimitive::SwitchPT` (effect.rs) — a unit variant (no fields, it's a toggle not a value).
- `layers::effective_characteristics` — a new loop over `Layer::PowerToughnessSwitching` effects
  (timestamp order, same `applicable_effects_in_layer` helper every other layer uses), placed after
  the existing 7c loop. Swaps `power`/`toughness` only when both are already `Some` — skipped
  otherwise, same "skip rather than guess" stance every other unresolvable value in this query
  already takes.
- `script.rs`: `(Layer::PowerToughnessSwitching, EffectPrimitive::SwitchPT)` added to
  `validate_layer_effect_pairing`; `Game.Layers.PowerToughnessSwitching` and `Game.Effects.SwitchPT`
  bindings. `SwitchPT` is bound as a plain tagged value (not a function), same convention
  `Game.Costs.TapSelf` already uses for a zero-argument tag.
- Updated three doc comments to stop saying "5 layers"/list 7e among the unbuilt ones:
  `Layer`'s own doc comment (effect.rs), `layers::effective_characteristics`'s doc comment (added a
  paragraph explaining why the layer-then-switch pass order reproduces CR 613.4d's example for free),
  and `bind_game_globals`'s "only N layers are exposed" comment (script.rs).

**STRUCT DEVIATIONS:** None — matched the pre-slice requirements report exactly.

**INTERFACE DEVIATIONS:** None.

**SCOPE DEVIATIONS:** None — no `GameState`/`CardDefinition` literal sweep needed (no new fields on
either type), everything landed in `effect.rs`, `layers.rs`, `script.rs`, and one new test file.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Layer 7d (counters) stays the pre-existing base-level simplification (folded into the base P/T
  before any continuous effect, including this slice's own switch, is applied) — not CR 613.4's
  literal timestamp-interleaving of counters alongside 7b/7c/7e, unchanged by this slice.
- Layers 1/2/3 (copy/control/text-changing) and the CR 613.6-8 dependency system — untouched, same
  as every prior layer-engine sub-slice's own UNRESOLVED list. Backlog item 1 is now fully closed
  except for these, which stay last per the backlog's own explicit ordering.
- No `Game.AddContinuousEffect`/`Game.CreateStaticAbility`-driven end-to-end test exercises
  `SwitchPT` yet (this slice's tests build `ContinuousEffect`s via struct literal, same bootstrap
  stance every prior layer-engine slice took before `static-ability-self-registration` landed) — a
  real card script using `Game.Effects.SwitchPT` is exercised the same way any other layer's
  primitive already is via that builder, just not proven with a dedicated test this slice.

## Phase 5: phase5/script-ops-damage slice (backlog item 8, first Script API primitive)

Branch `phase5/script-ops-damage`, off master. First of backlog item 8's missing `Game.Ops`
primitives beyond `GainLife` — `AbilityEffect::Damage(u32)`/`Game.Ops.Damage(n)`, e.g. "Shock deals
2 damage to any target."

**Key finding before writing code:** `ResolveStackTop` already builds an `events: Vec<GameEventKind>`
for whatever's resolving and routes it through the real `ReplacementCheck`→`CommitEvents` pipeline —
and `CommitEvents` already special-cases `GameEventKind::DamageDealt` by calling `commands::
apply_damage_event`, which isn't combat-specific (just `source`/`target`/`amount`). So `Damage`
doesn't mutate `GameState` directly the way `GainLife` does (a real pre-existing gap: `GainLife`
bypasses replacement checking and doesn't even emit a `LifeChanged` event) — it emits a real
`DamageDealt` event and reuses the exact pipeline combat damage already runs through, getting
prevention/replacement effects and Lifelink/Wither/Infect/Deathtouch for free.

**Two scope surprises found mid-implementation, each flagged and resolved with the human before
proceeding rather than silently improvised:**
1. `ResolveStackTop`'s effect-application match only ever ran for `StackEntryKind::TriggeredAbility`/
   `ActivatedAbility` — a cast **spell** got zero effect resolution at all (only the zone-change
   half), so "Shock deals 2 damage" wouldn't actually work as a real castable spell even with
   `Damage` added. Decided to widen the match to include instant/sorcery spells too (a permanent
   spell is deliberately excluded — CR 608.2m says it just enters the battlefield; any ETB behavior
   is a separate `Triggered`-kind `RegisteredAbility`, not this field).
2. Widening that match wasn't sufficient on its own: `commands::CastSpell` hardcoded
   `ScriptHandle(0)`/`None` for every cast spell's `operation`/`effect` — there was no lookup from
   the card's own definition at all. Decided to go full scope: `CardDefinition` gained a real
   `effect: Option<AbilityEffect>` field, and `CastSpell` now bakes it onto the pushed `StackObject`.

**VERIFICATION RESULT:** Pass. New `phase5_script_ops_damage.rs` — 5 end-to-end tests through the
real engine: a cast "deals 2 damage to any target" instant marks damage on a creature target; the
same spell reduces a player target's life; an *activated ability* (not just a spell) with a
Lifelink source dealing damage gains its controller life, proving `apply_damage_event`'s existing
keyword handling is reused, not reimplemented; a `PreventDamage` replacement effect registered
against the target stops a *scripted spell's* damage the same way it already stops combat damage
(the concrete proof reusing the real pipeline was worth it over `GainLife`'s shortcut); a fizzled
spell (illegal target at resolution) deals no damage at all and emits no `DamageDealt` event.
`cargo test --workspace` green throughout (all pre-existing suites unchanged) — notably including
every existing `phase5_targeting_basics`/`phase5_cast_spell*`/`phase5_activate_ability_*` test,
confirming the `CardDefinition.effect` sweep and the widened `ResolveStackTop` gate didn't change
behavior for any card that doesn't set `effect`.

**What got implemented:**
- `AbilityEffect::Damage(u32)` (types/ability.rs) — single fixed target only, no "divide among any
  number of targets" (Arc Trail-style) or "each opponent" mass-damage shape; matches
  `targeting-basics`'s own existing narrowing.
- `CardDefinition.effect: Option<AbilityEffect>` (types/card.rs) — a spell's Rust-native fast-path
  effect, same shape `RegisteredAbility.effect` already uses for abilities. Mechanical sweep: every
  `CardDefinition` struct literal across the workspace (56 occurrences, 40 files, both
  `crates/engine` and `crates/scenario`) gained `effect: None,` — a scripted GNU-sed pass anchored
  on the unique `alternative_costs: Vec::new(),` last-field line, plus one manual fix for the single
  literal with a non-empty `alternative_costs` (`phase5_flashback_alternative_costs.rs`).
- `commands::execute_command`'s `CastSpell` arm — reads `last_known_info.definition.effect.clone()`
  into a new `effect` local (captured from the same clone `last_known_info`/CR 603.10 already takes,
  before the zone-move mutations that would otherwise require a fresh borrow) and passes it to
  `StackObject::new` instead of the old hardcoded `None`.
- `commands::execute_command`'s `ActivateAbility` arm — `Some(AbilityEffect::Damage(_))` added
  alongside `Some(AbilityEffect::GainLife(_))` in the "non-mana effect, uses the stack" match pattern
  (`non_mana_effect @ (None | Some(GainLife(_)) | Some(Damage(_)))`).
- `engine.rs`'s `ResolveStackTop`: a new `is_instant_or_sorcery` bool (computed once, before the
  zone-move removes `top.stack_id` from `state.objects`) widens the effect-application gate to
  `StackEntryKind::TriggeredAbility | ActivatedAbility` **or** `is_instant_or_sorcery`. New
  `Some(AbilityEffect::Damage(amount))` arm: if `top.targets.first()` exists, pushes
  `GameEventKind::DamageDealt { source: top.source, target: *t, amount }` onto `events` (instead of
  mutating state directly, unlike `GainLife`'s existing arm) — empty targets is a silent no-op
  (shouldn't be reachable given cast-time target validation).
- `Game.Ops.Damage(amount)` binding (script.rs), same one-line pattern `GainLife` already uses.

**STRUCT DEVIATIONS:** `CardDefinition.effect` wasn't part of the original narrow plan (a wider match
arm) — surfaced mid-implementation as described above, confirmed with the human before proceeding.

**INTERFACE DEVIATIONS:** None beyond the anticipated `CastSpell`/`ResolveStackTop` changes once the
wider scope was agreed.

**SCOPE DEVIATIONS:** The 40-file `CardDefinition` literal sweep — mechanical, same "scripted pass
anchored on a unique last-field line" precedent the cost-modifiers slice's own `GameState` sweep
established, not independently scoped but a direct, necessary consequence of the widened plan.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report and the two scope
decisions above):**
- `Destroy` (CR 701.8/702.12b) is next in backlog item 8 — needs its own zone-move-commit design;
  unlike `Damage`, `CommitEvents`'s generic loop doesn't perform zone moves for an arbitrary
  `ZoneChange` the way it special-cases `DamageDealt` (that logic today only exists inside
  `sba.rs`'s SBA-death-specific `resolve_deaths`/`commit_death`).
  `DrawCards`/`Counters`/`Exile`/`Sacrifice`/`CreateToken` — still no `Game.Ops` primitive.
- `GainLife`'s own inline-mutate-no-replacement-check shortcut is untouched — a pre-existing gap,
  now inconsistent with `Damage`'s more correct event-based shape, flagged but not fixed.
- No TOML scenario field for `CardDefinition.effect` yet (`crates/scenario`'s two `CardDefinition`
  builders both hardcode `effect: None` with an inline TODO) — same "direct struct literal only"
  limit `alternative_costs` already had before its own scenario-field slice, if one ever lands.
- No "divide damage among any number of targets" or "each opponent" mass-damage shape — single
  fixed target only, per `AbilityEffect::Damage`'s own doc comment.

## Phase 5: phase5/script-ops-destroy slice (backlog item 8, second Script API primitive)

Branch `phase5/script-ops-destroy`, off master. Second of backlog item 8's missing `Game.Ops`
primitives — `AbilityEffect::Destroy`/`Game.Ops.Destroy()` (CR 701.8), e.g. "Destroy target
creature," named as its own follow-up when `Damage` landed since it needed a different design.

**Key finding, confirmed before writing code:** `Damage` worked by pushing an unapplied
`GameEventKind::DamageDealt` event into the normal `events` vec, since `CommitEvents` already
special-cases that event kind to retroactively apply it. A `ZoneChange` event has no such property
— every other zone-move site in this codebase (`PlayLand`, discard, `CastSpell`'s own hand→stack
move) performs the mutation *first*, then pushes `ZoneChange` purely as a description of something
that already happened. The one place that already does "propose a Battlefield→Graveyard move,
replacement-check it, then commit it" is `sba.rs`'s `resolve_deaths`/`commit_death` — SBA-specific,
but `commit_death` itself is a generic, reusable `pub(crate)` function. Reused its per-object body
inline in `ResolveStackTop` (propose → `replacement::apply_replacements` → `sba::commit_death`),
same "resume exactly where CommitEvents would otherwise have gone" manual-ledger-append convention
`Engine::submit_payment` already established for a Sacrifice cost payment.

**Scope decision, confirmed before writing code:** CR 616.1's ordering choice (2+ `ReplacementEffect`s
matching the same destroy) isn't supported this slice — handling `ReplacementOutcome::Resolved` only
and no-oping on `NeedsOrderingChoice`, same "narrower than full rules text, named not silently
skipped" precedent `replacement-prevention-effects` set before `fix-replacement-bypasses` added real
pause support as its own follow-up.

**VERIFICATION RESULT:** Pass. New `phase5_script_ops_destroy.rs` — 5 end-to-end tests through the
real engine: a cast "Destroy target creature" spell moves a real creature to its owner's graveyard
and its own "when this dies" trigger fires (proving the `departed_triggered_abilities` stash reused
correctly, same mechanism the Sacrifice-cost slice wired up); an indestructible target survives
completely untouched, no zone change logged at all; a `WouldDie` replacement effect (exile-instead)
redirects the destroy to exile instead of the graveyard, mirroring `phase5_dies_triggers.rs`'s own
exile-redirect test; an *activated ability* carrying `Destroy` works identically to a spell (same
dual-path proof `Damage` established); a fizzled spell (illegal target at resolution) destroys
nothing. `cargo test --workspace` green throughout, no regressions in any prior suite — notably
`phase5_dies_triggers`/`phase5_sba_death_replacement`/`phase5_script_ops_damage` all unchanged.

**What got implemented:**
- `AbilityEffect::Destroy` (types/ability.rs) — unit variant, no fields (a toggle, same shape as
  `SwitchPT`).
- `commands::effective_keywords` changed from private to `pub(crate)` — the CR 702.12b
  indestructible check needed it from `engine.rs`, same visibility every other cross-module helper
  here already has.
- `engine.rs`'s `ResolveStackTop`: a new `Some(AbilityEffect::Destroy)` arm. If
  `top.targets.first()` is `Some(Target::Object(id))` and `id` currently resolves on the
  battlefield: checks `INDESTRUCTIBLE` via `effective_keywords` first (CR 702.12b — an absolute
  immunity, not a replacement effect); if not indestructible, proposes
  `GameEventKind::ZoneChange{object: id, from: Battlefield, to: Graveyard, last_known_info: Some(..)}`
  and runs it through `replacement::apply_replacements` inline. On `Resolved`, calls
  `sba::commit_death`, folds the returned departed abilities into
  `self.departed_triggered_abilities`, and pushes the resulting real `GameEvent` straight onto
  `self.state.ledger` (already has its own `event_id`/`sequence_no` from `commit_death`, unlike the
  `events: Vec<GameEventKind>` convention every other arm uses). On `NeedsOrderingChoice`: no-op,
  per the scope decision above. Any other target shape (player, already-gone object) is a silent
  no-op.
- `commands::execute_command`'s `ActivateAbility` arm — `Some(AbilityEffect::Destroy)` added
  alongside `GainLife`/`Damage` in the "non-mana effect, uses the stack" match pattern.
- `Game.Ops.Destroy()` binding (script.rs) — a plain zero-arg tagged value (`OperationTag(
  AbilityEffect::Destroy)`), same convention `Game.Costs.TapSelf`/`Game.Effects.SwitchPT` already
  use for a fieldless primitive.

**STRUCT DEVIATIONS:** None — matched the pre-slice requirements report exactly.

**INTERFACE DEVIATIONS:** `commands::effective_keywords`'s visibility change (`fn` → `pub(crate) fn`)
was anticipated by the pre-slice plan (needed from `engine.rs`) — not a deviation.

**SCOPE DEVIATIONS:** None — no `GameState`/`CardDefinition` literal sweep needed (no new fields on
either type), everything landed in `types/ability.rs`, `commands.rs`, `engine.rs`, `script.rs`, and
one new test file.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- CR 616.1's ordering choice for a scripted Destroy facing 2+ matching replacement effects — no
  resume-target shape threads back into mid-`ResolveStackTop` yet; silently no-ops rather than
  guessing an order. A real follow-up would need a new `ReplacementResumeTarget`-style variant
  carrying enough context to re-enter `ResolveStackTop`'s tail after the choice.
- Regeneration (CR 701.19) still isn't modeled at all — but `Destroy` already runs through the real
  `replacement::apply_replacements` pipeline, so a future regeneration `ReplacementEffect` payload
  would interact correctly with zero changes needed here.
- `DrawCards`/`Counters`/`Exile`/`Sacrifice`/`CreateToken` — still no `Game.Ops` primitive; next in
  backlog item 8.
- Single fixed target only, same narrowing `Damage`/targeting-basics already established.

## Phase 5: phase5/script-ops-counters slice (backlog item 8, third Script API primitive)

Branch `phase5/script-ops-counters`, off master. Third of backlog item 8's missing `Game.Ops`
primitives — `AbilityEffect::Counters { kind: CounterKind, amount: i32 }`/`Game.Ops.Counters(kind,
n)` (CR 122.1a), e.g. "Put two +1/+1 counters on target creature."

**Key finding, confirmed before writing code:** cheaper than `Destroy` — `GameEventKind::
CounterAdded { object, kind, amount }` already existed in the event enum as complete dead code
(never constructed, never matched anywhere), and it's the exact shape needed. Reused `Damage`'s
"push an unapplied event into `events`, let `CommitEvents` retroactively apply it" pattern rather
than `Destroy`'s heavier inline replacement-check path, since there's no existing counter-
replacement concept to hook into (no `EventFilter::CounterAdded` shape exists — `EventFilter` only
has `DamageDealt`/`WouldDie`).

**Scope narrowing, confirmed before writing code:**
- Object targets only — `CounterAdded` only carries `object: ObjectId`, not a generic `Target`.
  Player-facing counters (Energy/Experience/Poison-via-effect) deferred as a named follow-up
  (would need widening this event or a parallel one).
- No replacement-effect modeling for counter placement — same pre-existing gap `GainLife`'s
  inline-mutate shortcut already has, now also true of `Counters` (unlike `Damage`, which got the
  real prevention/replacement pipeline for free by reusing `DamageDealt`).
- CR 704.5q's +1/+1-vs--1/-1 annihilation SBA doesn't exist at all (grepped, confirmed nothing).
  Both kinds now coexist un-annihilated forever — a genuine pre-existing gap, not introduced here.
  `net_counters` (already used by `layers::effective_characteristics`) nets them correctly for P/T
  purposes regardless, so visible behavior stays correct even without the SBA; only the literal
  counter bookkeeping piles up. Named, not fixed.

**VERIFICATION RESULT:** Pass. New `phase5_script_ops_counters.rs` — 6 end-to-end tests through the
real engine: a cast "put two +1/+1 counters on target creature" spell resolves with `CounterSet::
count` and effective power/toughness (via `layers::effective_characteristics`) both reflecting it;
same for -1/-1 counters (amount 1, not 2, to avoid CR 704.5g killing the 2/2 test creature before
the assertions run); both kinds coexisting on one creature (3 +1/+1, 1 -1/-1) prove no annihilation
happens (literal counts both persist) while `net_counters` still nets the correct effective P/T
anyway; an *activated ability* (not just a spell) with `Counters` works identically, same dual-path
proof `Damage`/`Destroy` established; an unrecognized kind name (`"Skull"`) round-trips through
`ScriptRuntime::load_operation("return Game.Ops.Counters(\"Skull\", 3)")` to `CounterKind::
Custom("Skull")`; a fizzled spell (illegal target at resolution) adds no counters and emits no
`CounterAdded` event. `cargo test --workspace` green throughout — no regressions in any prior suite.

**What got implemented:**
- `AbilityEffect::Counters { kind: CounterKind, amount: i32 }` (types/ability.rs).
- `engine.rs`'s `ResolveStackTop`: a new `Some(AbilityEffect::Counters { kind, amount })` arm —
  if `top.targets.first()` is `Some(Target::Object(id))`, pushes `GameEventKind::CounterAdded
  { object: id, kind: kind.clone(), amount: *amount }` into `events`; any other target shape is a
  silent no-op, same convention every other arm here uses.
- `engine.rs`'s `CommitEvents` `for kind in checked` loop gained a second special-case (right next
  to the existing `DamageDealt` one): `if let GameEventKind::CounterAdded { object, kind, amount }
  = &kind { ... obj.counters.add(kind.clone(), *amount) }`.
- `commands.rs`'s `ActivateAbility` non-mana-effect match pattern gained
  `Some(AbilityEffect::Counters { .. })` alongside `GainLife`/`Damage`/`Destroy`.
- `parse_counter_kind_name(name: &str) -> CounterKind` (script.rs) — recognized names
  (`PlusOnePlusOne`/`MinusOneMinusOne`/`Loyalty`/`Poison`/`Energy`/`Experience`/`Charge`) map to
  their variant; anything unrecognized becomes `CounterKind::Custom(name)` rather than an error —
  matches `CounterKind`'s own doc comment ("the long tail of flavor-named counters falls back to
  `Custom`"), unlike `parse_keyword_name`/`parse_card_type_name` which do error on an unknown name.
- `Game.Ops.Counters(kindName, amount)` binding (script.rs) — a two-arg Lua function (unlike
  `Destroy`'s zero-arg tagged value or `Damage`'s single-arg one), decoding into
  `OperationTag(AbilityEffect::Counters { kind: parse_counter_kind_name(&kind), amount })`.

**STRUCT DEVIATIONS:** None — matched the pre-slice requirements report exactly.

**INTERFACE DEVIATIONS:** None.

**SCOPE DEVIATIONS:** None — no `GameState`/`CardDefinition` literal sweep needed (no new fields on
either type), everything landed in `types/ability.rs`, `commands.rs`, `engine.rs`, `script.rs`, and
one new test file.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Player-target counters (Energy/Experience/Poison-via-effect) — `CounterAdded` only carries
  `object: ObjectId`; would need widening this event or a parallel one.
- No replacement/prevention modeling for counter placement — no `EventFilter::CounterAdded` shape;
  same class of gap `GainLife`'s inline-mutate shortcut already has.
- CR 704.5q's +1/+1-vs--1/-1 annihilation SBA still doesn't exist — pre-existing, unrelated to this
  slice, but now more visibly relevant since cards can actually put both kinds on one creature.
- `DrawCards`/`Exile`/`Sacrifice`/`CreateToken` — still no `Game.Ops` primitive; next in backlog
  item 8.

## Phase 5: phase5/script-ops-drawcards slice (backlog item 8, fourth Script API primitive)

Branch `phase5/script-ops-drawcards`, off master. Fourth of backlog item 8's missing `Game.Ops`
primitives — `AbilityEffect::DrawCards(u32)`/`Game.Ops.DrawCards(n)` (CR 120.3/121), e.g. "Draw two
cards."

**Key finding, confirmed before writing code:** cheapest of the four so far. A `ZoneChange` always
describes a move that already happened (`Destroy`'s own doc comment) — `draw_step` already proved
this convention for the natural draw step: mutate state immediately, then push the resulting real
event purely so `CollectTriggers`'s ledger scan picks it up. Rather than re-deriving that, extracted
`draw_step`'s ~15-line body into a shared `Engine::draw_one_card(&mut self, player: PlayerId) ->
Option<GameEventKind>` helper, parameterized by player instead of hardcoded `active_player`;
`draw_step` now calls it once, the new `ResolveStackTop` arm calls it in a loop `amount` times. No
`CommitEvents` special-case needed at all — the only one of the four primitives without one.

**Scope decision, confirmed before writing code:** no target — always draws for `top.controller`,
mirroring `GainLife`'s exact "controller only" shape rather than `Damage`/`Destroy`/`Counters`'s
object-target shape, since "Draw a card"/"you draw two cards" is the overwhelmingly common real
card text vs. "target player draws." `Target::Player` support deferred as a named follow-up.

**VERIFICATION RESULT:** Pass. New `phase5_script_ops_drawcards.rs` — 5 end-to-end tests through the
real engine: a cast "draw two cards" spell puts two new cards in the caster's hand and empties a
2-card library; the scripted draw fires an already-existing `DrawsCard`-conditioned trigger (proves
the real `ZoneChange` event reaches `collect_triggers`, not just a bespoke assertion — first time
`DrawsCard` fires from anything other than the natural draw step); requesting 2 draws from a
1-card library draws the 1 available card and stops cleanly, no crash, no phantom card; an
*activated ability* (not just a spell) with `DrawCards` works identically, same dual-path proof
every prior primitive established; drawing 2 from a 3-card library preserves order — the correct
top-two cards land in hand in order, the third stays on top of the library. `cargo test --workspace`
green throughout, no regressions in any prior suite.

**What got implemented:**
- `AbilityEffect::DrawCards(u32)` (types/ability.rs).
- `engine.rs`: `draw_step`'s body extracted into `draw_one_card(&mut self, player: PlayerId) ->
  Option<GameEventKind>` (same logic, parameterized instead of hardcoding `active_player`);
  `draw_step` itself now just calls it once for `active_player`.
- `engine.rs`'s `ResolveStackTop`: a new `Some(AbilityEffect::DrawCards(amount))` arm — loops
  `0..*amount`, calling `draw_one_card(top.controller)` each time and pushing the returned event
  into `events`; breaks early (rather than continuing to loop pointlessly) the first time it
  returns `None` (empty library).
- `commands.rs`'s `ActivateAbility` non-mana-effect match pattern gained
  `Some(AbilityEffect::DrawCards(_))` alongside `GainLife`/`Damage`/`Destroy`/`Counters`.
- `Game.Ops.DrawCards(amount)` binding (script.rs), same one-arg-function pattern `Damage` uses.

**STRUCT DEVIATIONS:** None — matched the pre-slice requirements report exactly.

**INTERFACE DEVIATIONS:** `draw_step`'s body becoming a separate `draw_one_card` helper was
anticipated by the pre-slice plan (needed for the loop) — not a deviation.

**SCOPE DEVIATIONS:** None — no `GameState`/`CardDefinition` literal sweep needed, everything landed
in `types/ability.rs`, `engine.rs`, `commands.rs`, `script.rs`, and one new test file.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- `Target::Player` "target player draws N cards" variant — deferred, `DrawCards` only ever draws
  for `top.controller`.
- CR 704.5b's empty-library-loss SBA still doesn't exist — pre-existing gap `draw_step` itself
  already carried, inherited not fixed.
- No replacement/prevention modeling for draw effects — no `EventFilter::DrawsCard`/"would draw"
  shape exists; same class of gap `Counters`/`GainLife` already have.
- `Sacrifice`/`CreateToken` — still no `Game.Ops` primitive; next in backlog item 8.

## Phase 5: phase5/script-ops-exile slice (backlog item 8, fifth Script API primitive)

Branch `phase5/script-ops-exile`, off master. Fifth of backlog item 8's missing `Game.Ops`
primitives — `AbilityEffect::Exile`/`Game.Ops.Exile()`.

**Key finding before writing code:** `sba::commit_death` cannot be reused for a scripted Exile
even though the `Destroy` arm reuses it for SBA-driven deaths. `commit_death` calls
`state.zone_mut(to, Some(owner))` — that works for Graveyard (per-player zone) and even when
a replacement redirects a death to Exile (because `commit_death` takes the event's final `to`
field, and the test fixture happened to use per-player exile zones, a pre-existing
inconsistency the flashback slice already documented). For a direct `Battlefield → Exile`
proposal with `to: ZoneKind::Exile` up front, `Some(owner)` finds nothing — the engine's
shared Exile zone (`owner = None`, built as a single instance in `build_initial_state`) is
invisible to that lookup. The `Exile` arm in `ResolveStackTop` inlines its own zone move with
`state.zone_mut(ZoneKind::Exile, None)` (a `match to { ZoneKind::Exile => None, _ => Some(owner) }`
pick so replacement effects that redirect exile to a per-player zone still land correctly).

Unlike `Destroy`, no indestructibility check — CR 702.12b is Destroy-specific; exile bypasses
indestructibility entirely. `apply_replacements` is still consulted (someone could replace
exile with something else via a replacement effect). Battlefield targets only — "exile target
creature/permanent" is the overwhelming majority of real card text vs. "exile target card from
a graveyard"; the latter requires a different zone lookup and is a named follow-up.

**VERIFICATION RESULT:** Pass. New `phase5_script_ops_exile.rs` — 5 end-to-end tests through
the real engine: a cast "exile target creature" spell moves the creature from the battlefield
to the shared Exile zone (verifies the `None`-owner zone lookup works end-to-end); an
activated ability with `Exile` works identically, same dual-path proof every prior primitive
established; exile fires a `LeavesBattlefield`-conditioned trigger (proves `retire_object` →
`departed_triggered` → `collect_triggers` path works for exile, not just SBA death — the
trigger resolves and grants 1 life as expected); an indestructible target is exiled anyway
(Destroy would skip it — Exile does not); exiling one of two permanents leaves the other
untouched. `cargo test --workspace` green throughout, no regressions in any prior suite.

**What got implemented:**
- `AbilityEffect::Exile` (types/ability.rs) — no fields, same toggle shape as `Destroy`.
- `script.rs`: `ops.set("Exile", OperationTag(AbilityEffect::Exile))?` — same value
  (non-function) binding pattern `Destroy` uses.
- `engine.rs`'s `ResolveStackTop`: a new `Some(AbilityEffect::Exile)` arm — checks
  `Target::Object` on battlefield, clones `last_known_info`, proposes `Battlefield → Exile`
  `ZoneChange`, runs `apply_replacements`, then on `Resolved` inlines the zone move: new id,
  `zone_mut(Battlefield, None).remove`, `retire_object` (stashing `departed_triggered`),
  `zone_mut(to, exile_owner).push_bottom` (where `exile_owner` is `None` for `Exile`, else
  `Some(owner)` for any replacement-redirected destination), `objects.insert`, ledger append.
- `commands.rs`'s `ActivateAbility` non-mana-effect match pattern gained
  `Some(AbilityEffect::Exile)` alongside the other primitives.

**STRUCT DEVIATIONS:** None — matched the pre-slice requirements report exactly.

**INTERFACE DEVIATIONS:** None — the inline zone-move approach (no `commit_death` reuse) was
called out as a pre-slice design decision, not a deviation.

**SCOPE DEVIATIONS:** None — landed in `types/ability.rs`, `engine.rs`, `commands.rs`,
`script.rs`, and one new test file only.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Non-battlefield exile (from graveyard, hand) — deferred, `Exile` only ever targets the
  battlefield today.
- CR 616.1 ordering choice if 2+ replacement effects intercept the same exile —
  `NeedsOrderingChoice` arm silently does nothing, same gap `Destroy` already carries.
- No replacement/prevention modeling for exile events — no `EventFilter::WouldExile` shape
  exists; same class of gap `Counters`/`GainLife`/`DrawCards` already have.
- `Sacrifice`/`CreateToken` — still no `Game.Ops` primitive; next in backlog item 8.

## Phase 5: phase5/script-ops-sacrifice slice (backlog item 8, sixth Script API primitive)

Branch `phase5/script-ops-sacrifice`, off master. Sixth of backlog item 8's missing `Game.Ops`
primitives — `AbilityEffect::Sacrifice`/`Game.Ops.Sacrifice()`.

**Key finding before writing code:** `sba::commit_death` IS reusable for scripted sacrifice,
unlike the Exile slice. The destination is the owner's per-player graveyard (`Some(owner)`), so
`state.zone_mut(to, Some(owner))` finds it correctly. The entire `Sacrifice` arm in
`ResolveStackTop` is the `Destroy` arm minus the indestructibility check — that's the full
implementation. "Sacrifice a permanent you control" (player chooses at resolution) needs Lua
coroutines and is blocked on backlog item 9; self-sacrifice ("sacrifice this permanent," no
target) is a named follow-up.

**Test setup note:** three tests use `card_object_controlled_by(target, ..., owner: p1, controller: p0)`
to separate the sacrificed creature's graveyard (p1's) from where the resolved instant lands
(p0's), matching the Destroy test pattern. The target filter is `FilterController::You`, which
validates against `controller == caster` (p0 == p0 ✓), so a p1-owned p0-controlled creature is
a legal target.

**VERIFICATION RESULT:** Pass. New `phase5_script_ops_sacrifice.rs` — 6 end-to-end tests
through the real engine: a cast "sacrifice target creature you control" spell moves the creature
to its owner's graveyard; an activated ability with `Sacrifice` works identically (dual-path
proof); a `Dies`-conditioned trigger fires off the sacrifice ZoneChange (`retire_object` →
`departed_triggered` path works for scripted sacrifice, not just cost-sacrifice or SBA death);
a `LeavesBattlefield`-conditioned trigger also fires; an indestructible target is sacrificed
anyway (Destroy would skip it — Sacrifice does not); sacrificing one of two permanents leaves
the other untouched. `cargo test --workspace` green throughout, no regressions in any prior suite.

**What got implemented:**
- `AbilityEffect::Sacrifice` (types/ability.rs) — no fields, toggle shape, same as `Destroy`/`Exile`.
- `script.rs`: `ops.set("Sacrifice", OperationTag(AbilityEffect::Sacrifice))?` — value binding.
- `engine.rs`'s `ResolveStackTop`: `Some(AbilityEffect::Sacrifice)` arm — identical to `Destroy`
  arm with the indestructibility check removed.
- `commands.rs`'s `ActivateAbility` non-mana-effect match gained `Some(AbilityEffect::Sacrifice)`.

**STRUCT DEVIATIONS:** None.

**INTERFACE DEVIATIONS:** None.

**SCOPE DEVIATIONS:** None — landed in `types/ability.rs`, `engine.rs`, `commands.rs`,
`script.rs`, and one new test file only.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- "Sacrifice a permanent you control" (player chooses at resolution) — blocked on coroutines
  (backlog item 9).
- Self-sacrifice ("sacrifice this permanent," no target) — named follow-up.
- CR 616.1 ordering choice if 2+ replacement effects intercept — same gap as Destroy/Exile.
- Pre-existing: `commit_death` silently loses the object if a replacement redirects to Exile
  (documented on `commit_death` itself, not introduced here).
- `CreateToken` — still no `Game.Ops` primitive; next in backlog item 8.

## Phase 5: phase5/script-ops-create-token slice (backlog item 8, final Script API primitive)

Branch `phase5/script-ops-create-token`, off master. Final `Game.Ops.*` primitive in backlog item
8 — `AbilityEffect::CreateToken { spec: TokenSpec, count: u32 }`, `Game.Tokens.*`, and
`Game.Ops.CreateToken(spec, count?)`.

**Key design decisions (locked in before writing code, per the requirements discussion):**
- **API shape:** `Game.Tokens.Clue()` / `Game.Tokens.Food()` / `Game.Tokens.Creature(subtypes, colors, power, toughness, opts?)` / `Game.Tokens.ArtifactCreature(...)` return `TokenSpec`-typed `TokenTag` UserData values; `Game.Ops.CreateToken(spec, count?)` wraps one into an `OperationTag`. No `Game.Tokens.Treasure` — blocked on Lua coroutines (backlog item 9; Treasure needs player input "add one mana of any color"). `count` defaults to 1.
- **Token zone entry:** direct insertion into `state.objects` + `state.zones[Battlefield]` via `push_bottom` — tokens originate on the battlefield, they don't travel through the stack. Does NOT go through `move_resolving_spell` (which moves an existing stack object).
- **Named token abilities:** Clue ({2}, Sacrifice → draw a card) and Food ({2}, Sacrifice → gain 3 life) are registered via `register_token_abilities` immediately at `CreateToken` resolution, pushing real `RegisteredAbility` entries into `state.abilities`. Creature tokens have no innate abilities.
- **`is_token: bool` on `GameObject`:** new field, always `false` on every non-token construction site. CR 111.7 cessation (tokens vanish when they leave the battlefield instead of going to the graveyard) is explicitly a TODO — tokens currently go to the graveyard like any other permanent. `is_token` exists so a future SBA or zone-change handler can key on it.
- **`run_card_script` skipped for tokens:** tokens have `ScriptReference::Inline("")` (empty). The empty string would fail parsing (`initial_effect` field not found), and token abilities are already registered directly via `register_token_abilities`. The `is_token` field provides the guard if a future cleanup ever needs it.
- **FilterSpec identity gap for named tokens:** Clue's/Food's sacrifice cost uses `FilterSpec { controller: You, card_types: Artifact, ... }` — this matches *any* artifact you control, not specifically this token. Same pre-existing gap every Sacrifice cost in the codebase shares. TODO comment added on both registrations.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 13 new tests in
`crates/engine/tests/phase5_script_ops_create_token.rs` (token on battlefield with `is_token: true`,
Clue ability registration, Food ability registration, 3-at-once count, creature token P/T/subtypes,
multi-subtype name join, name override, ArtifactCreature card types, token controller tracking,
Clue ability end-to-end draw, Food ability end-to-end life gain, creature token has no abilities,
dying token goes to graveyard TODO CR 111.7), all prior tests unaffected. Total: 0 failed.

**What got implemented:**
- `types/ability.rs`: `NamedToken { Clue, Food }` (Treasure TODO), `CreatureTokenSpec` struct
  (subtypes/colors/power/toughness/keywords/additional_types/additional_supertypes/name), `TokenSpec
  { Named(NamedToken), Creature(CreatureTokenSpec) }`. `AbilityEffect::CreateToken { spec, count }`.
  New imports: `use crate::types::card::{CardTypeSet, KeywordSet, SupertypeSet}` and
  `use crate::types::mana::{ColorSet, ManaUnit}`.
- `types/object.rs`: `pub is_token: bool` field added before `visibility` with a TODO for CR 111.7.
  All 45+ existing `GameObject {}` struct literal sites across the codebase got `is_token: false,`
  inserted (PowerShell line-by-line patching, not the broken `ScriptBlock -replace` approach from
  the first attempt — see "Mistakes caught" below).
- `engine.rs`: two new free functions before `impl Engine`:
  - `build_token_definition(spec: &TokenSpec) -> CardDefinition` — builds the card definition from
    scratch: `ManaCost { symbols: Vec::new() }` (not `Option<ManaCost>`), `PTValue::Fixed(i32)` for
    power/toughness, `ScriptReference::Inline(String::new())`.
  - `register_token_abilities(state, token_id, spec, controller)` — pushes a `RegisteredAbility` for
    Clue (`DrawCards(1)`) and Food (`GainLife(3)`); Creature arm is empty.
  - `AbilityEffect::CreateToken { spec, count }` arm in `Engine::advance` between `DrawCards` and
    `AddMana`: loops `count` times, calls `build_token_definition`, allocates a new `ObjectId` and
    `SequenceNo`, constructs a `GameObject` with `is_token: true`, inserts into `state.objects` and
    the battlefield `Zone`, calls `register_token_abilities`, pushes a `ZoneChange` event.
  - Imports updated to add `NamedToken`, `TokenSpec`, `CardTypeSet`, `KeywordSet`, `PTValue`,
    `SupertypeSet`, `AdditionalCost`, `FilterController`, `FilterSpec`.
- `commands.rs`: `Some(AbilityEffect::CreateToken { .. })` added to the `non_mana_effect` exhaustive
  match (same pattern as Sacrifice/Exile/Destroy before it).
- `script.rs`: four new helper functions (`parse_supertype_name`, `parse_color_string`,
  `decode_subtypes`, `decode_creature_token_opts`); `TokenTag(TokenSpec)` UserData wrapper;
  `ops.set("CreateToken", ...)` in `bind_game_globals`; a `tokens` table with `Clue`/`Food` value
  bindings and `Creature`/`ArtifactCreature` function bindings; `game.set("Tokens", tokens)`.

**STRUCT DEVIATIONS:**
- `GameObject.is_token: bool` — one new field, not anticipated before this slice. Requires touching
  every `GameObject {}` struct literal in the codebase (45+ sites).
- `NamedToken`/`CreatureTokenSpec`/`TokenSpec` — anticipated in the pre-slice requirements report.
- `AbilityEffect::CreateToken` — anticipated; required updating `commands.rs`'s exhaustive match.

**INTERFACE DEVIATIONS:** None beyond what the requirements report scoped.

**SCOPE DEVIATIONS:**
- 45+ `GameObject {}` struct literal sites across 20+ files required `is_token: false,` — larger
  footprint than any prior slice's mechanical struct-literal update (the previous high was ~10 sites
  for `StackObject::effect`). All done via PowerShell line-by-line string replacement rather than
  the `ScriptBlock` approach that burned the first attempt (see "Mistakes caught" below).

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- **CR 111.7 cessation** — tokens should cease to exist when they leave the battlefield (going to
  the graveyard, being exiled, etc.) rather than persisting as cards. `is_token` exists as the hook
  for this future SBA or zone-change handler.
- `Game.Tokens.Treasure` — blocked on Lua coroutines (backlog item 9), since Treasure's {T},
  Sacrifice → add one mana of any color needs player input at resolution time.
- Self-registration for token abilities — Clue/Food abilities are registered via the direct
  `register_token_abilities` Rust path, not through card-script self-registration. This is by design
  (tokens have empty scripts and no `initial_effect` entry point).
- FilterSpec identity gap on Clue/Food sacrifice costs — `FilterSpec::controller: You, card_types:
  Artifact` matches any artifact, not specifically this Clue/Food. Shared pre-existing gap.

## Phase 5: phase5/continuous-effects-script slice (backlog item 3a)

Branch `phase5/continuous-effects-script`, off master. Slice backlog item 3a, split out of the
layer-engine-cda slice as its own follow-up: `AffectedFilter::Script`/`EffectPayload::Script` (a
`ContinuousEffect`'s affects-predicate and actual modification, backed by a script instead of a
`Primitive`/tag) had been dead code since the layer-engine-basics slice first introduced the
`Primitive`/`Script` split. Scope, confirmed before writing code: real evaluation for both, plus
the `Game.CreateStaticAbility` builder sugar (`:SetAffected`/`:SetEffect`) to construct them from a
card script — narrowed to the same "no `Game.*` state-query bindings exist yet" limitation
`invoke_cda` already carries (checked again before starting: still true), so every test script
below decides purely from `object`/`source`/`controller` identity or a constant, never real board
state.

**Design, confirmed before writing code:**
- `ContinuousEffect::applies_to` (`types/effect.rs`) stays `Primitive`-only — no `ScriptRuntime`
  import added to the types module. `AffectedFilter::Script` is evaluated in
  `layers::applicable_effects_in_layer` instead (which already has `&ScriptRuntime` in scope via
  `effective_characteristics`), same separation `resolve_base_pt` already keeps for CDA.
- New `ScriptRuntime::invoke_continuous_affects(&self, handle, object, source, controller) ->
  Result<bool, ScriptError>` — `function(object, source, controller) -> bool`. New
  `ScriptRuntime::invoke_continuous_effect(&self, handle, source, controller) ->
  Result<Option<EffectPrimitive>, ScriptError>` — `function(source, controller) -> table`, same
  argument shape `invoke_cda`/`invoke_operation` already use. Decodes one of six recognized keys
  (`set_pt`/`modify_pt`/`add_keyword`/`add_card_type`/`add_color`/`switch_pt`) in a fixed order —
  the same "Lua asks, Rust decides" one-key-per-shape convention `invoke_operation`'s `gain_life`
  established, generalized from one key to six. An empty/unrecognized table or a script error both
  resolve to `None` — "skip rather than guess," same stance every other unresolvable value in this
  engine already takes.
- `layers.rs`'s six per-layer loops in `effective_characteristics` were refactored to call a new
  `resolve_primitive(effect, script_runtime, controller) -> Option<EffectPrimitive>` helper instead
  of matching `EffectPayload::Primitive(..)` directly — `Primitive(p) => Some(*p)`,
  `Script(handle) => invoke_continuous_effect(...).ok().flatten()`. A decoded shape that doesn't
  match what a given loop expects (e.g. an `AbilityAdding`-declared effect whose script never
  returns `add_keyword`) is silently skipped by the existing `if let Some(EffectPrimitive::X(..))`
  pattern in each loop — no new mismatch-handling code needed, it falls out for free.
- `Game.CreateStaticAbility`'s `:SetEffect`/`:SetAffected` gained the same two-shapes pattern
  `:SetCondition`/`:SetOperation` already use: a `Game.Effects.*` tag / plain filter table decodes
  to `Primitive` with zero further evaluation, a custom function is registered and returned as
  `Script`. `StaticAbilityBuilder.effect` changed type from `Option<EffectPrimitive>` to
  `Option<EffectPayload>`; `.affected` changed from `EffectFilter` to `AffectedFilter`.
  `validate_layer_effect_pairing` now takes `&EffectPayload` and only checks the `Primitive` case —
  a `Script` payload's real shape isn't known until invoked, so `:Register()` accepts any
  layer/`Script` pairing; a mismatch surfaces (silently, per the point above) at query time instead
  of eagerly at registration.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 12 new tests in
`crates/engine/tests/phase5_continuous_effects_script.rs` (unit-level: an affects-predicate matches
only the identical object and errors resolve to `Err` not a panic; effect decode covers
`modify_pt`/`add_keyword`/`switch_pt`, an empty table resolves to `None`, an erroring script
resolves to `Err` not a panic; builder-level: `Game.CreateStaticAbility` registers cleanly with a
function-form `:SetAffected` and, separately, a function-form `:SetEffect`; real-consumer:
`effective_characteristics` actually applies a script-backed affects-predicate to the matching
object only, actually applies a script-decoded `add_keyword`, and silently skips a script effect
whose decoded shape doesn't match its declared layer rather than applying garbage), all 410 prior
tests unaffected — full workspace run: 422 passed, 0 failed. Confirmed via the real CLI too:
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_continuous_effects_script.toml`
— 20/20 steps ran/passed (an anthem land whose static ability's `:SetEffect` is a script decoding
`{modify_pt = {power=1,toughness=1}}`, not a `Game.Effects.ModifyPT` tag, still buffs a vanilla
bear's combat damage from 2 to 3, same real-consumer proof
`phase5_static_ability_self_registration.toml` established for the tag-only form).

**What got implemented:**
- `layers.rs`: `applicable_effects_in_layer` gained a `&ScriptRuntime` parameter and now branches on
  `AffectedFilter::Primitive`/`Script` (the latter via the new `invoke_continuous_affects`, `Err`
  treated as "doesn't match"). New `resolve_primitive` helper, called from all six per-layer loops
  in `effective_characteristics` in place of the old direct `EffectPayload::Primitive` match.
- `script.rs`: `ScriptRuntime::invoke_continuous_affects`/`invoke_continuous_effect` (new methods,
  next to `invoke_cda`). `decode_effect_value`/`decode_affected_value` (renamed from
  `decode_affected_table`) both gained the function-fallback branch and now take `&Lua`/`&mut
  ScriptRegistry` to register a handle, mirroring `decode_condition_value`/`decode_operation_value`
  exactly. `validate_layer_effect_pairing` signature changed to `&EffectPayload`, early-returns `Ok`
  for `Script`. `StaticAbilityBuilder` field types updated; its `:SetEffect`/`:SetAffected` methods
  route through the registry the same way `TriggeredAbilityBuilder`'s setters already do.
- `docs/lua_scripts.md`: new "Script-backed `:SetAffected`/`:SetEffect`" subsection under
  "Self-registering a static ability," with a worked example and the same hidden-information caveat
  every other script-bridge doc section already carries. Removed the stale "Not available yet"
  bullet for `AffectedFilter::Script`/`EffectPayload::Script`.
- `crates/engine/tests/scenarios/phase5_continuous_effects_script.toml`: same anthem-land/vanilla-
  bear/combat-damage fixture `phase5_static_ability_self_registration.toml` uses, except the
  anthem's `:SetEffect` is a script decoding `modify_pt` instead of a `Game.Effects.ModifyPT` tag.

**STRUCT DEVIATIONS:**
- `StaticAbilityBuilder.effect: Option<EffectPrimitive>` → `Option<EffectPayload>`,
  `.affected: EffectFilter` → `AffectedFilter` — both anticipated by the pre-slice requirements
  report exactly (needed to hold either shape), not a deviation from it.

**INTERFACE DEVIATIONS:** None beyond what the pre-slice report's "Design" section already named —
`applicable_effects_in_layer` gaining a `&ScriptRuntime` parameter, `decode_effect_value`/
`decode_affected_value` gaining `&Lua`/`&mut ScriptRegistry` parameters, and
`validate_layer_effect_pairing`'s signature change were all anticipated, not discovered mid-slice.

**SCOPE DEVIATIONS:**
- Found (not created) a pre-existing documentation gap while updating the backlog list at the top
  of this file: backlog item 6a (Phyrexian/snow/hybrid mana payment) and 6b (`{X}` costs) both have
  real code and tests but never got a session write-up appended here, unlike every other completed
  sub-item. Flagged in the backlog list itself rather than silently fixed by inventing a plausible-
  sounding write-up after the fact — the actual pre-slice design conversations for 6a/6b aren't
  recoverable from this session.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- No `Game.*` state-query bindings — a predicate/effect-shape script can only reason from
  `object`/`source`/`controller` identity or a constant, not real board state ("for each Zombie you
  control"-style effects stay unbuildable). Backlog item 8's hidden-information-safe `ScriptContext`
  is the real unblock, untouched here.
- A `Script` effect's layer/decoded-shape mismatch is silently skipped at query time, not surfaced
  as an error the way a `Primitive` mismatch is caught eagerly at `:Register()` — a real, narrower
  safety net, flagged not fixed.
- `EffectDuration` stays `Permanent` for any script-registered static ability (pre-existing gap,
  unaffected). No CR 606 "as long as" conditional gating beyond the per-object affects-predicate;
  no CR 613.6-8 dependency system; layers 1/2/3/7d and the overwrite ("loses all types/colors")
  shape stay unbuilt — all pre-existing, untouched by this slice.

## Phase 5: phase5/sba-player-loss slice (backlog item 10, player-loss triad)

Branch `phase5/sba-player-loss`, off master. Slice backlog item 10, narrowed before writing code to
the three genuine "a player loses the game" state-based actions — 704.5b (drew from an empty
library), 704.5c (10+ poison counters), CR 903.10a (21+ combat damage from a single commander
source) — all symmetric extensions of the already-real 704.5a. Legend rule (704.5j)/world rule
(704.5k) split out as their own named follow-up (10a) rather than bundled in: both destroy
permanents via a player *choice* among duplicates, a structurally different SBA shape from "this
player loses," needing its own choice-pause design.

**Design, confirmed before writing code:**
- New `PlayerState.drew_from_empty_library: bool` (default `false`) — set by `Engine::draw_one_card`
  when the library zone is missing or empty, instead of the prior silent no-op. Sticky, never
  cleared, same permanence 704.5a's life-based elimination already has. Required a mechanical sweep
  of every `PlayerState { .. }` struct literal (92 sites across 48 files) — same category as the
  CreateToken slice's `is_token` sweep, done via the same "PowerShell regex over the whole tree, not
  `-replace` with a ScriptBlock" method that slice's own "Mistakes caught" note documented.
- `PlayerState::commander_damage_from` — was `todo!()` despite being named "already exposed for
  this" in the old backlog text; given a real one-line lookup body.
- `sba::apply_player_loss`'s filter extended with three more `||` arms: `drew_from_empty_library`,
  `counters.count(&CounterKind::Poison) >= 10`, and `commander_damage_taken.iter().any(|&(_, d)| d
  >= 21)` — the last one checked directly against the stored per-source totals rather than via
  `commander_damage_from` (which needs a specific source id the SBA loop doesn't have one of;
  `commander_damage_from` is a general-purpose accessor, made real for its own sake). One
  `GameEventKind::PlayerEliminated` regardless of which cause fired, unchanged from before.
- Checked before wiring the commander-damage check: nothing in this engine increments
  `commander_damage_taken` anywhere (grepped — always initialized empty). The check is real and
  testable via a hand-built fixture, but currently unreachable through actual gameplay — there's no
  "is this object a player's commander" concept at all yet (backlog item 11's Commander-format
  machinery, not this slice's).
- Checked before wiring 704.5c: poison counters *are* already reachable through real gameplay —
  `commands.rs`'s damage-application already routes Infect damage to a player into
  `player.counters.add(CounterKind::Poison, ...)` (from the script-ops-damage slice), so the
  real-consumer test drives actual combat damage, not just a hand-seeded counter total.

**Regression found and fixed while verifying, not silently worked around:** 5 solo (1-player)
scenario TOML fixtures (`phase5_script_bridge_primitives`, `phase5_script_bridge_trigger`,
`phase5_script_self_registration`, `phase5_targeting_basics`, plus `phase5_cast_spell_payment` which
had no ledger-length assertion to break) never seeded a library. CR 103.8a's turn-1-draw-skip only
applies in a two-player game (`engine.rs`'s `advance_turn_step` checks `turn_order.len() == 2`), so
each of these solo fixtures' turn-1 Draw step genuinely runs — and with the new 704.5b check real,
a real draw attempt against a nonexistent library now (correctly) sets the elimination flag, which
changed the ledger length by one extra event once eliminated. This isn't a bug in the new SBA check
— it's the new check correctly surfacing a previously-latent gap in five fixtures that assumed the
empty-library draw would silently do nothing forever, which was true before this slice but was
never the intended CR behavior. Fixed each fixture the same way `phase5_beginning_phase.toml`
already seeds a library card, and adjusted each fixture's `ledger_len` assertion by +1 (confirmed
against the real CLI's actual output per fixture, not guessed).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 9 new tests in
`crates/engine/tests/phase5_sba_basics.rs` (empty-library draw sets the flag without eliminating
that same pass, same split 704.5a's life check already has; the flag eliminates on the next SBA
pass; a real draw with a card present never sets the flag; 9 poison untouched, 10 eliminates; a
real-consumer test dealing 10 Infect combat damage in one unblocked hit through the actual
`AssignCombatDamage` pipeline confirms 704.5c fires off real gameplay, not just a hand-seeded
counter; `commander_damage_from` unit test; 20 commander damage untouched, 21 eliminates), all 422
prior tests unaffected — full workspace run: 431 passed, 0 failed. Confirmed via the real CLI too:
re-ran `phase5_targeting_basics.toml` (one of the five regressed fixtures) after the fix —
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_targeting_basics.toml` — all
steps ran, all assertions passed including the corrected `ledger_len == 17`.

**What got implemented:**
- `types/player.rs`: `PlayerState.drew_from_empty_library: bool` field (with doc comment).
  `commander_damage_from`'s real body.
- `engine.rs`: `draw_one_card` restructured around a `let Some(card) = ... else { ... }` — the
  `else` branch sets `drew_from_empty_library` before returning `None`, replacing the old two
  early-return `?`s that couldn't attach a side effect. `draw_step`'s and `draw_one_card`'s own doc
  comments updated to describe the real CR 704.5b split instead of "not modeled yet."
- `sba.rs`: `apply_player_loss`'s filter extended (three new `||` arms, doc comment rewritten to
  describe all four causes plus the split-out legend-rule note). New `use
  crate::types::counter::CounterKind` import. `check_state_based_actions`'s own module doc comment
  and trailing TODO block updated to match.
- `crates/engine/tests/phase5_sba_basics.rs`: 9 new tests (see VERIFICATION RESULT), plus a new
  `drain`/`pass` helper pair (mirrors every other multi-turn-step Rust test file's own convention)
  used by the two new turn-flow tests.
- 5 scenario TOML fixtures fixed (see "Regression found" above): each gained a filler library card
  + setup entry, and (where applicable) a `ledger_len` assertion bumped by 1.

**STRUCT DEVIATIONS:** `PlayerState.drew_from_empty_library` — anticipated in the pre-slice
requirements report exactly (the mechanical-sweep scope was named upfront, not discovered after the
fact).

**INTERFACE DEVIATIONS:** None — `draw_one_card`'s signature is unchanged, `apply_player_loss`'s
signature is unchanged; only the bodies grew.

**SCOPE DEVIATIONS:**
- The 5 scenario-fixture fixes above were not in the pre-slice requirements report (the report
  anticipated the mechanical `PlayerState` sweep, not a behavior change to existing fixtures) —
  surfaced while verifying, fixed as part of this slice rather than left as a newly-broken test
  suite, per the "regression found and fixed" note above.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Legend rule (704.5j)/world rule (704.5k) — split into their own named follow-up (10a in the
  backlog list), needs a real choice-pause design.
- Commander-damage *production* — nothing increments `commander_damage_taken`; the check is real
  but inert until backlog item 11's Commander-format machinery (identifying a commander, routing
  its combat damage) exists.
- `StepStatus::GameOver` is declared but never constructed anywhere, and nothing checks
  `.eliminated` to skip an eliminated player's turn/priority — checked before starting this slice:
  this gap already existed for 704.5a (merged, untouched) and is shared by all four causes now, not
  introduced or expanded by this slice. A player can be `eliminated` and the game mechanically keeps
  going exactly as before.
- `submit_discard`'s CR 514.1 hand-size discard and `draw_step`'s library-side retirement still
  discard departed Triggered abilities outright (pre-existing gap, unrelated, untouched).

## Phase 5: phase5/legend-world-rule slice (backlog item 10a)

Branch `phase5/legend-world-rule`, off master. Slice backlog item 10a, split out of the
sba-player-loss slice: CR 704.5j (legend rule) and CR 704.5k (world rule) — both destroy permanents
via a mechanism structurally different from "this player loses" (704.5a-c/903.10a), so they got their
own follow-up rather than being forced into `apply_player_loss`'s shape.

**Design, confirmed before writing code:**
- **Legend rule reuses `sba::resolve_deaths` for the actual destruction.** The only genuinely new
  piece is the player's choice of which duplicate to keep — once made, the losers are just another
  `pending_deaths: VecDeque<ObjectId>` fed into the exact same replacement-checked, CR-400.7-new-id,
  `commit_death`-based path every other SBA death already uses. No bespoke destruction logic exists
  anywhere in this slice.
- New `SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far }` — `sba.rs`'s new
  `find_legend_rule_group` groups battlefield objects by `(controller, effective name)` where
  `effective.supertypes` contains `Legendary` (queried via `layers::effective_characteristics`, same
  "always query through the layer engine" convention `collect_dying_creatures` already established —
  future-proofs against a not-yet-built "becomes legendary"/name-changing effect for free). Not
  creature-specific — legend rule applies to any permanent type. Only the *first* violating group
  pauses; `check_state_based_actions` re-invokes fresh after each choice resolves, so multiple
  simultaneous violations resolve one at a time across passes, same as CR 616.1's own ordering-choice
  pause already does. Candidates sorted by `ObjectId` for determinism (`state.objects` is a
  `HashMap`; iteration order isn't guaranteed) — which required adding `PartialOrd`/`Ord` to
  `PlayerId`/`ObjectId` (purely a deterministic sort key, no rules meaning).
- New `SbaOutcome::NeedsLegendRuleChoice` wired at both existing `SbaOutcome::NeedsOrderingChoice`
  call sites (`ProcessState::SbaLoop` and `CleanupSbaCheck` in `engine.rs`) via a new parallel
  `ProcessState::AwaitingLegendRuleChoice { player, candidates, then: SbaResumeThen }` and
  `StepStatus::AwaitingLegendRuleChoice` — `then` reuses the existing `SbaResumeThen` enum
  `ReplacementResumeTarget::SbaDeath` already has, since both pause shapes resume into the same two
  possible callers. New `Engine::submit_legend_rule_choice(kept: ObjectId)` validates the candidate,
  then calls `self.resume_sba_death(losers, Vec::new(), Vec::new(), then)` — the exact same helper
  `submit_replacement_choice` already uses — so a replacement effect on a losing legendary correctly
  re-pauses on the existing `AwaitingReplacementChoice`, not silently skipped. New
  `LegendRuleChoiceError { NotAwaitingChoice, InvalidCandidate }`, mirroring `ReplacementChoiceError`
  exactly.
- **World rule is fully deterministic** (CR 704.5k resolves purely on "shortest time as World" —
  no player choice at all) — new `collect_world_rule_losers` (independent of
  `collect_dying_creatures`'s creature-only filter, since World permanents aren't necessarily
  creatures) returns the losing ids directly; `check_state_based_actions` merges them into the same
  `dying` vec already fed to `resolve_deaths`. "Shortest time as World" = highest `timestamp` (since
  that value only ever increases as the game proceeds, the most-recently-arrived object has had the
  supertype the least time) — the one exception, everyone else dies. A tie for the maximum timestamp
  kills *all* World permanents, not just the tied ones, per CR 704.5k's own explicit tie clause. A
  duplicate id from a World creature also independently dying to lethal damage is harmless —
  `resolve_deaths`'s existing "already moved, skip" guard handles it for free, no dedup needed.
- Checked before writing code: no TOML/CLI scenario fixture exists for the CR 616.1 replacement
  ordering-choice pause either (`phase5_sba_death_replacement.rs` has no companion `.toml`) — the
  scenario crate's step vocabulary has never needed a "submit a mid-pipeline choice" step type.
  Consistent with that precedent, this slice's legend-rule pause/resume is verified via `Engine`
  tests only, not a new TOML step type (would be new scope, not needed for correctness).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 10 new tests in
`crates/engine/tests/phase5_legend_world_rule.rs` (two same-name legendaries under one controller
pause with both candidates offered; three same-name legendaries offer all three, not just one other,
per CR 704.5j's "the rest" wording; different controllers with the same name aren't grouped;
`submit_legend_rule_choice` rejects an invalid candidate and rejects when not awaiting; a real
end-to-end `Engine` pipeline test where choosing one destroys the other through the actual
replacement-checked pipeline; a "would die, exile instead" replacement effect on the losing
legendary correctly exiles it instead of touching the graveyard; world rule destroys all but the
most-recently-arrived permanent automatically, a tie destroys all of them, and a single World
permanent is untouched), all 431 prior tests unaffected — full workspace run: 441 passed, 0 failed.

**What got implemented:**
- `types/ids.rs`: `PartialOrd`/`Ord` added to `PlayerId`/`ObjectId` (doc comments note these exist
  purely as a deterministic sort/map key, not a rules concept).
- `sba.rs`: `SbaOutcome::NeedsLegendRuleChoice` variant. New `find_legend_rule_group`/
  `collect_world_rule_losers` functions. `check_state_based_actions` checks legend rule (pausing on
  the first violation) before creature deaths, and folds world-rule losers into the existing dying
  list. Module doc comment and trailing TODO block updated to match.
- `engine.rs`: `LegendRuleChoiceError`, `StepStatus::AwaitingLegendRuleChoice`,
  `ProcessState::AwaitingLegendRuleChoice`, the "nothing to do until answered" `step()` arm for it,
  and `Engine::submit_legend_rule_choice`. Both `SbaLoop`/`CleanupSbaCheck` arms gained a
  `SbaOutcome::NeedsLegendRuleChoice` match case.
- `crates/scenario/src/lib.rs`: `drain`'s exhaustive `StepStatus` match gained
  `AwaitingLegendRuleChoice` alongside the other pause variants (same "stop draining, this scenario
  run reached a pause" treatment every other pause status already gets).
- 4 existing test files' own exhaustive `SbaOutcome` matches (`phase5_sba_basics.rs`,
  `phase5_layer_engine_basics.rs`, `phase5_layer_engine_types_colors.rs`,
  `phase5_layer_engine_cda.rs`) gained a `NeedsLegendRuleChoice` arm (a panic, same as their existing
  `NeedsOrderingChoice` arm — none of those slices' fixtures expect either pause).

**STRUCT DEVIATIONS:**
- `PlayerId`/`ObjectId` gaining `PartialOrd`/`Ord` — anticipated in the pre-slice requirements report
  (needed for deterministic grouping/sorting over a `HashMap`'s unordered iteration), not discovered
  mid-implementation.

**INTERFACE DEVIATIONS:** None beyond what the pre-slice report's "Design" section already
named — the new `ProcessState`/`StepStatus` variants and `submit_legend_rule_choice`'s signature
were all anticipated by mirroring `AwaitingReplacementChoice`/`submit_replacement_choice` exactly.

**SCOPE DEVIATIONS:**
- 4 existing test files' `SbaOutcome` matches needed a new arm (mechanical, anticipated as expected
  churn — any exhaustive match over `SbaOutcome` needed updating once a new variant was added, same
  as every other enum-growing slice in this codebase).

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- CR 801.12's "range of influence" restriction on the world rule — not modeled; applied globally
  across all players, correct for this engine's 2-player default scope, would need revisiting if
  true multiplayer (backlog item 11) ever lands.
- A legendary permanent becoming legendary via a continuous effect, or a name-changing effect (layer
  3) — neither primitive exists anywhere in this engine yet; the query is future-proofed for the
  former (via `layers::effective_characteristics`) but nothing can construct either case today.
- No TOML/CLI scenario fixture — consistent with the pre-existing precedent CR 616.1's own
  pause-shaped slice already set (see "Design" above), not a gap unique to this slice.

## Phase 5: phase5/payment-unwind slice (backlog item 15)

Branch `phase5/payment-unwind`, off master. Slice backlog item 15. Scope: `Engine::submit_payment`
rejecting an invalid `PaymentPlan` already left `AwaitingPayment` untouched so the caller could retry
— that part was already correct. The real gap: nothing let a caller declare "I cannot or will not
pay" and get out. Since the card/ability already moved onto the stack before `AwaitingPayment` is
ever reached (CR 601.2a happens before cost payment, confirmed in `commands.rs`'s own comment), a
cost with no valid plan at all left the engine stuck in `AwaitingPayment` forever.

**Design, confirmed before writing code:**
- New `Engine::unwind_payment(&mut self) -> Result<(), PaymentError>` (CR 601.6: "the game returns to
  the moment before the casting of that spell was proposed"). Reuses `PaymentError::NotAwaitingPayment`
  as its only failure mode — no new error type needed.
- **Reuses `move_resolving_spell` directly** — the exact same Stack-departure helper CR 608.2b's
  fizzle path already uses (`ResolveStackTop`'s fizzle/normal-resolution branches). Calling
  `self.move_resolving_spell(stack_id, ZoneKind::Hand, false)` handles the entire card-side move (new
  CR 400.7 id, `commands::remap_abilities` — not `retire_object`, matching `CastSpell`'s own
  "same card, new id" convention for its Hand→Stack move rather than a "gone for good" death) with
  zero duplicated logic. Checked before writing this: `move_resolving_spell` doesn't itself remove
  the `StackObject` from `state.stack` (its caller does, same as `ResolveStackTop`'s own
  `state.stack.pop()` at the top of that arm) — `unwind_payment` does this first, by `stack_id`
  rather than assuming top-of-stack.
- `StackEntryKind::ActivatedAbility` (the only other case reachable via `AwaitingPayment` today —
  `TriggeredAbility` never carries a cost per `RegisteredAbility.cost`'s own contract, `Copy` isn't
  modeled anywhere): just removes the `StackObject`, no zone move at all — the source permanent
  never left the battlefield to begin with.
- New `GameEventKind::CastingUnwound { stack_id }` — checked before writing code: this engine's
  ledger is the single source of truth for replay/scrubbing, so the `ActivatedAbility` case (no
  accompanying `ZoneChange`) would otherwise be invisible to any replay viewer. Mirrors `Fizzled`'s
  own precedent exactly ("emitted instead of/alongside a `ZoneChange` depending on what's leaving the
  stack") — paired with the `ZoneChange` for a spell, alone for an ability.
- Resumes via the existing `Engine::end_pipeline_run()` (clears `pipeline_context`/
  `departed_triggered_abilities`, returns whatever `pipeline_exit` says) rather than hardcoding
  `ProcessState::PriorityWindow` — same hazard that field's own doc comment already flags for any
  pipeline-exit path.
- **`crates/scenario/src/lib.rs` gained a matching `ActionSpec::UnwindPayment { player }` TOML/CLI
  step** (`run_unwind_payment`, mirrors `run_payment` exactly) — checked before writing code:
  `SubmitPayment` already has full TOML/CLI support (unlike the legend-rule slice's choice-pause,
  which had no such precedent to extend), so adding the companion step was natural, not new scope.

**Explicitly not modeled (named, not silently guessed):**
- True CR 733 state rewind (full history erasure) — this engine's ledger is append-only, no undo
  primitive exists anywhere. `unwind_payment` is an honest forward zone-change/removal instead.
- No automatic "is any valid plan even possible" pre-check before a cast reaches `AwaitingPayment` —
  a much harder, near-solver-shaped problem for arbitrary cost combinations. The caller decides when
  to give up and calls `unwind_payment()` explicitly.
- Mana already added via mana abilities activated during the payment window (CR 601.2g) is
  untouched — matches CR 601.2h/601.6's own stance, and moot today since mana abilities apply
  directly to the pool outside `PaymentPlan` regardless.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 4 new tests in
`crates/engine/tests/phase5_payment_unwind.rs` (unwinding a cast spell returns the card to hand
under a new id with the stack cleared, ledger showing `ZoneChange` then `CastingUnwound`; priority
reopens for the same player afterward, and a rejected `submit_payment` plan beforehand doesn't
interfere with the later unwind; unwinding an activated ability removes only the stack entry — the
source permanent stays exactly where it was, no `ZoneChange` at all; `unwind_payment` rejects when
not awaiting payment), all 441 prior tests unaffected — full workspace run: 445 passed, 0 failed.
Confirmed via the real CLI too: new
`crates/engine/tests/scenarios/phase5_payment_unwind.toml` —
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_payment_unwind.toml` — 11/11
steps ran/passed (a spell with an unaffordable `{1}` cost against an empty pool reaches the stack,
`unwind_payment` returns it to hand, `ledger_len == 15` confirmed against the CLI's actual output,
not guessed).

**What got implemented:**
- `types/event.rs`: `GameEventKind::CastingUnwound { stack_id }`.
- `engine.rs`: `Engine::unwind_payment`, placed directly after `submit_payment`.
- `crates/scenario/src/lib.rs`: `ActionSpec::UnwindPayment { player }`, `run_unwind_payment`, wired
  into `run_action_spec`'s dispatch, `describe()`'s one-liner, and the `unreachable!()` guard in
  `to_player_action` (same three touch-points `SubmitPayment` itself already has).
- `crates/engine/tests/scenarios/phase5_payment_unwind.toml`: new fixture, cast-then-unwind.

**STRUCT DEVIATIONS:** None — `GameEventKind::CastingUnwound` and `ActionSpec::UnwindPayment` were
both anticipated exactly in the pre-slice requirements report.

**INTERFACE DEVIATIONS:** None — `unwind_payment`'s signature (mirroring `submit_replacement_choice`/
`submit_payment`'s own shape) and its reuse of `move_resolving_spell`/`end_pipeline_run` were both
anticipated by name in the report's "Design" section before implementation started.

**SCOPE DEVIATIONS:**
- Adding the TOML/CLI `unwind_payment` step wasn't explicitly named in the original pre-slice report
  (which focused on the `Engine`-level fix) — confirmed and added once `SubmitPayment`'s own existing
  TOML support was checked, per the "Design" note above, rather than left as a gap the report didn't
  anticipate.

**UNRESOLVED:** none new — see "Explicitly not modeled" above, all named and scoped before writing
code.

## Phase 5: phase5/script-ops-shuffle slice (backlog item 14)

Branch `phase5/script-ops-shuffle`, off master. Slice backlog item 14. **Correction to the backlog
first:** `Zone::new`/`push_top`/`top` (the item's literal claim) are not `todo!()` — verified real by
reading `zone.rs` directly, same class of stale-doc gap the 6a/6b cost-completeness items turned out
to have. What was actually missing: a real, generic `Engine::shuffle_library(player)` already existed
(Fisher-Yates over `state.rng`, logs `LibraryShuffled`/`RngDrawn`) — but it was private, called only
from pregame setup. Nothing let a card/script trigger it mid-game.

Scry/surveil/search/tutor were bundled into this same backlog item originally — too coarse. Each
needs a genuine new player-choice pause (`PlayerActionKind::MakeChoice` is a pure stub) or a hidden-
info exception this engine doesn't have yet — the same magnitude of work the legend-rule slice was,
not a "zone/library helper" gap. Split off as 14a (scry/surveil) and 14b (search/tutor), named but not
implemented this slice.

**What got implemented:**
- `types/ability.rs`: `AbilityEffect::Shuffle` — no target, always the resolving `StackObject`'s
  controller's own library (CR 701.19b), same "controller only" shape `DrawCards`/`GainLife` already
  established, not `Damage`/`Destroy`'s object-target shape.
- `engine.rs`'s `ResolveStackTop`: a new `Some(AbilityEffect::Shuffle)` arm calling
  `self.shuffle_library(top.controller)` directly — that helper already mutates state and logs its
  own real events, so no `CommitEvents` special-case needed, same convention `DrawCards` established
  for `draw_one_card`.
- `commands.rs`'s `ActivateAbility` non-mana-effect match gained `Some(AbilityEffect::Shuffle)`
  alongside the other primitives.
- `Game.Ops.Shuffle()` binding (script.rs) — a toggle value, not a function, same pattern
  `Destroy`/`Sacrifice`/`Exile` already use (no arguments).

**Explicitly not modeled (named, not silently guessed):**
- `Target::Player` ("target player shuffles their library") — controller-only this slice, same
  precedent `DrawCards` itself set for its own target gap.
- Scry/Surveil (14a) and Search/tutor (14b) — split off as their own follow-ups per the correction
  above.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 4 new tests in
`crates/engine/tests/phase5_script_ops_shuffle.rs` (a cast spell with `Shuffle` reorders the library
deterministically under the fixed test seed — same seed run twice produces identical results — and
logs `LibraryShuffled`; an activated ability with `Shuffle` works identically; shuffling an empty or
single-card library is a harmless no-op, zero `RngDrawn` events since there's nothing to swap; a
subsequent `DrawCards` spell actually draws whatever the shuffle put on top, not just "an event
fired"), all 445 prior tests unaffected — full workspace run: 449 passed, 0 failed.

**STRUCT DEVIATIONS:** None — `AbilityEffect::Shuffle` was anticipated exactly in the pre-slice
requirements report.

**INTERFACE DEVIATIONS:** None — reusing the existing (now-called-from-a-new-site)
`Engine::shuffle_library` was the report's stated design.

**SCOPE DEVIATIONS:** None — scry/surveil/search/tutor were named as deferred follow-ups (14a/14b) in
the pre-slice report, not silently dropped.

**UNRESOLVED:** none new — see "Explicitly not modeled" above.

## Fix: wire Cleanup's own SBA pass into real trigger collection (backlog item 7a, cleanup half)

Committed directly to `master` (no branch) at the human's request — a small, bounded fix picked off
backlog item 7's own "Still open" list, not a fresh vertical slice. `ProcessState::CleanupSbaCheck`
decided priority-vs-advance (CR 514.3) straight off its own SBA pass's event count, bypassing
`CollectTriggers`/`PutTriggersApnap` entirely — a CR 603.10 dies-trigger (or any other trigger) raised
by a creature dying during cleanup's own SBA check was silently dropped, never placed onto the stack.
Flagged as an explicit gap since the `upkeep-triggers` slice (7a's first half, `AdvanceTurnStep`'s own
fix) — this closes the second, differently-shaped half that slice's own doc comment named.

**What got implemented:**
- `engine_context.rs`: new `PipelineExit::CleanupCheck { since_ledger: usize }` — unlike the other
  three variants (a fixed destination), the actual next `ProcessState` depends on whether the ledger
  grew since `since_ledger` (an SBA event committed, or a trigger got pushed onto the stack): grown
  means CR 514.3 opens a priority window, unchanged means the turn advances silently. Decided in
  `Engine::end_pipeline_run`'s own new match arm.
- `engine.rs`: `Engine::finish_cleanup_sba_check` reworked to stash `departed` triggered abilities into
  `departed_triggered_abilities` and route through `ProcessState::CollectTriggers` (setting
  `pipeline_exit` to the new `CleanupCheck` variant) instead of deciding priority-vs-advance inline —
  shared between a fresh `CleanupSbaCheck` pass and one resumed after a paused CR 616.1/704.5j choice
  (`resume_sba_death`'s `SbaResumeThen::CleanupSbaCheck` arm), same "one shared transition function"
  convention `finish_sba_loop` already established for `SbaLoop`. `SbaResumeThen::CleanupSbaCheck` now
  carries `since_ledger` through the pause so a resumed pass reaches the identical fix.

**Explicitly not modeled (named, not silently guessed):**
- CR 514.3's "another cleanup step begins after this priority window and its stack empties" —
  `advance_turn_step`'s `(Ending, Cleanup)` arm still just rotates the turn instead of re-running
  discard/effect-expiry; a pre-existing TODO, unaffected by this fix (still needs its own case to
  actually reach it before it matters).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 2 new tests in
`crates/engine/tests/phase5_cleanup_trigger_collection.rs`: a creature that drops to 0-or-less
effective toughness (CR 704.5f) the instant its until-end-of-turn pump expires at cleanup (not a
damage-based scenario — CR 514.2 wipes marked damage in that same step, so a lethal-*damage* setup
can't survive to reach `CleanupSbaCheck`'s own SBA pass) now fires and resolves its Dies trigger for
real, correctly reopening priority instead of the turn silently advancing (confirmed CR 117.5 also
holds: priority reopens once more with the stack empty before the turn is finally allowed to advance);
a healthy creature with nothing to trigger still advances silently as before. All prior tests
unaffected — full workspace run: 451 passed, 0 failed.

**UNRESOLVED:** none new.

## Fix: add CR 603.2h once-per-turn trigger cap (backlog item 7)

Also committed directly to `master`, same session, second of "a couple commits" picked off backlog
item 7's own list. Scope: just CR 603.2h ("This ability triggers only once each turn") — CR 603.4's
intervening-if clause is a genuinely different mechanism (a state predicate re-checked at trigger time
*and* again at resolution, not a trigger-count cap) and stays a named, separate follow-up.

**Design, confirmed before writing code:**
- New `ConditionSpec::PrimitiveOncePerTurn(ConditionPrimitive)` variant rather than a new field on
  `RegisteredAbility` — checked first: `RegisteredAbility` literals exist at 63 call sites across 25
  files, `ConditionSpec` variants are matched at only 2 (both in `triggers.rs`) with zero existing
  construction sites forced to change (adding an enum variant doesn't touch code that doesn't use it,
  unlike a new mandatory struct field). Matching itself is identical to `Primitive`'s own — the cap is
  a separate check layered on top.
- New `GameState::once_per_turn_triggered: HashSet<(ObjectId, u32)>` (keyed the same way
  `PendingTrigger`/`commands::find_ability` already key ability instances) — reset alongside
  `lands_played_this_turn` at the same turn-rotation point (`advance_turn_step`'s `(Ending, Cleanup)`
  arm). Touches every existing `GameState` struct literal (52 files) — same kind of mechanical,
  bounded addition `pending_step_skips` itself was added as, per that field's own precedent already in
  the codebase.
- `triggers::PendingTrigger` gained a `once_per_turn: bool` field (only 2 real construction sites, both
  in `triggers.rs` itself, plus one internal unit-test helper in `engine.rs` — no test file constructs
  `PendingTrigger` directly) so `Engine`'s `CollectTriggers` arm knows which matches to record.
- **The subtle part:** two qualifying events can land in the *same* `CollectTriggers` batch (e.g. CR
  510.1c's attacker-splits-damage-across-2-blockers shape — one `AssignCombatDamage` command, one
  batch, two `DamageDealt` events). `collect_triggers` itself can't mutate `GameState` (`&GameState`
  only) to record a match immediately, so `Engine`'s `CollectTriggers` arm does it instead, per event,
  before the next event in the same batch is checked — confirmed this is sound before writing it:
  `self.state.ledger` and `self.state.once_per_turn_triggered` are disjoint fields of the same struct,
  so mutating the latter while the `for event in &self.state.ledger[since..]` loop holds a shared
  borrow of the former compiles fine (Rust's disjoint-field-borrow support), no restructuring needed.
- `script.rs`: `Game.Conditions.OncePerTurn(tag)` wraps any other `Game.Conditions.*` tag — a new
  `OncePerTurnTag` `UserData` wrapper (not a second field on `ConditionTag`) so `decode_condition_value`
  can tell the two apart by type at its `AnyUserData::borrow` check, same "distinguish by wrapper type"
  shape the `Primitive`/`Script` split already uses.

**Explicitly not modeled (named, not silently guessed):**
- CR 603.4's intervening-if clause — a real, separate follow-up (see backlog item 7's updated text).
- `ConditionSpec::Script` (a custom Lua condition function) can't carry the once-per-turn cap this
  slice — no engine-side `(source, ability_index)`-keyed slot is threaded into `ScriptContext` for a
  script to consult itself; deferred, not silently guessed at.
- The departed-abilities fallback loop (CR 603.10's `Dies`/`LeavesBattlefield` off an already-purged
  source) got the same once-per-turn treatment for consistency, even though a permanent dying twice in
  one turn (flicker + re-death) is a rare edge case - free to add alongside the main loop's fix, not
  separately verified against a real card.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 3 new tests in
`crates/engine/tests/phase5_trigger_once_per_turn.rs`: an attacker splitting combat damage across two
blockers (CR 510.1c) triggers a once-per-turn "whenever this deals damage" ability exactly once despite
two `DamageDealt` events in the same batch; the same setup *without* the cap fires it twice (pauses on
`AwaitTriggerOrderChoice` with a 2-trigger group — confirms the capped test isn't passing for an
unrelated reason, like only one `DamageDealt` event existing at all); the cap resets once the turn
actually rolls over (driven through the real turn structure — Combat → End → Cleanup → next
Untap/Upkeep — not shortcut). All prior tests unaffected — full workspace run: 454 passed, 0 failed.

**UNRESOLVED:** none new — see "Explicitly not modeled" above.

## Fix: make Untap-step triggers fireable (backlog item 7)

Third of a run of small, bounded fixes picked off backlog item 7's own "Still open" list, still
committed directly to `master`. `AdvanceTurnStep`'s `Untap` branch was the one step-transition
`upkeep-triggers` deliberately left alone (CR 502.4 grants no priority there, so at the time it seemed
like there was no window to place a trigger before) — but CR 603.3's last sentence means a trigger
whose condition is met during Untap just waits to go on the stack until the next time a player *would*
receive priority (Upkeep, immediately after); it doesn't mean the trigger never happens.

**What got implemented:**
- `engine.rs`'s `Untap` branch now routes its events (plus `untap_step`'s own `Untapped` events) through
  the real `ReplacementCheck`/`CommitEvents`/`SbaLoop`/`CollectTriggers`/`PutTriggersApnap` pipeline,
  same as every other step — but with `PipelineExit::AdvanceTurnStep` instead of `BackToPriority`, so it
  still doesn't open a priority window CR 502.4 doesn't grant. Placing a matched trigger onto
  `state.stack` immediately (rather than literally deferring the "put on stack" action until Upkeep's
  window) is functionally equivalent once that window opens — the trigger is already sitting there
  either way.
- `types/ability.rs`: `ConditionPrimitive::BeginningOfYourUntap`/`BeginningOfEachUntap`, mirroring
  `BeginningOfYourUpkeep`/`BeginningOfEachUpkeep` exactly, plus `Game.Conditions.Untap`/`EachUntap`
  bindings (script.rs).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 3 new integration tests in
`crates/engine/tests/phase5_untap_triggers.rs` (a `BeginningOfYourUntap` trigger is already on the
stack by the time Upkeep's priority window opens — the first pause reachable at all — and resolves
normally; doesn't fire for a different controller; `BeginningOfEachUntap` fires regardless of whose
turn it is), plus 5 new unit tests in `types/ability.rs` mirroring the Upkeep ones. All prior tests
unaffected — full workspace run: 462 passed, 0 failed.

**UNRESOLVED:** none new.

## Fix: wire submit_discard into real trigger collection (backlog item 7)

Fourth of the run. `Engine::submit_discard` (CR 514.1's cleanup hand-size discard) committed its
`ZoneChange` events directly to the ledger and jumped straight to `ProcessState::CleanupSbaCheck`,
the same `log_events`-shaped pipeline bypass every other fix in this run has been closing — so a
"whenever you discard a card" trigger off this specific discard source was silently dropped (a
cost-payment `Discard`, via `move_to_graveyard_for_cost`, already reached the real pipeline).

**What got implemented:**
- `engine_context.rs`: new `PipelineExit::CleanupSbaCheck` — unconditional (unlike `CleanupCheck`),
  since a cleanup discard always falls through to a fresh SBA check afterward regardless of whether
  anything triggered, matching `begin_cleanup_step`'s own discard-then-SBA-check sequencing.
- `engine.rs`'s `submit_discard`: stashes each discarded card's departed `Triggered` abilities (via
  `commands::retire_object`) into `departed_triggered_abilities`, then routes its `ZoneChange` events
  through `ProcessState::CollectTriggers` with `pipeline_exit = PipelineExit::CleanupSbaCheck` instead
  of committing directly.
- `types/ability.rs`: `ConditionPrimitive::DiscardsCard`/`EachDiscardsCard`, mirroring
  `DrawsCard`/`EachDrawsCard` exactly (a Hand→Graveyard `ZoneChange` is always a discard in this engine
  today, same "zone-shape only" caveat `DrawsCard` already carries) — the concrete condition proving
  the fix, plus `Game.Conditions.DiscardsCard`/`EachDiscardsCard` bindings.
- `draw_one_card`'s own `retire_object` call now stashes into `departed_triggered_abilities` too, for
  consistency with every other call site — still practically unreachable (no card has a library-side
  ability that cares about itself leaving the library), named rather than silently left as the one
  remaining `let _ = ...` discard.

**Bug found and fixed while wiring this:** `PipelineExit::CleanupCheck`'s priority-vs-advance check
(from the earlier "wire Cleanup's own SBA pass" fix) only compared the ledger's length against its own
`since_ledger` marker — missing a trigger placed *earlier* in the same cleanup sequence, which is
exactly what happens here: `submit_discard`'s own `DiscardsCard` trigger lands on the stack via
`PipelineExit::CleanupSbaCheck` *before* `CleanupSbaCheck`'s own SBA pass ever runs, so that pass's
fresh `since_ledger` marker can't see it and incorrectly concluded nothing was waiting. Fixed by also
checking `!self.state.stack.is_empty()` — either signal now means CR 514.3 opens a priority window.
Caught by `discards_card_trigger_fires_off_the_cleanup_hand_size_discard` actually failing (turn
silently advanced past the pending trigger) before this was added, not discovered by inspection alone.

**Explicitly not modeled (named, not silently guessed):**
- CR 514.3's "another cleanup step begins after this priority window" — same pre-existing TODO
  `finish_cleanup_sba_check` already carries, unaffected by this fix.
- Discard replacement effects (e.g. Leyline of the Void) — `submit_discard`'s own pre-existing TODO,
  unaffected; events still commit directly rather than passing through `ReplacementCheck`.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 2 new tests in
`crates/engine/tests/phase5_discard_triggers.rs`: a `DiscardsCard` trigger fires off a single
cleanup-forced discard, correctly reopening priority (not silently advancing past it) and resolving for
real; discarding two cards at once pauses on `AwaitTriggerOrderChoice` with a 2-trigger group (same
"group size proves it" shape `phase5_trigger_once_per_turn.rs`'s own uncapped test uses), confirming
both discards actually triggered rather than the test passing for an unrelated reason. Plus 5 new unit
tests in `types/ability.rs` mirroring `DrawsCard`/`EachDrawsCard`'s own. All prior tests unaffected —
full workspace run: 469 passed, 0 failed.

**UNRESOLVED:** none new — see "Explicitly not modeled" above.

## Phase 5: phase5/intervening-if slice (backlog item 7)

Branch `phase5/intervening-if`, off master. Last of the run of backlog-item-7 fixes, but scoped as a
real vertical slice (its own requirements report, per the human's request) rather than a small
plumbing fix — CR 603.4's intervening-if clause needs actual new modeling, not just a pipeline-bypass
wire-up. Scope locked in before writing code: checked that `ConditionPrimitive::matches` is
event-shaped (always needs a `&GameEventKind`) and can't represent a pure state predicate; checked
that `ScriptRuntime::invoke_condition`/`invoke_cda` don't pass `&GameState` into Lua at all (no
`Game.*` state-query binding exists yet — a separate, already-flagged DESIGN.md gap), so
`InterveningIfSpec` ships `Primitive`-only this slice, `Script` named as a follow-up.

**Design, confirmed before writing code:**
- New `InterveningIfPrimitive` enum (`types/ability.rs`) with `holds(&self, state, controller) ->
  bool` — a pure state predicate, deliberately not reusing `ConditionPrimitive::matches`'s
  event-shaped signature. One concrete variant: `ControllerAtOrBelowLife(i32)` — real, common, and
  cleanly testable (nothing else needs constructing to flip it, just change life).
- New `InterveningIfSpec::Primitive(InterveningIfPrimitive)` (no `Script` variant yet, per the scope
  note above).
- `RegisteredAbility.intervening_if: Option<InterveningIfSpec>` — a genuine new mandatory struct
  field (~70 call sites, 28 files), not folded into `ConditionSpec` the way CR 603.2h's once-per-turn
  cap was. Checked why once-per-turn could reuse `ConditionSpec` but this can't: once-per-turn is
  still fundamentally "does this match," just capped — intervening-if is checked at a *different*
  pipeline stage entirely (resolution, not just trigger-time) and is orthogonal to the trigger
  condition, not another flavor of it. The mechanical field-addition itself is the same bounded,
  precedented edit `GameState::once_per_turn_triggered` already went through this session.
- `StackObject.intervening_if`, baked in at push time from `RegisteredAbility.intervening_if` (same
  "primitive stand-in, decided once at push, not re-derived" contract `effect`/`target_requirement`
  already establish) — only 3 `StackObject::new` call sites, cheap. A `Spell`/`ActivatedAbility`
  StackObject always gets `None` here (CR 603.4 is triggered-ability-only text; neither push site has
  a `RegisteredAbility` with this field set).
- `triggers::collect_triggers`: checks `ability.intervening_if` (both the main live-ability loop and
  the CR 603.10 departed-abilities fallback loop, for symmetry) against current state, independent of
  the existing trigger-event `condition` match — false means the ability never even gets collected as
  a `PendingTrigger`.
- `engine.rs`'s `ResolveStackTop`: rechecks `top.intervening_if` fresh against current state, OR'd
  into the existing CR 608.2b target-legality `fizzles` bool — same "removed from stack, no effect,
  `Fizzled` logged" observable outcome, no new branch needed.
- `script.rs`: `Game.InterveningIf.AtOrBelowLife(n)` + `TriggeredAbilityBuilder:SetInterveningIf(tag)`,
  mirroring `Game.Conditions.*`/`:SetCondition`'s own shape — a new `InterveningIfTag` `UserData`
  wrapper (not a new `ConditionTag` field) so the two stay distinguishable by type, same "distinguish
  by wrapper type" convention `OncePerTurnTag` already established.

**Mechanical note:** the ~70-call-site `RegisteredAbility` field addition (and the 3 `StackObject`
sites) required per-file fixes beyond the usual bulk `perl` pass — several files use CRLF line endings
(untouched by earlier sessions, unlike this session's own LF-saved files) and a few `RegisteredAbility`
literals reference `operation`/`effect` via local variables rather than the `ScriptHandle(0)`/`Some(...)`
literals the bulk regex matched on. Fixed file-by-file once `cargo build --workspace` surfaced each
remaining `E0063`.

**Explicitly not modeled (named, not silently guessed):**
- `InterveningIfSpec::Script` — deferred until `ScriptContext`/`invoke_condition` expose `&GameState`
  to Lua (a separate, already-flagged gap).
- Only one concrete `InterveningIfPrimitive` variant (life-total). More shapes (hand size, board-state
  checks, "still on the battlefield") are the same category of follow-up every other primitive in this
  file started narrow before growing.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 3 new integration tests in
`crates/engine/tests/phase5_intervening_if.rs` (a trigger whose intervening-if is already false when
the event fires never triggers at all, stack stays empty; one that stays true throughout resolves
normally; the real proof of the two-independent-checks claim: a trigger placed while the condition
holds, then invalidated at resolution by a response spell — cast in response, resolving first since it
sits on top of the stack — correctly fizzles with no effect and a logged `Fizzled` event, while the
response spell's own effect still applies), plus 4 new unit tests for `InterveningIfPrimitive::holds`
in `types/ability.rs`. All prior tests unaffected — full workspace run: 476 passed, 0 failed.

**STRUCT DEVIATIONS:** None — `RegisteredAbility.intervening_if`/`StackObject.intervening_if`/
`InterveningIfSpec`/`InterveningIfPrimitive` were all anticipated exactly in the pre-slice requirements
report.

**INTERFACE DEVIATIONS:** None — the `collect_triggers`/`ResolveStackTop` touch-points and the
`Script`-deferral reasoning were both anticipated in the report before implementation started.

**SCOPE DEVIATIONS:** None — `InterveningIfSpec::Script` and additional primitive shapes were named as
deferred follow-ups in the pre-slice report, not silently dropped.

**UNRESOLVED:** none new — see "Explicitly not modeled" above.

## Phase 5: phase5/script-coroutines slice (backlog item 9, `Game.SelectTargets`)

Branch `phase5/script-coroutines`, off master. Slice backlog item 9 — the first real player-input-
mid-script mechanism, flagged unresolved since Phase 2 part 3. Genuinely open-ended before this
slice (no prior design beyond two-line stub comments and a DESIGN.md open question), so this one
started with a spike rather than a locked design.

**Spike, before any `Engine`-level plumbing:** `crates/engine/tests/phase5_coroutine_spike.rs`,
standalone, no engine code depends on it. Confirmed against the real vendored mlua 0.9.9 (checked
by reading the crate's own source directly, not from memory): `Thread::resume`/`coroutine.yield`
round-trip real values correctly and `Thread::status()` distinguishes suspended-at-yield from
finished-for-good; a suspended `Thread` survives a `create_registry_value`/`registry_value`
round-trip (confirmed `Thread` implements `IntoLua`/`FromLua` in mlua 0.9.9 from the source) — the
exact mechanism `ScriptRegistry` already uses for `Function`s via `RegistryKey`. Both passed on the
first real attempt; the design proceeded exactly as planned with no surprises.

**Design:**
- `ScriptRuntime::invoke_operation` now runs the loaded function as a coroutine
  (`Lua::create_thread` + `Thread::resume`) instead of a plain `.call()` — transparent for any
  script that never yields, since a fresh thread's first `resume` behaves exactly like calling the
  function directly (confirmed in the spike). This is the only invocation path that gets the
  coroutine treatment this slice — `invoke_condition`/`invoke_cda` stay synchronous, since a trigger
  condition or a CDA has no legitimate reason to ask the player anything mid-check.
- `Game.SelectTargets(filter)` is a **plain Lua function**, not a `create_function` Rust closure —
  `coroutine.yield` can only be called by code running *as* a coroutine; mlua 0.9 without the
  `async` feature (not enabled here) has no way for a Rust-bound callback to suspend the Lua stack
  from inside itself. A tiny literal Lua chunk (`function(filter) return coroutine.yield({ op =
  "select_target", filter = filter or {} }) end`) sidesteps needing that entirely, loaded once
  in `bind_game_globals` alongside the rest of the `Game` table.
- New `ScriptChoiceRequest::SelectTarget { filter: TargetFilter }` — reuses `TargetFilter`
  (`types/target.rs`) unchanged rather than inventing a second filter vocabulary; narrowed to
  `TargetFilter::Object` only this slice (no player targets, no "any target," no zone override).
  New `OperationOutcome::{Finished, NeedsChoice}` is what `invoke_operation`/the new
  `resume_operation` return instead of the old bare `Result<(), ScriptError>`.
- New `ScriptRegistry.suspended_threads: HashMap<ScriptHandle, RegistryKey>` — a suspended `Thread`
  stashed under the same `ScriptHandle` id space `scripts` uses (shares `next_handle`), kept in a
  separate map since a suspended thread is consumed exactly once by `resume_operation`, unlike a
  loaded `Function` handle which is reusable.
- `ProcessState`/`StepStatus::AwaitingScriptChoice` mirror `AwaitingReplacementChoice`/
  `AwaitingLegendRuleChoice` exactly (same "own entry point, not `submit_action`" rationale). The
  pause carries `stack_id`/`controller`/`events_so_far` — exactly what `ResolveStackTop` had already
  built for this resolution before hitting the ask — so `Engine::submit_script_choice` can push the
  deferred `StackObjectResolved` and continue into `ReplacementCheck` in the same spot once answered,
  rather than re-deriving any of it. Validates the answer via the existing `commands::
  target_is_legal` (the same check a spell's static target requirement already uses) — no second
  bespoke legality check.
- A script error on `resume_operation` is silently dropped (`Err(_) => {}`), same pre-existing "no
  path from `ResolveStackTop` back to a `StepStatus` error for a mid-resolution script failure"
  stance the original (non-yielding) `invoke_operation` call already had — not a new gap this slice
  introduces.

**Real bug caught while writing the first end-to-end test, not a design flaw:** the first attempt at
an `Engine`-driven test pre-loaded the operation into a standalone `ScriptRuntime` (mirroring a
unit-level test pattern used elsewhere) — but `Engine::new` owns its *own* internal `ScriptRuntime`
(a separate Lua VM entirely), so a `ScriptHandle` from an external one is meaningless to it; the
call silently errored and got swallowed by the same `Err(_) => {}` stance above, with no crash to
signal the mistake. Fixed by switching to real card-script self-registration (`Game.
CreateActivatedAbility`), the same mechanism `phase5_activated_ability_self_registration.rs` already
proves, so the operation is registered through the engine's own runtime. A second, separate mistake
in the same debugging pass: forgetting that paying a cost only commits it — the ability still sits
on the stack until priority passes around (CR 117.4) — caught by a temporary `eprintln!` in
`ResolveStackTop`'s operation-invocation arm (removed before this commit) showing the arm was never
even reached.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 2 new spike tests
(`phase5_coroutine_spike.rs`) plus 6 new tests in `crates/engine/tests/phase5_script_target_selection.rs`
(a self-registered activated ability calling `Game.SelectTargets` pauses `Engine::step()` with the
real filter from the script; submitting a legal target resumes the exact suspended script, which
finishes with its own continuation using the precise resumed value (the chosen creature's own raw
object id, round-tripped through `gain_life` as proof the value reached the script correctly) and
the deferred `StackObjectResolved` lands once resolved; an illegal target is rejected with the pause
left open and no life change; `submit_script_choice` rejects when not awaiting; a script that never
calls `Game.SelectTargets` still resolves synchronously; `invoke_operation` finishes synchronously
at the `ScriptRuntime` level directly, unit-style, confirming the coroutine rewrite doesn't change
existing behavior), all 482 prior tests unaffected — full workspace run: 490 passed, 0 failed.

**What got implemented:**
- `script.rs`: `ScriptChoiceRequest`, `OperationOutcome` (both `pub`). `ScriptRegistry.
  suspended_threads`. `invoke_operation` rewritten around `Lua::create_thread`/`Thread::resume`; new
  `resume_operation`; shared `handle_operation_result` tail (yielded → decode request, stash thread;
  finished → same `gain_life` decode the old body always had, same typed-return strictness). New
  `thread`/`stash_thread` registry helpers, mirroring `function`'s own pattern. New
  `decode_script_choice_request`/`decode_target_filter_table` free functions. `Game.SelectTargets`
  bound as a literal Lua chunk in `bind_game_globals`.
- `engine.rs`: `ScriptChoiceError`, `StepStatus`/`ProcessState::AwaitingScriptChoice`, the "nothing
  to do until answered" `step()` arm for it, `Engine::submit_script_choice`. `ResolveStackTop`'s
  `None => invoke_operation(...)` arm rewritten to branch on `OperationOutcome`, pausing (early
  `return StepStatus::Progressed`, same pattern already used elsewhere in `step()`) rather than
  falling through to `StackObjectResolved`/`ReplacementCheck` when the script yields.
- `crates/scenario/src/lib.rs`: `drain`'s exhaustive `StepStatus` match gained
  `AwaitingScriptChoice`, same "stop draining, this run reached a pause" treatment every other pause
  status already gets. No new TOML/CLI step type — checked before writing code: neither CR 616.1's
  nor the legend-rule choice-pause has one either (no existing precedent to extend), consistent
  with that same standing decision, not a gap unique to this slice.

**STRUCT DEVIATIONS:** None — `ScriptChoiceRequest`/`OperationOutcome`/the new `ProcessState`/
`StepStatus` variants were all anticipated exactly in the pre-slice requirements report.

**INTERFACE DEVIATIONS:** `invoke_operation`'s return type changing from `Result<(), ScriptError>`
to `Result<OperationOutcome, ScriptError>` was anticipated by name in the report's design section,
not discovered mid-implementation.

**SCOPE DEVIATIONS:** None beyond the two real bugs already described under their own heading above
— both caught and fixed while writing this slice's own first end-to-end test, not silently worked
around.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Modal spells / "may" abilities (backlog item 9a) — same mechanism, different request shape, own
  follow-up.
- `Target::Player` selection via `Game.SelectTargets` (backlog item 9b) — object-only this slice,
  same controller-only-style narrowing `DrawCards`/`Shuffle` already set precedent for.
- `invoke_target`/`TargetSpec::Script` — a *static* target-requirement predicate (evaluated once at
  cast/activation time, before the stack), a different concept from an operation's own mid-
  resolution ask; untouched, still `todo!()`.
- Nested/sequential asks within one script (ask twice in a row) — the resume design accounts for it
  structurally (`NeedsChoice` can recur), but no test exercises it this slice.
- A script error on `resume_operation` is silently dropped, same pre-existing stance the original
  `invoke_operation` already had — not expanded or newly introduced here.

## Mistakes caught (don't repeat)

- Claimed hybrid Phyrexian mana symbols (`{W/U/P}`) needed a 3-element container because they're
  "a true 3-alternative choice." Wrong framing — confused *payment options* with *symbol slots*.
  An atom can itself carry multiple payment options (`Phyrexian(Color)` = color or life), so a
  2-slot `Hybrid` of two atoms can yield more than 2 total ways to pay. Before declaring a data
  shape "can't represent X," check whether an existing variant already absorbs the complexity.
- **PowerShell 5.1 `-replace` with ScriptBlock** (phase5/script-ops-create-token): `$content -replace
  "pattern", { ScriptBlock }` is PowerShell 7+ only. In Windows PowerShell 5.1 (the environment here),
  the ScriptBlock is treated as a literal string — every match site gets `$nl = use std::...` inserted
  verbatim instead of the intended content. Result: `engine.rs` ballooned from ~2900 lines to 20000+
  (7 apparent copies of every function). Fix: always use line-by-line PowerShell processing (read with
  `[System.IO.File]::ReadAllLines`, walk the array, splice at a target index) for any insertion that
  needs to be position-sensitive. `-replace` with a literal replacement string (no ScriptBlock) is
  fine. Reverted with `git checkout HEAD -- <files>` and reapplied correctly.
- First draft of `replacement::apply_replacements`'s CR 616 stabilization loop had no termination
  condition for a `Repeatable`/multi-`LimitedUses` effect matching its own already-rewritten output
  again (`PreventDamage` zeroing an already-zero `amount` doesn't stop its own filter from matching)
  — `cargo test` hung rather than failed. `ConsumptionPolicy` governs whether an effect survives
  for *future* events, not whether it can reapply to the *same* event repeatedly; CR 616.1's actual
  rule ("each applicable replacement effect can apply only once to a given event") was missing
  entirely. Any "loop until no further match" stabilization/fixed-point design needs an explicit
  per-resolution exclusion set from the start, not just "the rewritten form might not match
  anymore" — a rewrite that doesn't change the property being matched on will match forever
  otherwise. Caught by the test suite (two tests visibly stalled) before merge, not after.

## Phase 5: phase5/first-strike slice (backlog item 12)

Branch `phase5/first-strike`, off master. CR 510.1a/702.4/702.7: first strike and double strike
split the combat damage step into two (a first-strike step, then the regular step) whenever any
attacker or blocker has either keyword; otherwise combat is unchanged. Confirmed before writing
code that `turn.rs`'s `Step` enum had exactly one `CombatDamage` step — the two-step sequence
didn't exist at all.

**What got implemented:**
- `types/card.rs`: `KeywordSet::FIRST_STRIKE`/`DOUBLE_STRIKE` (bits 12/13, CR 702.7) plus matching
  `Keyword` enum variants; wired into `parse_keyword_name` in both `script.rs` and the scenario
  crate's `parse_keywords`, same as every existing keyword.
- `types/turn.rs`: new `Step::FirstStrikeDamage`, inserted between `DeclareBlockers` and
  `CombatDamage`.
- `engine.rs`'s `advance_turn_step`: the `(Phase::Combat, Step::DeclareBlockers)` arm now checks
  (via `effective_keywords`, so a layer-6-granted keyword counts too) whether any current attacker
  or blocker has `FIRST_STRIKE`/`DOUBLE_STRIKE`; if so, next step is `FirstStrikeDamage`, else
  straight to `CombatDamage` exactly as before this slice. A new `(Phase::Combat,
  Step::FirstStrikeDamage)` arm unconditionally advances to `CombatDamage` next.
- `commands.rs`'s `AssignCombatDamage`: accepts either damage step now, and got a real rewrite of
  its setup rather than a bolted-on branch. New `participates_in_damage_step(keywords, step) ->
  bool` decides, per creature, whether it assigns/deals damage in the current step. Three derived
  lists replace direct use of `combat.attackers`/`combat.blocks`:
  - `attackers`: alive AND itself step-eligible - decides whether an attacker deals damage *at
    all* this step.
  - `alive_blocks`: alive only, deliberately **not** filtered by the blocker's own step-eligibility
    - which step an attacker's damage lands in is decided by the attacker's keywords, not the
      blocker's, so a first-strike attacker's damage still routes to a perfectly ordinary blocker
      during the first-strike step. Feeds `blockers_by_attacker`/`multi_blocked` (further scoped to
      attackers present in `attackers`, so a non-participating attacker never demands a split) and
      the attacker-side damage-event loop.
    - `blocks`: `alive_blocks` further filtered by the blocker's own step-eligibility - decides
    whether a blocker strikes back *this step*, used only by the blocker-side damage-event loop.

**Bug found and fixed while wiring this:** `AssignCombatDamage` previously hard-errored
(`InvalidTarget`) the instant any attacker/blocker id in `combat` no longer resolved in
`state.objects` - harmless before first strike (only reachable if a spell killed a combatant in a
priority window before `CombatDamage`, rarely hit), fatal now that a first-strike kill is the
central case: a first-striker routinely kills its blocker in the first step, and that blocker's id
genuinely won't resolve by the time the regular step's `AssignCombatDamage` runs. Fixed by the
alive-filtering above rather than a special case - a missing id is just an ineligible participant
now, same code path as any other exclusion.

**Bug found and fixed by the test suite, not anticipated in the requirements report:** an early
version filtered `alive_blocks` by the blocker's own step-eligibility too (matching a literal
reading of the pre-implementation design). That broke the core first-strike interaction outright -
a first-strike attacker blocked by an ordinary creature stopped dealing damage to it at all during
the first-strike step, because the blocker (no first strike) was filtered out of
`blockers_by_attacker` and the attacker fell into the "unblocked" branch instead. Real CR 510.1a:
the *attacker's* keywords decide which step its damage is dealt in, regardless of the blocker's own
keywords - the blocker's keywords only decide whether *it* strikes back that step. Caught by
`first_strike_attacker_kills_blocker_before_it_deals_damage_back` actually failing (blocker survived
undamaged) before this split into two separately-filtered lists (`alive_blocks` vs `blocks`) was
added.

**Explicitly not modeled (named, not silently guessed):**
- CR 509.1h-style "attacker removed from combat" bookkeeping beyond the damage consequence itself -
  `combat.attackers`/`combat.blocks` still list a dead participant's id after it dies mid-combat
  (no broader blocked/blocking status tracking exists or is touched here, same as every other
  combat slice so far).
- A continuous effect granting first/double strike - `effective_keywords` already covers this for
  free (CR 613 layer 6), not separately implemented or tested this slice (the report's own
  read-through of `effective_keywords`'s existing doc comment was enough to confirm it, not worth a
  dedicated test given every other keyword check in this file already relies on the same helper).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` - 6 new integration tests in
`crates/engine/tests/phase5_first_strike.rs`: no participant with either keyword skips
`FirstStrikeDamage` entirely (single `AdvanceTurnStep` hop from `DeclareBlockers` lands directly on
`CombatDamage`); a first-strike attacker's presence inserts `FirstStrikeDamage` in that same single
hop; an unblocked first striker deals damage only in the first-strike step; an unblocked double
striker deals damage in both steps (total 2x power); the core interaction - a first-strike attacker
kills an ordinary blocker in the first-strike step, the blocker never strikes back, and the
regular step's `AssignCombatDamage` afterward silently skips the now-dead blocker instead of
`InvalidTarget`-erroring; and a multi-blocked ordinary attacker's split requirement is scoped to the
step it actually participates in (no split demanded during a first-strike step it sits out, one
demanded during the regular step it does participate in). All prior tests unaffected - full
workspace run: 482 passed, 0 failed (476 prior + 6 new).

**STRUCT DEVIATIONS:** `alive_blocks` (a third derived list alongside `attackers`/`blocks`) wasn't
anticipated in the pre-slice report, which described only two filtered lists. Needed once testing
surfaced that blocker-side step-eligibility must NOT gate where an attacker's damage routes - see
the bug note above.

**INTERFACE DEVIATIONS:** None - `participates_in_damage_step`'s signature and the
`advance_turn_step`/`AssignCombatDamage` touch-points match the pre-slice report exactly.

**SCOPE DEVIATIONS:** None - both "explicitly not modeled" items were named as deferred in the
pre-slice report, not silently dropped.

## Phase 5: phase5/script-target-players slice (backlog item 9b)

Branch `phase5/script-target-players`, off master. Slice backlog item 9b, split out of the
`phase5/script-coroutines` slice - `Game.SelectTargets` widened to accept `Target::Player`/CR
115.4's "any target", not just `Target::Object`. Confirmed before writing code that the *static*
targeting path (`TargetFilter::Player`/`AnyTarget`, `commands::target_is_legal`) already handled
players correctly from the earlier `phase5/targeting-basics` slice - this slice only had to get a
`Target::Player` value into that existing check from the script-choice (mid-resolution) path, not
rebuild any legality logic.

**Design:**
- `decode_target_filter_table` (script.rs) gained a `type` discriminator - `"object"` (default,
  so every pre-existing script that omits it keeps working unchanged), `"player"`, `"any"` -
  matching the scenario crate's own TOML `target_requirement.filter` convention
  (`build_target_requirement` in `crates/scenario/src/lib.rs`) exactly rather than inventing a
  second vocabulary.
- `ScriptRuntime::resume_operation`'s `answer` widened from `ObjectId` to `Target`, and
  `Engine::submit_script_choice`'s `chosen` likewise - `target_is_legal` itself needed zero
  changes, since it already branched correctly on `Target::Player`/`Target::Object` for
  `TargetFilter::Player`/`AnyTarget` from the static-targeting slice.
- Real decision locked in up front: the value resumed into the Lua coroutine changed from a bare
  number (`answer.0`) to a tagged table `{ kind = "object"|"player", id = n }`. `ObjectId` (`u64`)
  and `PlayerId` (`u32`) are both plain numbers to Lua - a raw-number answer worked when only
  objects existed, but once a player answer is possible a script needs an explicit way to tell them
  apart rather than trusting its own memory of which filter it asked for. This changes the existing
  test script's `gain_life = target` to `gain_life = target.id` - an expected, up-front-flagged
  interface deviation, not a surprise found mid-implementation.
- No new TOML/CLI step - `submit_script_choice` stays engine-API-only, same precedent the original
  `phase5/script-coroutines` slice already set (no scenario-file surface exists for
  `AwaitingScriptChoice` at all).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` - 5 new tests in
`phase5_script_target_selection.rs` (player-filter pause carries `TargetFilter::Player`; submitting
a legal player answer resumes with `kind == "player"` actually reaching the script, proven by
folding `target.kind` into the resulting life total rather than adding a new effect-shape key just
for this test; an object answer against a player-only request is rejected, `InvalidCandidate`;
`{ type = "any" }` pauses with `TargetFilter::AnyTarget` and accepts both a creature and a player),
plus the 4 pre-existing tests in that file updated for the tagged-table answer shape. Full
workspace run: 495 passed, 0 failed (490 prior + 5 new).

**STRUCT DEVIATIONS:** None - `Target`, `TargetFilter::Player`/`AnyTarget` all pre-existed from the
targeting-basics slice; nothing new was added to either type.

**INTERFACE DEVIATIONS:** None beyond what the requirements report named up front
(`resume_operation`/`submit_script_choice` widening from `ObjectId` to `Target`, and the resumed
value's tagged-table shape).

**SCOPE DEVIATIONS:** None.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- `TargetFilter::Player`'s own pre-existing TODO (no controller restriction - "target opponent"
  specifically vs. any player) stays open, not bundled into this slice.
- 9a (modal spells / "may" abilities) - untouched, same coroutine mechanism but a different
  `ScriptChoiceRequest` variant, own follow-up.

**UNRESOLVED:** none new - see "Explicitly not modeled" above.

## Phase 5: phase5/script-may-abilities slice (backlog item 9a, narrowed to "may" abilities)

Branch `phase5/script-may-abilities`, off master. Slice backlog item 9a, narrowed before writing
code to CR 608.2d "may" abilities only - a script offering a plain yes/no choice mid-resolution
("You may sacrifice a creature. If you don't, you lose 4 life."), reusing the exact
`Game.SelectTargets` coroutine mechanism (`invoke_operation`/`resume_operation`/
`AwaitingScriptChoice`) with a new request shape, exactly as backlog item 9a's own text predicted.
True modal spell mode-selection (CR 700.2a - the mode is chosen *before* the stack, as part of
casting, not mid-resolution) was split out as its own named follow-up (9a-i) rather than guessed
at - a fundamentally different mechanism, closer to `TargetRequirement`/`PaymentPlan` than to a
coroutine yield inside an already-resolving operation.

**What got implemented:**
- `script.rs`: `ScriptChoiceRequest` gained `MayChoose` (no payload - there's nothing to filter,
  unlike `SelectTarget`'s `TargetFilter`). `decode_script_choice_request` gained the `"may_choose"`
  `op` arm.
- A new literal Lua binding `Game.MayChoose()` (`function() return coroutine.yield({ op =
  "may_choose" }) end`), same "plain Lua function calling `coroutine.yield` directly" pattern
  `Game.SelectTargets` already established - mlua 0.9 without the `async` feature still can't yield
  from inside a Rust-bound closure.
- `ScriptRuntime::resume_operation_bool` - a new sibling to `resume_operation`, resuming the
  suspended coroutine with a raw Lua boolean instead of `SelectTargets`' tagged `{kind, id}` table
  (there's nothing to tag - the yielded request only ever means "yes or no"). Shares
  `handle_operation_result` as its tail, same structure `resume_operation` already uses.
  `resume_operation` itself is untouched.
- `Engine::submit_script_may_choice(bool)` - a new sibling to `submit_script_choice`, matching
  `ScriptChoiceRequest::MayChoose` instead of `SelectTarget`. No legality check: unlike a target
  answer, any `bool` is legal for a "may" choice. Deliberately a separate entry point rather than
  widening `submit_script_choice`'s own `chosen: Target` parameter - same "one method per pause
  shape" precedent `submit_replacement_choice`/`submit_legend_rule_choice` already set.
- `ScriptChoiceError` gained `WrongRequestKind` - submitting a target answer to a pending
  `MayChoose` pause, or a bool answer to a pending `SelectTarget` pause, is rejected explicitly
  rather than silently misapplied or panicking.

**Real, anticipated interface deviation (flagged in the pre-slice report):** `submit_script_choice`'s
existing body destructured `request` as `let ScriptChoiceRequest::SelectTarget { filter } =
request;` - an *irrefutable* pattern, since only one variant existed before this slice. Adding
`MayChoose` made that refutable; fixed to a `let-else` returning the new `WrongRequestKind` error.
The same now-refutable pattern existed in three tests in the pre-existing
`phase5_script_target_selection.rs` (added by the `phase5/script-target-players` slice) - all three
fixed the same way. Neither was a surprise found mid-implementation; both were called out as
required, anticipated fallout in the pre-slice requirements report.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` - 7 new tests in
`crates/engine/tests/phase5_script_may_abilities.rs` (a script calling `Game.MayChoose()` pauses
with a `MayChoose` request; answering `true` resumes into the "yes" branch, proven via a distinct
life total; answering `false` resumes into the "no" branch; `submit_script_choice` rejects a
pending `MayChoose` pause with `WrongRequestKind` and the pause stays open; `submit_script_may_choice`
rejects a pending `SelectTarget` pause the same way; `submit_script_may_choice` rejects when the
engine isn't awaiting a choice at all; a script that never calls `Game.MayChoose()` still resolves
synchronously, unaffected regression guard). Full workspace run: 502 passed, 0 failed (495 prior +
7 new).

**STRUCT DEVIATIONS:** None - `ScriptChoiceRequest::MayChoose` is exactly the payload-free variant
the pre-slice report described.

**INTERFACE DEVIATIONS:** The `submit_script_choice`/test-file `let-else` fixes above - both
anticipated in the pre-slice report, not surprises.

**SCOPE DEVIATIONS:** None - modal spell mode-selection was named as a split-off follow-up (9a-i)
before any code was written, not discovered mid-slice.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- 9a-i (modal spell mode-selection, CR 700.2a) - untouched, needs its own cast-time choice design,
  not the mid-resolution coroutine mechanism this slice reused.
- `handle_operation_result` still only decodes `gain_life` from a finished operation's result table
  - a `MayChoose`-driven script is exactly as limited to that one effect key as every other
  operation today (pre-existing gap, same TODO already on that function, not expanded here).
- Nested/repeated `Game.MayChoose()` calls in one script (ask twice) - structurally supported (the
  same `NeedsChoice`-can-recur design every prior coroutine slice relied on) but not specifically
  tested this slice, same stance the original `phase5/script-coroutines` slice took for repeated
  `Game.SelectTargets` calls.

Docs updated: `docs/lua_scripts.md` gained a new "Asking the player a yes/no question
mid-resolution" section, and its stale "Not yet possible" note (still claiming player-target
selection wasn't built, despite `phase5/script-target-players` having shipped it) was corrected
alongside the "may"-abilities addition rather than left half-wrong next to this slice's own edit.

## Phase 5: phase5/script-modal-spells slice (backlog item 9a-i)

Branch `phase5/script-modal-spells`, off master. Slice backlog item 9a-i, split out of the
`phase5/script-may-abilities` slice — CR 700.2a modal spell mode-selection, narrowed to "choose
exactly one" (the common "Choose one —" case). Confirmed before writing code that this entry's own
framing ("closer to `TargetRequirement`/`PaymentPlan` than to a coroutine yield") undersold how
cheap it actually was: `PlayerActionKind::CastSpell::modes: Vec<u32>` and the TOML `cast_spell`
action's matching `modes = []` field already flowed end-to-end (all the way onto
`StackObject.chosen_modes`) as dead plumbing before this slice — nothing ever read them. No new
`ProcessState`/`StepStatus` pause was needed at all; mode and targets are announced atomically in
the same `CastSpell` action already, matching how targets themselves already work (no
`AwaitingTargets` pause exists either — see `commands.rs`'s `execute_command`).

**What got implemented:**
- `types/card.rs`: new `SpellMode { target_requirement: Option<TargetSpec>, effect:
  Option<AbilityEffect> }`. `CardDefinition` gained `modes: Vec<SpellMode>` — empty for the
  overwhelming majority of (non-modal) cards, which keep using the struct's own singular
  `target_requirement`/`effect` fields exactly as before this slice.
- `commands.rs`'s `CastSpell` arm: a new mode-resolution step runs before target validation (CR
  601.2b precedes 601.2c). Non-modal card (`definition.modes` empty): the action's `modes` must
  also be empty, or `CommandError::IllegalAction` (reusing the existing catch-all variant — no new
  error type needed); target requirement/effect come from the card's own singular fields,
  unchanged. Modal card: the action's `modes` must contain exactly one valid index into
  `definition.modes`, or `IllegalAction`; the chosen `SpellMode`'s own `target_requirement`/`effect`
  are used for the rest of casting (`validate_targets`) and for what gets baked onto the pushed
  `StackObject`, replacing the card's own singular fields entirely for that cast.
- No TOML schema for modal cards — `effect` already has no TOML representation at all (pre-existing
  gap noted in the scenario crate), so a modal test fixture is a direct `CardDefinition` struct
  literal, same precedent `phase5_script_ops_damage.rs`'s `shock_definition()` already set. Noted
  with a TODO at both scenario-crate `CardDefinition` construction sites.

**Anticipated mechanical fallout (flagged in the pre-slice report, not a surprise):** adding
`modes: Vec<SpellMode>` to `CardDefinition` broke every existing struct literal — same precedented
cost the `effect` field's own addition paid (`phase5/script-ops-damage`: "mechanical sweep across
56 literals/40 files"). Fixed the same way here, but via a small throwaway Python script driving
`cargo build --tests --message-format=json` in a loop (find every `E0063 missing field \`modes\``
span, insert `modes: Vec::new(),` before that literal's closing brace, rebuild, repeat) rather than
by hand — cheaper and immune to human copy-paste error across ~60 files. Six passes to reach zero
errors; every insertion verified by the final `cargo test --workspace` run, not just "it compiled."

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 8 new tests in
`crates/engine/tests/phase5_script_modal_spells.rs` (choosing mode 0 resolves `GainLife` and never
touches mode 1's `AnyTarget` requirement or its `Damage` effect; choosing mode 1 resolves `Damage`
against the supplied target and never touches mode 0's `GainLife`; mode 1's own target requirement
rejects a cast with no target the same way a non-modal spell's `target_requirement` does —
`InvalidTarget`, no special-casing; casting a modal card with zero modes chosen is rejected,
`IllegalAction`, card stays in hand, ledger empty; choosing two modes is rejected this slice — CR
700.2d out of scope; an out-of-range mode index is rejected; a non-modal card rejects an announced
mode choice, symmetric with the modal-card checks; a non-modal card with an empty `modes` list
still casts exactly as before — regression guard). Full workspace run: 510 passed, 0 failed (502
prior + 8 new).

**STRUCT DEVIATIONS:** None beyond what the pre-slice report named — `SpellMode` and
`CardDefinition::modes` are exactly the shape described.

**INTERFACE DEVIATIONS:** None — `commands.rs`'s `CastSpell` arm gained a resolution step, not a
signature change; `PlayerActionKind::CastSpell::modes` and the TOML `cast_spell` action's `modes`
field both pre-existed untouched.

**SCOPE DEVIATIONS:** None — the mechanical struct-literal sweep was explicitly anticipated and
named in the pre-slice report as precedented cost, not discovered mid-slice.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- CR 700.2b (a modal *activated/triggered* ability, as opposed to a modal spell) — untouched;
  `RegisteredAbility` has no `modes` field of its own.
- CR 700.2d (choosing more than one mode, or the same mode twice), 700.2e (another player chooses),
  700.2h (a mode carrying its own additional cost), 700.2i (pawprint mode budgets) — all
  unimplemented; `CardDefinition::modes`'s own doc comment TODOs name each explicitly.
- CR 700.2g (copying a modal spell copies its mode) — moot, no Copy mechanic exists in the engine
  at all yet.
- Pre-filtering illegal modes before the player picks (CR 700.2a: "if one of the modes would be
  illegal... that mode can't be chosen") — this slice only rejects post-hoc via the existing
  target-validation error path once a mode is announced, same as any other illegal cast; there's no
  mechanism offering the caster a pre-filtered legal-modes list.
- No TOML schema for modal cards — `[[cards.modes]]` or equivalent doesn't exist; same "direct
  struct literal only" limit `effect`/`alternative_costs` already have.

## Phase 5: phase5/scry-surveil slice (backlog item 14a)

Branch `phase5/scry-surveil`, off master. Slice backlog item 14a, split out of the
`phase5/script-ops-shuffle` slice — CR 701.22 (Scry) and CR 701.25 (Surveil). Bundled together in
one slice (not split further) since they're mechanically identical past "where does the 'not kept
on top' pile go" — bottom of library (pure reorder) versus the graveyard (a real zone move).

**What got implemented:**
- `AbilityEffect::Scry(u32)`/`Surveil(u32)` — `Game.Ops.Scry(n)`/`Game.Ops.Surveil(n)`, same shared
  enum every other primitive and `CardDefinition.effect` already use, so both an ability's operation
  script and a directly-cast instant/sorcery get it for free (same "one enum, two producers"
  precedent `Damage`/`Destroy` already established).
- Unlike every earlier primitive, these can't resolve in one step. `ResolveStackTop` peeks the
  controller's top `n` library ids read-only (no mutation), then pauses on a new
  `ProcessState`/`StepStatus::AwaitingScryChoice`/`AwaitingSurveilChoice` — same "stack_id/
  controller/events_so_far carried through the pause" shape `AwaitingScriptChoice` established for
  backlog item 9, but reached directly from `ResolveStackTop`'s own `AbilityEffect` match rather than
  through the Lua coroutine/`ScriptChoiceRequest` mechanism (there's no player-facing Lua callback
  mid-resolution here — the whole decision is native). CR 701.22b/701.25c: `n == 0` or an empty
  library falls straight through — no pause, no event.
- Two new entry points: `Engine::submit_scry_choice(kept_on_top, put_bottom)` and
  `submit_surveil_choice(kept_on_top, put_graveyard)`. Both validate the two lists together contain
  exactly the pending `cards` set once each via a new `is_exact_partition` helper (`is_permutation`'s
  own sibling — same manual-multiset-comparison shape, kept separate since this checks two lists
  against one reference set rather than one list against another). Scry's piles are a pure reorder —
  `Zone::remove`/`push_top`/`push_bottom` directly, no new `ObjectId`s, same convention
  `shuffle_library` already uses. Surveil's graveyard pile is a real library → graveyard move (CR
  400.7 new `ObjectId` per card, `commands::retire_object`), same convention `submit_discard` already
  established. Both resume the paused resolution the same way `submit_script_choice` does: push the
  new summary event + the deferred `StackObjectResolved`, then continue into `ProcessState::
  ReplacementCheck`.
- New `GameEventKind::CardsScried { player, kept_on_top, put_bottom }`/`CardsSurveiled { player,
  kept_on_top, put_graveyard }` — one event per scry/surveil (not per card), same granularity
  `LibraryShuffled` already uses. Already reaches `CollectTriggers`/`PutTriggersApnap` for free (same
  `events` vec every other resolution effect flows through) — but see UNRESOLVED below.
- New `ScryChoiceError`/`SurveilChoiceError` (`NotAwaitingChoice`, `WrongCards`).
- `Game.Ops.Scry(n)`/`Game.Ops.Surveil(n)` Lua bindings, same shape as `Game.Ops.GainLife(n)`.

**Mechanical fallout:** none beyond a `match` exhaustiveness ripple — two enum variant additions to
`AbilityEffect`, not a new struct field, so no `CardDefinition`-literal sweep this time (unlike
`effect`/`modes`'s own additions). Two non-exhaustive-match compile errors surfaced and were fixed:
`commands.rs`'s `ActivateAbility` arm (added `Scry`/`Surveil` to its existing non-mana-effect
or-pattern) and `crates/scenario/src/lib.rs`'s `drain` (added the two new `StepStatus` variants to
its pass-through match).

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 11 new tests in
`crates/engine/tests/phase5_scry_surveil.rs` (Scry 2 keeping both on top in a chosen order reorders
correctly; Scry 2 to bottom in a chosen order; Scry 3 split 1-top/2-bottom; Scry 0 resolves
synchronously with no pause or event; Scry N greater than library size takes however many cards
exist, no panic; `submit_scry_choice` rejects a wrong/incomplete answer and leaves the pause open;
`submit_surveil_choice` rejects answering a pending Scry pause with `NotAwaitingChoice`; Surveil 2
keeping both on top is a pure reorder with no graveyard move; Surveil 2 to graveyard is a real
`ZoneChange` per card under new `ObjectId`s; `Game.Ops.Scry`/`Game.Ops.Surveil` Lua bindings decode
to the right `AbilityEffect`). Full workspace run: 521 passed, 0 failed (510 prior + 11 new).

**STRUCT DEVIATIONS:** None — `AbilityEffect::Scry`/`Surveil`, the two new `ProcessState`/
`StepStatus` pause-variant pairs, and the two new `GameEventKind` variants all match the pre-slice
report's shape exactly.

**INTERFACE DEVIATIONS:** None — `submit_scry_choice`/`submit_surveil_choice` match the report's
signatures exactly. The `is_exact_partition` helper wasn't named in the pre-slice report but is a
direct, precedented consequence of "validate the two lists together form the pending set" (same
role `is_permutation` already plays for trigger ordering) — not a surprise, just an implementation
detail the report described in prose rather than by name.

**SCOPE DEVIATIONS:** None — the `commands.rs`/`scenario` crate match-exhaustiveness fixes were
anticipated ("adding new `AbilityEffect` variants" always ripples through every exhaustive match on
that enum) even though the pre-slice report didn't enumerate the exact two call sites by name.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- CR 701.22d/701.25d ("whenever a player scries/surveils" trigger) — the `CardsScried`/
  `CardsSurveiled` event already reaches `CollectTriggers`, but no `ConditionPrimitive::Scries`/
  `Surveils` exists yet to match it (extends backlog item 7, same "primitive ships before its own
  trigger condition" precedent `Damage`/`DrawCards` already set).
- CR 701.22c (simultaneous multi-player scry, APNAP ordering) — single-player-effect shape only.
- CR 701.25b ("look at additional cards while you surveil") — not modeled, always exactly `n` cards.
- CR 616.1 replacement-effect ordering on a scry/surveil — not modeled; no real card replaces a
  scry/surveil in practice, so this mirrors `Destroy`/`Exile`'s own deferred-ordering stance rather
  than a new gap.
- No TOML schema for a Scry/Surveil card — same pre-existing `effect`-has-no-TOML-representation gap
  every `CardDefinition.effect`-carrying slice already notes; a test fixture needed a direct struct
  literal.

## Phase 5: phase5/search-tutor slice (backlog item 14b)

Branch `phase5/search-tutor`, off master. Slice backlog item 14b — CR 701.23 (Search).

**Correction to the backlog text, confirmed before writing code:** the "look at the whole library
visibility exception" this item worried about was a non-issue — `GameState`/`Zone` already expose
full contents freely; the project's only hidden-info gap (DESIGN.md's open question) is the *Lua*
callback boundary, never crossed by this Rust-native primitive. The real blocker, surfaced while
scoping: `cost.rs`'s `FilterSpec` (shared by every `AdditionalCost::Sacrifice`/`Discard`/
`TapPermanents` filter) had no supertype/subtype fields at all, so it couldn't match "basic land
card" — the single most common real search target. Raised to the human before writing any code;
decided to extend `FilterSpec` itself (not a narrower Search-only filter type), fixing the gap for
every existing caller too.

**What got implemented:**
- `FilterSpec` gained `supertypes: Option<SupertypeSet>` and `subtypes: Option<Vec<Subtype>>`,
  checked in `matches()` alongside the existing `card_types`/`controller` checks. Both default to
  `None` on every pre-existing construction site (mechanical sweep across 16 call sites — 2 in
  `src/`, 14 in tests — same "widen a shared struct, ripple through every literal" cost the `effect`/
  `modes` fields already paid). `decode_filter_table` (the Lua-table decoder `Game.Costs.Sacrifice`/
  `Discard`/`TapPermanents` already used) extended to read `supertypes`/`subtypes` arrays too, reusing
  the existing `parse_supertype_name` helper (already present, unused until now) for the former and a
  plain `Subtype(String)` wrap for the latter.
- `AbilityEffect::Search { filter: FilterSpec, count: u32, destination: SearchDestination,
  shuffle_after: bool }` — `Game.Ops.Search(filter_table, count, opts?)`, same shared enum every
  other primitive uses. `SearchDestination { Hand, Battlefield { tapped: bool } }` — narrowed to the
  two overwhelmingly common real destinations.
- Unlike `Scry`/`Surveil` (fixed top-N peek), `ResolveStackTop` scans the controller's *entire*
  library, pre-filters to ids matching `filter` (evaluated with the controller as `viewer`), and
  pauses on a new `ProcessState`/`StepStatus::AwaitingSearchChoice` carrying that pre-filtered
  `candidates` list plus `count`/`destination`/`shuffle_after` — empty `candidates` falls straight
  through with no pause and no event, same "nothing to decide" stance `Scry`/`Surveil`'s own `n == 0`
  case takes.
- `Engine::submit_search_choice(chosen: Vec<ObjectId>)` — CR 701.23b: `chosen` may be shorter than
  `count`, including empty; validated against `candidates` directly (no re-filtering — the pre-filter
  already did that work) plus a duplicate check. `Hand` destination mirrors `submit_surveil_choice`'s
  graveyard-move convention (`retire_object`, no script); `Battlefield` mirrors
  `move_resolving_spell`'s own battlefield-entry convention instead (`remap_abilities`, a real ETB
  `run_card_script`, `controlled_since_turn` set to the current turn, `tapped` applied as requested).
  Shuffles afterward iff `shuffle_after`, via the existing `shuffle_library` helper — no new event
  needed for the shuffle itself, it already logs its own `LibraryShuffled`/`RngDrawn`.
- New `SearchChoiceError` (`NotAwaitingChoice`, `TooMany`, `NotACandidate`).
- No new summary `GameEventKind` — unlike Scry/Surveil, CR 701.23 has no "whenever you search"
  trigger rule to eventually wire toward, so the real per-card `ZoneChange` events (Library ->
  Hand/Battlefield) plus `LibraryShuffled` already say everything a replay/trigger needs.

**Mechanical fallout:** the `FilterSpec` field addition rippled through 16 existing construction
sites (fixed via `sed` for the 14 single-line ones, by hand for 2 multi-line ones) — anticipated and
named in the pre-slice report as a scope decision, not a surprise. The new `AbilityEffect::Search`
variant rippled through the same two exhaustiveness-check call sites `Scry`/`Surveil` already hit
(`commands.rs`'s `ActivateAbility` non-mana-effect or-pattern, `crates/scenario/src/lib.rs`'s
`drain`) — also anticipated, same "adding an `AbilityEffect` variant always ripples through every
exhaustive match on that enum" precedent.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 2 new tests in `cost.rs`'s own test module
(`supertypes_true_and_false_cases`, `subtypes_true_and_false_cases`) plus 10 new tests in
`crates/engine/tests/phase5_search_tutor.rs` (finds a basic land and puts it in hand; choosing zero
is legal per CR 701.23b; rejects choosing more than `count` with the pause left open; rejects a
non-matching id, proving `candidates` is pre-filtered rather than post-hoc-checked; rejects the same
id chosen twice; search-to-battlefield-tapped sets `tapped` and runs the real ETB script;
`shuffle_after` defaults true and logs `LibraryShuffled`; `shuffle_after: false` leaves the remaining
library order untouched and logs no shuffle; the `Game.Ops.Search` Lua binding decodes filter/count/
destination/shuffle correctly; and decodes the right defaults — `hand`/`true` — when `opts` is
omitted entirely). Full workspace run: 533 passed, 0 failed (521 prior + 12 new).

**STRUCT DEVIATIONS:** None beyond the pre-slice report's own named decision — `FilterSpec`'s two new
fields, `AbilityEffect::Search`, and `SearchDestination` all match the shape described.

**INTERFACE DEVIATIONS:** None — `submit_search_choice`'s signature matches the report exactly.

**SCOPE DEVIATIONS:** None — the `FilterSpec` extension and its mechanical sweep were an explicit,
human-confirmed decision before any code was written, not a surprise found mid-slice.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- CR 701.23d (a mandatory-quantity search with no quality filter) — different shape, not built;
  `AbilityEffect::Search` is quality-search (701.23a/b) only.
- CR 701.23e (reveal the found card) — no reveal/visibility-transition concept exists in this engine
  yet; purely cosmetic today anyway since hidden info isn't enforced at the Lua boundary either.
- CR 701.23f (an effect that redirects "search" into "search the top N instead") — would need its
  own `EventFilter::Search` replacement shape; doesn't exist.
- CR 701.23i (simultaneous multiplayer search, APNAP) — single-player-effect shape only, same
  narrowing `Scry`/`Surveil`'s own 701.22c/701.25b gaps already took.
- Graveyard/Exile as a search destination — not modeled, `SearchDestination` is Hand/Battlefield
  only.
- No TOML schema for a Search card — same pre-existing `effect`-has-no-TOML-representation gap every
  `CardDefinition.effect`-carrying slice already notes.

## Phase 5: phase5/trigger-completeness-cleanup slice (backlog item 7, remaining threads)

Branch `phase5/trigger-completeness-cleanup`, off master. Closes backlog item 7 entirely — bundles
the three remaining named gaps into one slice, per explicit instruction rather than three separate
branches.

**What got implemented:**
- **`ConditionPrimitive::Scries`/`EachScries`/`Surveils`/`EachSurveils`** (CR 701.22d/701.25d) —
  matches `GameEventKind::CardsScried`/`CardsSurveiled` (the `Scries`/`Surveils` "your own" checked
  against `event.player == controller`), same "your" vs "each" split `DrawsCard`/`EachDrawsCard`
  already established. No plumbing changes needed — those events already reached `CollectTriggers`
  from the `scry-surveil` slice.
- **`ConditionPrimitive::DiesMatching`/`LeavesBattlefieldMatching(FilterSpec)`** (CR 700.4/603.10) —
  the filter-scoped sibling of the identity-scoped `Dies`/`LeavesBattlefield`: matches any qualifying
  `ZoneChange` where `filter.matches(&last_known_info, controller)`, `controller` being the
  *watching* ability's own controller (a separate, still-live permanent — Blood Artist-style), not
  the dying object's. Confirmed by reading `triggers.rs` before writing code: this needed **zero**
  plumbing changes — it's checked in `collect_triggers`'s ordinary live-source loop, never the CR
  603.10 departed/LKI fallback loop (which explicitly whitelists only the two identity-scoped
  variants). `FilterSpec`'s existing `FilterController::You`/`Any` gives "you control" vs "any
  player controls" for free — no separate `Each*` sibling needed. `Game.Conditions.DiesMatching`/
  `LeavesBattlefieldMatching(filter_table)` reuse `decode_filter_table`.
- **`InterveningIfSpec::Script(ScriptHandle)`** (CR 603.4) — the real design work. Traced the actual
  gap before building anything: `ScriptContext` already carries a live `&GameState`, and
  `invoke_condition` already receives it, but never passed it (or any state-query capability) into
  the Lua call. Fixed via the same "convert to a plain Lua table snapshot, not a live handle"
  convention `event_to_lua` already established: new `state_to_lua` (currently just per-player life
  totals, `{ life = { [player_id] = life, ... } }` — covers exactly the one existing
  `InterveningIfPrimitive` shape, `ControllerAtOrBelowLife`). `invoke_condition` gained this snapshot
  as a 4th Lua call arg (additive/non-breaking — Lua ignores extra args a function doesn't declare,
  confirmed every existing 3-arg condition script keeps working). New
  `ScriptRuntime::invoke_intervening_if(handle, state, source, controller)` (no event — CR 603.4's
  own doc comment already establishes this is "a pure state predicate, not event-shaped").
  `TriggeredAbilityBuilder::SetInterveningIf` widened to accept a function value too, same two-shape
  pattern `SetCondition`/`decode_condition_value` already use, inlined rather than a new standalone
  `decode_*` fn (`SetInterveningIf` is still the only consumer).
- 3 call sites gained a `Script` arm: `triggers.rs`'s two `InterveningIfSpec`-matching spots (live
  loop, departed loop) and `engine.rs`'s `ResolveStackTop` CR 608.2a recheck — each calls
  `invoke_intervening_if`, script error silently treated as "doesn't hold" (consistent conservative
  default in both places — a script error means the ability just doesn't do anything, same
  "no-error-surfacing-path-yet" stance every other script-invocation call site already takes).

**Struct fallout, anticipated in the pre-slice report:** `FilterSpec` isn't `Copy` (owns a
`Vec`/`String`), so `DiesMatching`/`LeavesBattlefieldMatching` carrying one drops `Copy` from
`ConditionPrimitive` and (in turn) `ConditionSpec`. Ripple, all mechanical: `script.rs`'s
`ConditionTag`/`OncePerTurnTag` wrapper structs dropped their own `Copy` derive; 3 `tag.0` copy-outs
from a borrowed `Ref` became `tag.0.clone()`; one `this.condition` move in the `Register()` builder
method became `.clone()`. `FilterSpec` and `ConditionPrimitive` both gained real `PartialEq`/`Eq`
derives alongside this (every field already supported them) rather than losing that capability —
not strictly required by the compiler, added since it was free and useful for test assertions.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 13 new tests in
`crates/engine/tests/phase5_trigger_completeness.rs`: (A) `Scries`/`Surveils` fire only for the
ability controller's own scry/surveil, `EachScries`/`EachSurveils` fire regardless; (B)
`DiesMatching` fires for a creature the watcher's controller controls dying, not for a noncreature
dying or an opponent's creature dying under a `You`-only filter; `LeavesBattlefieldMatching` fires
on exile too (not just death, mirroring `LeavesBattlefield` vs `Dies`); the `Game.Conditions.
DiesMatching` Lua binding decodes correctly; (C) a `Script` intervening-if behaves identically to
the existing `Primitive` shape across all three cases (false at trigger time never triggers; true
throughout resolves normally; true at trigger time but invalidated by a response spell fizzles at
resolution, CR 608.2a); `SetInterveningIf` accepts a function value and self-registers a real,
callable `Script` handle end-to-end via `run_card_script`. Full workspace run: 546 passed, 0 failed
(533 prior + 13 new).

**STRUCT DEVIATIONS:** None beyond the `Copy`-removal ripple the pre-slice report already
anticipated by name.

**INTERFACE DEVIATIONS:** None — `invoke_intervening_if`'s signature and `invoke_condition`'s widened
call (not signature — the Lua-facing call shape, not the Rust fn signature) both match the report.

**SCOPE DEVIATIONS:** None — every fallout site (the 3 `Copy`-ripple fixes, the 3 `InterveningIfSpec`
match sites) was named as anticipated in the pre-slice report before writing any code.

**UNRESOLVED (deferred, as scoped in the pre-slice requirements report):**
- Only a life-total snapshot this slice — hand size, board-state counts, "still on the battlefield"
  (all named in `InterveningIfPrimitive`'s own doc comment already) stay Rust-native-only or unbuilt.
- Script errors in `invoke_intervening_if`/a `Script` condition still silently resolve to
  false/no-match — pre-existing gap, not newly introduced.
- `ConditionSpec::PrimitiveOncePerTurn` still isn't offered for a `Script` condition (pre-existing,
  unrelated note already on that type).
- `AffectedFilter::Script`/`EffectPayload::Script` (backlog item 3a, continuous-effect/CDA scripting)
  — separate, untouched by this slice.
- The broader "hidden-info-safe `ScriptContext`" redesign (item 8) is **not** resolved — the
  life-total snapshot is public info, safe under the existing accepted-debt stance, but this doesn't
  touch hand/library visibility filtering. **Done as of `phase5/script-state-query`, below.**

## Phase 5: phase5/script-state-query slice (backlog item 8, hidden-information-safe `ScriptContext`)

Branch `phase5/script-state-query`, off master. Closes backlog item 8's remaining half.

**Correction to the pre-slice requirements report, found while reading the code before writing
any:** the report framed this as "replace `ScriptContext.state: &GameState` with a narrower type."
Traced every call site first — that field is never handed to Lua directly; every existing
`invoke_*` method already only used it to build a `state_to_lua` snapshot (a plain table) or ignore
it entirely. So the Rust-side struct didn't need narrowing — this engine is the omniscient rules
authority, full access on the Rust side isn't itself the leak. The real gap was two-fold: (1)
`state_to_lua` only ever built per-player life totals, nothing else; (2) `invoke_cda`,
`invoke_continuous_affects`, and `invoke_continuous_effect` didn't call it at all — a CDA or
scripted continuous effect had zero board-state access, only object/source/controller identity.
`ScriptContext`'s doc comment is updated to explain this instead of flagging a leak that traced out
false; `state_to_lua` is now the actual documented hidden-info boundary.

**What got implemented:**
- `state_to_lua` (script.rs) expanded from `{ life = {...} }` into a full public-safe snapshot:
  `life`, `hand_count`, `library_count` (per player), `battlefield`/`exile` (flat card-summary
  arrays, shared zones), `graveyard`/`command` (per-player card-summary arrays). Per CR 400.2/402.3:
  hand/library are hidden zones — only a count is ever exposed, never contents; every other zone is
  public — full contents exposed. No binding anywhere returns hand/library contents.
- `card_summary_to_lua`: one object's public-safe summary (id, name, base power/toughness netted
  against counters via `PTValue::net_with_counters` — CR 121.3, public info — is_creature,
  controller). Deliberately **base**, not layer-resolved `effective_characteristics` — see the next
  paragraph for why.
- `invoke_cda`, `invoke_continuous_affects`, `invoke_continuous_effect` all gained a `state:
  &GameState` parameter and now pass a `state_to_lua` snapshot as a trailing Lua call arg, same
  additive/non-breaking convention `invoke_condition`'s existing trailing arg already established.
  `layers.rs`'s `resolve_base_pt`/`resolve_primitive`/the two call sites feeding them threaded
  `state` through — no new plumbing needed, `state` was already in scope at every call site inside
  `effective_characteristics`/`applicable_effects_in_layer`.

**A real design hazard found and designed around, not guessed at:** the obvious version of
`card_summary_to_lua` — call `layers::effective_characteristics` for each battlefield object so a
script sees real current P/T — creates unbounded recursion. Building object B's snapshot entry would
invoke object B's own CDA/scripted-continuous-effect handles, which build *their own* snapshot right
back through the same `state_to_lua`, which walks the battlefield again, including object B. This
would recurse the instant any card on the board had a CDA or scripted continuous effect while
another script's snapshot was being built. Resolved by keeping card summaries to base
(`CardDefinition`-plus-counters) values only — no re-entrant Lua calls while building a snapshot.
A single-object *effective*-characteristics query is a real, separate follow-up, not folded in here.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` (`cargo test -p stackfish-engine`): 553
passed, 0 failed (546 prior + 7 new, in new `crates/engine/tests/phase5_script_state_query.rs`):
hand/library counts exposed with no `state.hand`/`state.library` content keys present at all; hand
count readable for an opponent, not just the querying controller (CR 402.3 symmetry); a CDA reading
`#state.graveyard[controller]` for its base power; card summaries include `+1/+1` counters (CR
121.3) without invoking the layer engine; a scripted continuous-effect "affects" predicate and
payload both gating on `state.life[...]`; exile/command zones also exposed as public. Full
workspace build (`cargo build --workspace`) also clean.

**STRUCT DEVIATIONS:** None — no new struct, `ScriptContext` unchanged (see the Correction above for
why).

**INTERFACE DEVIATIONS:** `invoke_cda`/`invoke_continuous_affects`/`invoke_continuous_effect` each
gained a `state: &GameState` parameter, and `layers.rs`'s private `resolve_base_pt`/
`resolve_primitive` did too — not called out as a signature change in the pre-slice report (which
assumed the fix lived entirely in `ScriptContext`'s type), but mechanical and expected once the real
gap was traced.

**SCOPE DEVIATIONS:** `crates/engine/tests/phase5_continuous_effects_script.rs`'s existing
unit-level tests for `invoke_continuous_affects`/`invoke_continuous_effect` needed a `&GameState`
argument added at each call site (a `base_state()` helper already existed in that file) — mechanical
fallout from the interface change above, not a new gap.

**UNRESOLVED (named during this slice, not silently narrowed):**
- Card summaries are base characteristics only, not layer-resolved effective ones (see the
  recursion-hazard paragraph above) — a single-object effective-characteristics query binding is a
  real, separate follow-up.
- The stack (CR 400.2, also public) isn't included in the snapshot — `StackObject` isn't
  `GameObject`-shaped (its characteristics come from `source`/`source_lki`), needs its own decode.
- Card summaries expose name/power/toughness/is_creature/controller only — no mana cost, no full
  type or keyword set (same `CardDefinition::colors()`/`ManaCost::colors()` still-`todo!()` gap
  `effective_characteristics` already carries forward, plus a not-yet-built type/keyword-set ->
  Lua-string-list serializer).
- Face-down permanents (`GameObject.face_down`) aren't special-cased in `card_summary_to_lua` — a
  face-down creature's real name/power/toughness would leak if one ever existed on the battlefield.
  Not fixed: `GameObject::new`'s constructor is still `todo!()` and no morph-style mechanic can put a
  face-down object into play yet, so this is currently unreachable dead code, not a live bug. Flagged
  by name so it isn't forgotten once face-down/morph lands.

## Phase 5: phase5/mana-add-choice slice (backlog item 13, mana system completeness)

Branch `phase5/mana-add-choice`, off master. Closes backlog item 13.

**Correction to the pre-slice requirements report, found while reading the code before writing any:**
the report's framing ("lands/abilities that add more than one mana... beyond the flat
`AbilityEffect::AddMana(Vec<ManaUnit>)` shape") was half stale. `AddMana` is already a `Vec` —
`commands.rs`'s apply loop (`for unit in &units { pool.add_unit(*unit); }`) already handles 2+ units
mechanically; it had just never been exercised by a test with more than one. The two real gaps were
bigger than "more than one": (1) **no `Game.Ops.AddMana` Lua binding existed at all** — grepped
`script.rs` for `AddMana` before writing anything and got zero hits; every existing
`phase5_activate_ability_mana.rs` test builds `AbilityEffect::AddMana(...)` directly as a Rust
fixture, meaning a card script could not define a mana ability, full stop. (2) No player-chosen-color
mechanism for "add one mana of any color" text.

**What got implemented:**
- `AbilityEffect::AddManaChoice { count: u32, choices: ColorSet }` (types/ability.rs) — CR 605's
  "add N mana, each any color among `choices`" shape. The color of each of the `count` mana isn't
  fixed on the ability; it's chosen at activation.
- `PlayerActionKind::ActivateAbility` gained `chosen_colors: Vec<Color>` (types/command.rs) — empty
  for every ability that isn't `AddManaChoice`, same "always-present, usually-empty" convention
  `targets`/`modes` already follow on `CastSpell`. Per CR 602.2b (activation follows the CR 601.2b-i
  casting process), the color choice is locked in as part of activation itself, not a mid-resolution
  pause — no coroutine/pause plumbing needed, since a mana ability never uses the stack (CR 605.3b)
  and so has no resolution step to pause during anyway.
- `commands.rs`'s `ActivateAbility` arm: the `AddMana` match arm widened to `AddMana(_) |
  AddManaChoice { .. }` so both share the same tap/summoning-sickness/no-target preamble, then
  branches internally to compute the actual `Vec<ManaUnit>` to add — `AddMana` uses its fixed units
  directly, `AddManaChoice` validates `chosen_colors.len() == count` and every entry is a member of
  `choices` (via new `ColorSet::contains_color`, mana.rs) before converting each to
  `ManaUnit::Colored`. A top-level check rejects any non-empty `chosen_colors` against an ability
  that isn't `AddManaChoice` (loud error, not a silently-dropped value).
- `Game.Ops.AddMana(letter, count?)` (script.rs) — `letter` is a single WUBRG/`"C"` (colorless)
  symbol via new `parse_mana_unit_letter`, `count` (default 1) repeats that fixed unit. Closes the
  "can't script this at all" gap directly.
- `Game.Ops.AddManaChoice(count, colors)` — `colors` is a WUBRG letter string decoded via the
  existing `parse_color_string` (same vocabulary `Game.Effects.AddColor` already uses).
- `engine.rs`'s `ResolveStackTop` defensive no-op arm (`Some(AbilityEffect::AddMana(_)) => {}`,
  "should never reach the stack") widened to include `AddManaChoice` too, same reasoning.
- `mana::parse_color_letter` (previously crate-private, used internally by `ManaCost::parse`) made
  `pub` so the scenario crate's TOML `chosen_colors: Vec<String>` field (on `ActionSpec::
  ActivateAbility`, `#[serde(default)]`) can reuse the same WUBRG vocabulary instead of a second
  parser.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 562 passed, 0 failed (553 prior + 9 new, in
new `crates/engine/tests/phase5_mana_add_choice.rs`): a fixed `AddMana` ability with 2 units actually
adds both (closes the untested "more than one" gap directly); `AddManaChoice` adds the chosen colors
and taps the source; wrong `chosen_colors` count is rejected with no partial mutation; a color outside
the allowed `choices` set is rejected; `chosen_colors` supplied against a plain `AddMana` ability is
rejected; `Game.Ops.AddMana`/`Game.Ops.AddManaChoice` decode correctly (fixed unit with default count,
repeated colorless, unknown letter rejected, count+colors for the choice shape). Full workspace build
also clean.

**STRUCT DEVIATIONS:** `PlayerActionKind::ActivateAbility` gaining `chosen_colors` was anticipated in
the pre-slice report. `ColorSet::contains_color` (mana.rs) and `parse_mana_unit_letter` (script.rs)
were not named in the report but are small, expected mechanical additions once the real design was
being written.

**INTERFACE DEVIATIONS:** None beyond the struct field above — no existing function signature changed
shape, only `ActivateAbility`'s literal fields.

**SCOPE DEVIATIONS:** Every existing `ActivateAbility { ... }` struct-literal call site across the
engine's test suite (16 files, ~19 occurrences) and the scenario crate (`ActionSpec::ActivateAbility`'s
`describe()` and TOML-to-engine conversion) needed `chosen_colors` added — mechanical fallout from the
new field, anticipated in kind (though not in exact count) by the pre-slice report's own framing of
this as a struct change.

**UNRESOLVED (named during this slice, not silently narrowed):**
- `Game.Ops.AddMana(letter, count?)` only accepts one repeated fixed symbol per call — no
  mixed-color-in-one-call shape ("add one white and one blue"). Real cards with that exact text are
  rare enough to defer rather than guess a table-based calling convention now.
- No `SnowColored`/`SnowColorless` letter in `parse_mana_unit_letter` — snow-producing lands are rare
  enough to defer rather than invent an unvalidated letter convention for them.
- Restricted/tagged mana (CR 106.4-adjacent errata target: "spend this mana only to cast a creature
  spell," Command Tower-style, filter mana) — `ManaPool`/`ManaUnit` don't model a spending
  restriction at all. Separate, larger follow-up, not attempted here.
- Mana abilities that also trigger something else ("Add {C}. When you do, ...") — no hook exists for
  a paired triggered ability off a mana ability's own activation/resolution (CR 605.1b's triggered-
  mana-ability case is untouched).
- No replacement/prevention modeling for `ManaAdded` — pre-existing gap, same one `GainLife` already
  carries, not fixed here.

## Phase 5: phase5/multiplayer-defending-player slice

Branch `phase5/multiplayer-defending-player`, off master. Closes backlog item 11a.

Scope, narrowed during the pre-slice research pass over `combat.rs`/`commands.rs`/`engine.rs`
(recorded in the backlog entry itself before writing code): `DeclareBlockers` (`commands.rs`) was
the one real hardcoded spot — it picked "the first non-active player in `turn_order`" as *the*
single defending player, ignoring CR 508.5a (multiplayer: the defending player is individually
determined per attacker, not one engine-wide value). Everything else the backlog item's own text
worried might also be broken turned out fine on inspection: `CombatState`/`DeclareAttackers` were
already N-safe (each attacker independently names its own `Target`), and `AssignCombatDamage`'s
damage-dealing already routes each attacker's damage to that attacker's own recorded `Target`, not
a hardcoded defender — the doc's "damage-assignment resolved per the attacker's own declared
defender" phrasing was already true there, confirmed by reading the code rather than assumed.

**What got implemented:**
- `CombatState.blockers_declared: bool` (`types/combat.rs`) replaced with `declared_defenders:
  Vec<PlayerId>` — CR 509.1 is a turn-based action performed once per defending player
  (CR 101.4 APNAP order among however many defending players exist this combat), not once total.
  Tracks who has already submitted, both to reject a repeat submission and to distinguish
  "declared zero blockers" from "hasn't declared yet."
- `commands.rs`'s `DeclareBlockers` arm: the hardcoded `defending_player` lookup is gone. Legality
  is now "does any attacker in `combat.attackers` target `command.actor` (directly via
  `Target::Player`, or via `Target::Object` where the object's controller is `command.actor` —
  CR 506.4d's planeswalker/battle case)?" — same check the per-block `attacking_this_defender`
  validation already ran, just promoted to the entry gate instead of a single precomputed player.
  `combat.blocks` now `.extend()`s across submissions instead of overwriting, so a second defending
  player's blocks land alongside the first's rather than replacing them.
- All per-block validation (flying/reach, menace count, uncontrolled/tapped blocker,
  attacker-isn't-attacking-this-defender) needed no changes — it already compared against
  `command.actor`, not the broken hardcoded variable, so it was correct for any player count once
  the entry gate was fixed.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 565 passed, 0 failed (562 prior + 3 new,
in `crates/engine/tests/phase5_combat_declarations.rs`, which gained a `combat_state_3p()` fixture
— the repo's first 3-player test fixture): two different defending players each declare their own
blocks in sequence and both end up in `combat.blocks` (not the second overwriting the first); a
player nobody is attacking is rejected from submitting `DeclareBlockers`; a 3-player case where the
attacker targets a planeswalker controlled by the *second* nonactive player in `turn_order` proves
the fix isn't just "pick a different fixed player" — the first nonactive player is correctly
rejected and the planeswalker's actual controller is correctly accepted. All 562 prior tests
unaffected, including every existing 2-player combat-damage fixture (`blockers_declared: true`
mechanically became `declared_defenders: vec![p1]` across ~30 call sites, `blockers_declared:
false`/no-attacker cases became `Vec::new()`).

**STRUCT DEVIATIONS:** None beyond what the pre-slice report scoped — `CombatState`'s field rename
was named directly in the report as the fix.

**INTERFACE DEVIATIONS:** None. `PlayerActionKind::DeclareBlockers`'s shape (`blocks: Vec<(ObjectId,
ObjectId)>`) didn't need to change — a defending player's blocks are already disjoint from every
other defending player's (a blocker must be controlled by the actor, and each attacker has exactly
one defending player), so the flat accumulated list needed no restructuring into a per-player map.

**SCOPE DEVIATIONS:** None. The backlog entry's own narrowing (AssignCombatDamage/DeclareAttackers
untouched) held up exactly as scoped once the code was actually read.

**UNRESOLVED (pre-existing, not this slice's to fix):**
- Turn-based actions still don't block priority from opening before they're performed
  (`engine.rs`'s `submit_action` doc comment already names this as a whole-engine simplification,
  not specific to combat) — nothing forces every applicable defending player to actually submit
  `DeclareBlockers` before the game moves on; a defending player who never declares simply never
  blocks, same pre-existing gap the 2-player case already had.
- Block restrictions (CR 509.1b, "effects that say a creature can't block") — same
  continuous-effects query gap `is_sorcery_speed` already carries, untouched.
- Backlog items 11b (player elimination), 11c/11d/11e (Commander marking/tax/damage) — separate
  slices, not attempted here.

## Phase 5: phase5/player-elimination slice

Branch `phase5/player-elimination`, off master. Closes backlog item 11b.

`sba::apply_player_loss` already flipped `PlayerState.eliminated` (a prior slice); checked before
writing code and confirmed, again, zero downstream consumers. Three independent gaps, same slice
per the backlog grouping:

**What got implemented:**
- `StepStatus::GameOver { winner: Option<PlayerId> }` (already declared, zero call sites, checked)
  now actually gets constructed — new `ProcessState::GameOver` variant (engine.rs), terminal: once
  reached, `step()` keeps returning it without mutating anything further, and `submit_action`
  already rejects new actions outside `PriorityWindow` so nothing more can be submitted either.
  `Engine::game_over_check` (called from both `finish_sba_loop` and `finish_cleanup_sba_check`,
  the two places an SBA pass's events actually commit) counts non-eliminated players in
  `turn_order`: 1 remaining constructs `GameOver { winner: Some(that player) }` (CR 104.2a), 0
  remaining (simultaneous elimination in one SBA pass) constructs `GameOver { winner: None }` (CR
  104.4a draw). Guarded by `turn_order.len() < 2` — a one-player test/scenario fixture (common
  throughout this crate, unrelated to this feature) reaching 0 life would otherwise read as "game
  over" after its very first SBA pass; CR 104.2a presupposes an opponent to win against.
- CR 800.4b (eliminated player's turns/priority skipped): `commands::next_active_player` (new,
  commands.rs) walks `turn_order` from a given player, skipping anyone `.eliminated`, used by both
  `PassPriority`'s handoff (commands.rs) and the Cleanup-step active-player rotation (engine.rs) —
  previously both did a blind `(idx + 1) % turn_order.len()`. `commands::non_eliminated_players`
  (new) replaces the "everyone passed" `consecutive_passes >= turn_order.len()` threshold too — an
  eliminated player is never asked to pass, so the old threshold could never be reached again once
  anyone was eliminated.
- CR 800.4a (eliminated player's objects leave the game): `sba::remove_players_objects` (new),
  called from `apply_player_loss` right after `.eliminated` flips. Per-owner zones
  (library/hand/graveyard/command) are drained wholesale; shared zones (battlefield/exile) are
  filtered by `GameObject.owner`; the stack (not a `Zone`) is filtered by `StackObject.controller`
  (see UNRESOLVED — not a perfect proxy for "owner"). Each removed object is purged the same way
  `commands::retire_object` already purges a normal death (abilities/continuous-effects sourced
  from it) — reused directly, not reimplemented. `retire_object`'s returned `Vec<RegisteredAbility>`
  (what a normal death feeds into `CollectTriggers` for a Dies/LeavesBattlefield trigger) is
  deliberately discarded here: the departing object's own triggered abilities are controlled by the
  very player who just left, and CR 800.4d says a triggered ability controlled by a player who's
  left the game never goes on the stack — so this is a correct read of the rule, not a shortcut.
  Same `turn_order.len() >= 2` guard as `game_over_check`, for the identical reason (a one-player
  fixture has nothing to "leave into").

**Regression found and fixed during verification** (not a design change, a real bug caught by the
existing suite): the object-purge, before the `turn_order.len() >= 2` guard was added, wiped a
solo scenario fixture's own hand/battlefield the instant it hit an empty-library draw on turn 1 (a
pre-existing, previously-harmless quirk: `Engine`'s two-player draw-skip only checks
`turn_order.len() == 2`, so a one-player fixture always draws on its first turn - with no library
seeded, that's every such fixture, immediately marking the fixture's only player `eliminated` -
harmless before this slice since nothing read the flag, actively destructive once purging was
added). Caught by `phase5_cast_spell_payment.toml` failing for real (not just an assertion mismatch
- the cast card had vanished from the engine's state entirely). Fixed by gating both new mutating
behaviors (`game_over_check`, `remove_players_objects`) behind "at least 2 players in the game."

**Pre-existing tests updated to the newly-correct behavior** (not broken by this slice - they were
asserting the exact gap this slice closes):
- `phase5_beginning_phase.rs`'s `draw_from_empty_library_is_noop` (renamed
  `draw_from_empty_library_is_a_noop_draw_not_a_panic`) — a two-player game where p0 draws from an
  empty library now correctly ends the game (p1 wins) instead of silently continuing.
- `phase5_sba_basics.rs`'s infect/poison-elimination test — now asserts `GameOver { winner: p0 }`
  instead of continuing priority past p1's elimination.
- `phase5_script_ops_drawcards.rs`'s `drawing_more_than_the_library_holds_stops_without_crashing` —
  same empty-library-mid-game shape as the beginning-phase fix.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 573 passed, 0 failed (565 prior + 8 new, in
new `crates/engine/tests/phase5_player_elimination.rs`): two-player zero-life ends the game and
`step()`/`submit_action` stay frozen/rejecting afterward; simultaneous zero-life for both remaining
players is a draw (`winner: None`); a one-player fixture at zero life never triggers `GameOver`
(the SBA itself still fires, unrelated); a three-player `PassPriority` skips an already-eliminated
middle player; the Cleanup-step active-player rotation does the same, landing back on the one
other survivor; an eliminated player's battlefield permanent is gone from `state.objects` and the
battlefield zone, its triggered ability purged, with zero abilities fed toward
`CollectTriggers` (CR 800.4d); an eliminated player's hand/library are fully drained while a
non-eliminated player's own zones stay untouched. All 565 prior tests unaffected once the two
pre-existing-gap tests above were updated and the solo-fixture regression was fixed. Also confirmed
via the real CLI: `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/
phase5_cast_spell_payment.toml` (the scenario the regression broke) — 6/6 actions ran, 5/5
assertions passed.

**STRUCT DEVIATIONS:** `ProcessState::GameOver` (engine.rs) wasn't explicitly named in the pre-slice
report but was the obvious mechanical counterpart to constructing the already-declared
`StepStatus::GameOver` — anticipated in kind by the report's "hook is `finish_sba_loop`" framing.

**INTERFACE DEVIATIONS:** None beyond the two new `pub(crate)` helpers (`next_active_player`,
`non_eliminated_players`) named directly in the report's own plan.

**SCOPE DEVIATIONS:**
- The `turn_order.len() < 2` guard (both `game_over_check` and `remove_players_objects`) wasn't
  anticipated in the pre-slice report — found via the regression above, not guessed at in advance.
- Three pre-existing tests needed updating to the new correct behavior (listed above) - the report
  anticipated this in general ("regression: existing 2-player tests... unaffected") but didn't (and
  couldn't, without running them) name which ones would actually need a behavior-assertion change
  versus truly regress.

**UNRESOLVED (named during this slice, not silently narrowed):**
- `replacement_effects`/`cost_modifiers` sourced from a purged object are never purged — checked,
  this is true of *every* object departure in this engine already (not even `retire_object` does
  it for a normal death), not introduced or widened by this slice.
- Stack-object removal is scoped by `StackObject.controller`, not a true "owner" (CR 800.4a) — no
  `owner` field exists on `StackObject` separately, and no control-changing effect exists yet in
  this engine to make the distinction observable. Pragmatic approximation, flagged not fixed.
- CR 800.4c (an object exiled when a control-granting effect from a departed player ends) —
  unreachable, no control-changing effects exist in this engine yet.
- Teams/Emperor multiplayer variants (CR 104.2c/d, 104.3g/i) — explicitly out of scope, this engine
  has no team concept.
- Backlog items 11c/11d/11e (Commander marking/tax/damage) — separate slices, not attempted here.

## Phase 5: phase5/commander-marking-and-casting slice

Branch `phase5/commander-marking-and-casting`, off master. Closes backlog item 11c.

**Scope decision made before writing code, per the pre-slice report:** the literal backlog text
("marking + casting from the command zone") would only ever exercise a commander once per game —
without some return-to-command-zone path, a commander that dies just sits in the graveyard forever,
leaving 11d's tax counter and 11e's repeated-recast scenarios untestable through real gameplay
(the same "inert until the next slice" gap 11e is itself closing for backlog item 10). Confirmed
with the human before starting: CR 903.9a (owner may return a commander from graveyard/exile to the
command zone — a genuine SBA-driven player choice) is included in this slice; CR 903.9b (the rarer
hand/library-redirect replacement effect) stays deferred.

**What got implemented:**
- `GameObject.is_commander: bool` (types/object.rs) — CR 903.3: an attribute of the specific card
  object, not its `CardDefinition` (a definition can be shared by non-commander copies). Every real
  zone-change site in this crate builds the successor object via an explicit struct literal or a
  `.clone()` of the original (`GameObject::new` itself is dead code, `todo!()`, never called) — so
  adding this field made the compiler enumerate every construction site as a `E0063` error until
  each one explicitly chose `is_commander: object.is_commander` (zone-move sites, via `.clone()` —
  automatic, no site touched) vs `is_commander: false` (freshly-minted tokens). Self-auditing, not
  guessed at.
- `commands.rs`'s `CastSpell` zone-permission check gained a third legal source alongside
  `Hand`/`AlternativeCost::Replacement`: `object.is_commander && object.zone == ZoneKind::Command &&
  object.owner == command.actor` (CR 903.8's "owner," not controller — matches the rule text). No
  alternative cost involved — normal mana cost, just a different source zone (the tax itself is
  11d's job).
- `PlayerConfig.commanders: Vec<CardDefinition>` (types/config.rs) — a `Vec`, not `Option`
  (DESIGN.md: "Command zone holds commander(s)," plural, for Partner). `Engine::new_game` mints
  these with `is_commander: true` into a new per-player `ZoneKind::Command` zone, right after that
  player's library (same id-allocation order the scenario crate's `card_ids` tracking depends on
  matching exactly — see below).
- CR 903.9a: `sba::find_commander_return_candidate` (mirrors `find_legend_rule_group`'s exact
  shape) finds the first (by `ObjectId`, for determinism) commander sitting in a graveyard or exile
  not yet in the new `GameState.commander_return_offered: HashSet<ObjectId>` — a one-time-per-arrival
  window (CR 903.9a's "since state-based actions were last checked"), so a *declined* offer isn't
  re-asked every subsequent pass. New `SbaOutcome::NeedsCommanderReturnChoice` /
  `ProcessState::AwaitingCommanderReturnChoice` / `StepStatus::AwaitingCommanderReturnChoice` /
  `Engine::submit_commander_return_choice(returned: bool)` — same pause/resume shape
  `find_legend_rule_group`/`AwaitingLegendRuleChoice`/`submit_legend_rule_choice` already
  established. Accepting calls new `sba::move_commander_to_command_zone` (graveyard/exile → command,
  CR 400.7 new `ObjectId`, `is_commander` preserved via clone) — **deliberately NOT routed through
  CR 614-616 replacement checking**, unlike `commit_death`/legend rule's own losers: no card in this
  engine produces a competing replacement effect on this specific event yet, and 903.9b (the natural
  vehicle for one) is out of scope this slice. Flagged below, not silently done.
- Scenario TOML: `[[setup]]` gained `is_commander` (optional bool, default `false`) — `"command"`
  already worked as a `[[setup]]` zone destination before this slice, just never paired with the
  flag. `[[game.players]]` gained `commanders` (optional `Vec<String>` of card keys, mirroring
  `library`) for the real `new_game` bootstrap path — minted after that player's `library` in the
  scenario crate's own id-tracking, matching `Engine::new_game`'s allocation order exactly (both
  loops share the same `next_id` counter, in the same sequence).

**Regression avoided, not introduced:** none this time — the whole plumbing sweep (is_commander
across ~30 test fixtures via a bulk `sed` insertion, `commander_return_offered` the same way,
`PlayerConfig.commanders` at 4 call sites) came back to a clean `cargo test --workspace` on the
first full run after the struct-literal churn, before any new commander-specific logic was added.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 583 passed, 0 failed (573 prior + 10 new,
in new `crates/engine/tests/phase5_commander.rs`): a commander cast from the command zone keeps
`is_commander` through both the Command→Stack and Stack→Battlefield CR 400.7 remaps; a non-commander
card sitting in the command zone is rejected (the flag gates permission, not the zone alone); a
commander owned by a different player is rejected; `Engine::new_game` with `PlayerConfig.commanders`
populated places them in the command zone (not the library) with `is_commander: true`, and a
commander-less player gets an empty (but present) command zone; a commander freshly in the graveyard
pauses for the CR 903.9a choice; declining leaves it in the graveyard and marks it so a later pass
doesn't re-ask; accepting moves it to the command zone under a new id with `is_commander` intact; a
non-commander creature dying never pauses at all; `submit_commander_return_choice` outside the pause
state is rejected. All 573 prior tests unaffected. Confirmed via the real CLI too:
`cargo run -p stackfish-cli -- crates/engine/tests/scenarios/phase5_commander.toml` — 6/6 actions
ran, 3/3 assertions passed (a command-zone commander cast for real through the TOML pipeline,
resolving to the battlefield).

**STRUCT DEVIATIONS:** None beyond what the pre-slice report scoped — `GameObject.is_commander`,
`PlayerConfig.commanders`, and `GameState.commander_return_offered` were all named directly.

**INTERFACE DEVIATIONS:** `Engine::submit_commander_return_choice`/`CommanderReturnChoiceError`
weren't named by field/method name in the report, but were the obvious mechanical counterpart to
the report's own "same pause/resume shape as legend rule" framing for CR 903.9a.

**SCOPE DEVIATIONS:** None beyond the CR 903.9a inclusion itself, which was confirmed with the
human as a scope question before implementation started (see above), not decided unilaterally
mid-slice.

**UNRESOLVED (named during this slice, not silently narrowed):**
- CR 903.9b (a commander redirected from hand/library into the command zone instead — a replacement
  effect) — deferred, rarer interaction than 903.9a, separate follow-up.
- `move_commander_to_command_zone`'s zone move skips CR 614-616 replacement checking entirely (see
  above) — a real gap once a card exists that would want to intercept this specific event.
- CR 903.3a (a card granting "can be your commander" via its own text) — `commanders` is chosen at
  `GameConfig`/scenario-setup time regardless of what the card's own text says, same "engine trusts
  the setup" stance DESIGN.md already takes elsewhere.
- CR 903.4/903.5 (color identity, deck-construction legality) — DESIGN.md already explicitly scopes
  this out.
- Melded/merged/partner commander edge cases (CR 903.3b/c) — no meld/merge mechanic exists in this
  engine.
- No scenario TOML action exists yet for `submit_commander_return_choice` (or, pre-existing gap,
  `submit_legend_rule_choice` either) — drive both directly via `Engine` methods in a Rust test.
- Backlog items 11d (commander tax) and 11e (commander damage production) — the two slices this one
  was explicitly built to unblock, not attempted here.

## Phase 5: phase5/commander-tax slice

Branch `phase5/commander-tax`, off master. Closes backlog item 11d.

**Correction to this item's original backlog text, found while scoping:** the tax itself is
CR 903.8, not 903.9 (903.9 is the return-to-command-zone rule 11c already closed) — the backlog
text had cited the wrong sub-rule.

**What got implemented:**
- `PlayerState.commander_casts: HashMap<String, u32>` (types/player.rs) — how many times this
  player has cast each commander from the command zone this game, keyed by `CardDefinition.name`
  rather than `ObjectId`. `ObjectId` doesn't work as the key: CR 400.7 mints a fresh id on every
  cast *and* every 11c/903.9a return, so nothing about an id survives a cast → death → return →
  recast cycle the way a card's name does. Every real `PlayerState` struct literal in the crate
  (~68 sites: `src/` construction sites plus every test fixture's own hand-built one) needed the
  new field — same bulk-`sed`-after-a-shared-last-field sweep 11c used for `is_commander`, anchored
  on each literal's existing `hand_kept: false,` line (the field immediately before
  `commander_casts` in the struct) rather than guessed at per-file.
- `commands.rs`'s `CastSpell`, `None`-alternative-cost branch (i.e. the plain CR 601.2f mana-cost
  path, which is the only path a 903.8 command-zone cast ever takes — an announced alternative cost
  is a different, mutually exclusive branch entirely): when `cast_from_command_zone` (11c's own
  flag), looks up this player's `commander_casts` count for `object.definition.name` and applies
  `ManaCost::apply_cost_delta(2 * prior_casts)` — the same generic-only, clamped-at-zero helper
  static cost-modifier effects (Thalia, etc) already use, applied *after* `apply_cost_modifiers` so
  a static tax effect and CR 903.8 stack rather than one clobbering the other. The counter itself
  increments right where the cast actually commits (the zone-move to the stack, a few lines below
  cost computation) — not merely attempted, matching CR 903.8's "cast," not "announced and later
  abandoned."
- CR 702.124d (Partner): falls out for free from keying by name — two differently-named commanders
  never collide in the same `HashMap`, no special-casing needed.

**Regression avoided, not introduced:** none — the `commander_casts` field sweep across every
`PlayerState` literal came back to a clean `cargo test --workspace` on the first full run after the
struct-literal churn, before the tax computation itself was added, same "plumbing first, logic
second" order 11c's `is_commander` sweep followed.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 589 passed, 0 failed (583 prior + 6 new, in
new `crates/engine/tests/phase5_commander_tax.rs`): a commander's first cast from the command zone
carries no tax at all; a second cast (after a manually-driven return to the command zone, same
CR 400.7 shape 11c's own 903.9a acceptance path produces) costs the base amount plus {2}; a third
costs base plus {4}, with `commander_casts` itself reading back `3` afterward; two same-player
Partner commanders keep fully independent counters — recasting one twice doesn't tax the other's
first cast at all; casting the same-named card from hand (not `is_commander`, not the command zone)
is never taxed, confirming the check is gated on the 903.8 path specifically, not just a name match;
a card with a real printed `{2}{G}` cost still carries its colored pip after tax is computed,
confirming the tax adds to the existing generic component rather than replacing the whole cost with
a bare `{2}`. All 583 prior tests unaffected. Confirmed via the real CLI too: `cargo run -p
stackfish-cli -- crates/engine/tests/scenarios/phase5_commander_tax.toml` — 6/6 actions ran, 2/2
assertions passed (a commander with a real `{1}` printed cost cast from the command zone through the
full TOML → Engine → CLI pipeline, mana pool debited by exactly 1, no phantom tax on an untaxed
first cast). The scenario only covers the untaxed baseline through the real CLI pipeline, not a
taxed repeat cast — see UNRESOLVED below.

**STRUCT DEVIATIONS:** None beyond what the pre-slice report scoped — `PlayerState.commander_casts`
was named directly.

**INTERFACE DEVIATIONS:** None — no new `Engine`/`commands` public functions, only a lookup-and-add
inline in `CastSpell`'s existing cost computation.

**SCOPE DEVIATIONS:** None — the ~68-site `PlayerState` literal sweep was anticipated in the
pre-slice report (named as an expected consequence of the struct change, not discovered mid-slice).

**UNRESOLVED (named during this slice, not silently narrowed):**
- `Engine::unwind_payment` (CR 601.6) doesn't roll back a `commander_casts` increment when a
  commander cast is abandoned mid-payment — the counter increments at zone-move time, before
  payment is even requested, and `unwind_payment` has no matching decrement. Compounding
  pre-existing gap: `unwind_payment` already hardcodes returning *any* unwound spell to
  `ZoneKind::Hand` regardless of its real origin zone, so an unwound commander cast already loses
  its command-zone recastability independent of this slice's own gap. Flagged, not fixed — a real
  CR 601.6 fix for commander casts specifically would need to know the origin zone, not just always
  target hand, which is a wider fix than this tax-only slice's scope.
- No scenario TOML action exists for CR 903.9a's return-to-command-zone choice (11c's own
  pre-existing UNRESOLVED item, still open) — so the taxed-repeat-cast cases can only be driven
  through a Rust test (which manually replicates the return by hand, same shape
  `phase5_commander.rs`'s own fixtures already use), not through the real TOML → CLI pipeline like
  the untaxed baseline is.
- Backlog item 11e (commander damage production) — the other slice 11c was explicitly built to
  unblock, not attempted here either.

## Phase 5: phase5/commander-damage slice

Branch `phase5/commander-damage`, off master. Closes backlog item 11e — the last item in the
11a-11e Commander sub-sequence.

**Two scope decisions confirmed with the human before writing code (per the pre-slice report):**
1. `GameEventKind::DamageDealt` needed a new `is_combat_damage: bool` field, since nothing
   previously distinguished CR 510 combat damage from a spell/ability's own damage effect, and
   CR 903.10a only counts the former. Confirmed: add it.
2. `PlayerState.commander_damage_taken`'s existing `ObjectId` key (from item 10's slice, predating
   11c's commander-return mechanism) has a real gap — a recast commander gets a new id (CR 400.7)
   and its damage count resets to 0 instead of accumulating against "the same commander." Confirmed:
   leave it as-is (matches the backlog's own "already real, just wire up the write side" framing;
   the tested `sba.rs`/`commander_damage_from` surface stays untouched), flag the gap rather than
   widen the field to name-keying like `commander_casts` did for the identical problem in 11d.

**What got implemented:**
- `GameEventKind::DamageDealt` gained `is_combat_damage: bool` (types/event.rs). `commands.rs`'s
  `AssignCombatDamage` sets it `true` at all 4 event-construction sites (unblocked, single-blocker,
  multi-blocker split, blocker-strikes-back) - the only real combat-damage producer. `engine.rs`'s
  script bridge for `AbilityEffect::Damage` (a spell/ability's own damage effect, resolved through
  the same `ReplacementCheck`→`CommitEvents` pipeline combat damage uses) sets it `false` - never
  combat, even from a commander source. `effect.rs`'s `ReplacementEffect::apply` (the
  `PreventDamage`/`RedirectDamage` rewrite arms) now threads the field through unchanged rather than
  losing it, so a prevented/redirected combat-damage event is still recognized as combat damage once
  it reaches `apply_damage_event`. Every literal `DamageDealt` construction elsewhere (~30 sites,
  entirely lower-level unit tests in `replacement.rs`/`ability.rs`/`effect.rs`/`engine.rs`'s own
  `#[cfg(test)]` modules, none of them exercising real combat) got `is_combat_damage: false`; every
  bare pattern match got a trailing `..` instead, since those only care about `source`/`target`/
  `amount`.
- `commands::apply_damage_event` gained an `is_combat_damage: bool` parameter. In the `Target::
  Player` arm, when `is_combat_damage && source.is_commander`, accumulates `amount` into
  `commander_damage_taken` (find-or-insert by `source` `ObjectId`) - runs unconditionally before the
  existing life-loss/poison branching, not nested inside either arm, since CR 903.10a's count is a
  parallel tracking system, not a redirect of the same number (an infect commander's damage still
  counts even though the player takes poison counters instead of life loss).
- New `AssertionSpec::CommanderDamage`/`assert_commander_damage` (crates/scenario/src/lib.rs),
  resolving `source_card` through the same `resolve_card`/`card_ids` CR 400.7-remap-following
  lookup `assert_object_zone`/`assert_tapped` already use, then reading
  `PlayerState::commander_damage_from` (item 10's own pre-existing query method) - no scenario
  assertion existed for this field at all before this slice.

**Regression avoided, not introduced:** none - the `is_combat_damage` field sweep (production sites
first, then the ~30 lower-level test-literal sites) came back to a clean `cargo test --workspace` on
the first full run after the struct-literal churn, before `apply_damage_event`'s own commander-damage
write was added.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 595 passed, 0 failed (589 prior + 6 new, in
new `crates/engine/tests/phase5_commander_damage.rs`): an unblocked commander attacker's combat
damage accumulates into the defending player's `commander_damage_taken` (life loss still happens
normally alongside it); two different commanders (Partner-style) attacking the same player track
fully independently; a non-commander creature's combat damage never touches
`commander_damage_taken` at all, even though life loss still happens; an Infect commander's combat
damage still accumulates the full commander-damage count even though the player takes poison
counters instead of life loss; a commander's own activated-ability damage effect (not combat) never
counts, confirmed via the same `{T}: deal damage` stack-resolution shape
`phase5_script_ops_damage.rs`'s lifelink test already established; 21+ cumulative commander combat
damage in one hit actually ends the game now (`StepStatus::GameOver`), integrating with item 10's
previously-inert SBA check for the first time. All 589 prior tests unaffected. Confirmed via the
real CLI too: `cargo run -p stackfish-cli -- crates/engine/tests/scenarios/
phase5_commander_damage.toml` - 15/15 actions ran, 2/2 assertions passed (a commander attacks
unblocked through the full TOML → Engine → CLI pipeline, both life loss and the new
`assert_commander_damage` assertion confirmed).

**STRUCT DEVIATIONS:** `AssertionSpec::CommanderDamage`/`AssertionStepSpec::CommanderDamage`
weren't named in the pre-slice report (which only discussed the engine-side struct changes) - added
once writing the CLI-scenario test surfaced that no way existed to check this field from TOML at
all, same "found while writing the test, not guessed at" shape other slices' own scope additions
have taken.

**INTERFACE DEVIATIONS:** `commands::apply_damage_event` and `commands::AssignCombatDamage`'s event
constructors both gained the `is_combat_damage` parameter/field exactly as the pre-slice report's
decision-1 framing anticipated - no deviation beyond what was already flagged for confirmation.

**SCOPE DEVIATIONS:** None beyond the `AssertionSpec` addition noted above (STRUCT DEVIATIONS) -
both scope questions (the event field, and leaving `commander_damage_taken`'s key alone) were
confirmed with the human before implementation started, not decided unilaterally mid-slice.

**UNRESOLVED (named during this slice, not silently narrowed):**
- `PlayerState.commander_damage_taken` stays keyed by `ObjectId` (decision 2 above) - a commander
  that dies and returns via CR 903.9a gets a new id, and its damage count resets to 0 instead of
  continuing to accumulate against "the same commander" the way CR 903.10a's own text means it.
  Deliberately left as-is per the confirmed scope decision, not silently narrowed - a real follow-up
  fix would mirror `commander_casts`' name-keying, touching the same tested `sba.rs`/
  `commander_damage_from` surface this slice deliberately avoided widening.
- Planeswalker/battle damage (CR 120.3c/d) still isn't modeled (`apply_damage_event`'s own
  pre-existing TODO, untouched by this slice) - irrelevant to 903.10a specifically (that rule is
  player-damage-only) but worth naming since a commander could in principle also deal combat damage
  to a planeswalker/battle, which this engine still can't represent at all.
- No TOML action exists for CR 903.9a's return-to-command-zone choice (11c's own open gap, still
  open) - a scenario proving the recast-reset gap above through the real CLI pipeline isn't possible
  yet either.
- This closes the entire 11a-11e Commander sub-sequence (backlog item 11). Only the item's own
  explicit non-goal remains: color identity/deck-construction legality, already scoped out by
  DESIGN.md.

## Phase 5: phase5/color-identity slice

Branch `phase5/color-identity`, off master. Closes backlog item 17 - DESIGN.md's own "explicitly
out of scope unless revisited" open question, revisited now that the rest of the Commander
sub-sequence (item 11) is closed.

**Scope decision confirmed with the human before writing code:** whether CR 903.5c/d deck legality
should be automatically enforced by `Engine::new_game` (changing its signature to
`Result<Engine, _>`, an interface break touching every existing call site) or exposed as a pure
query a caller checks itself. Confirmed: pure query only - matches the precedent the
commander-marking slice (11c) already set for CR 903.3a ("can be your commander" card text also
isn't engine-enforced, chosen at `GameConfig`/scenario-setup time regardless of what the card's own
text says).

**What got implemented:**
- Filled in four pre-existing `todo!()` stubs that turned out to be entirely dead code (zero
  callers anywhere) and exact prerequisites for this slice: `AtomicManaSymbol::colors`/
  `ManaSymbol::colors`/`ManaCost::colors` (mana.rs, CR 202.2/107.4e-f) and `CardDefinition::colors`
  (card.rs, CR 105.2a) - each implemented exactly per its own pre-existing doc comment, not
  redesigned.
- New `crates/engine/src/color_identity.rs`: `color_identity(card) -> ColorSet` (CR 903.4) unions
  `CardDefinition::colors()` with every mana symbol found in `oracle_text`, after stripping
  parenthetical reminder text first (CR 903.4c) - each `{...}` group found by a plain brace scan is
  parsed via the existing public `ManaCost::parse` (rewrapped in its own braces) rather than
  duplicating `mana.rs`'s private symbol-group grammar. `commander_color_identity(commanders)` (CR
  702.124c) unions across Partner commanders. `illegal_cards_for_color_identity(identity, cards)`
  (CR 903.5c) filters to cards whose own identity isn't a subset - CR 903.5d (basic lands) needed no
  separate logic, since a basic land's own `{T}: Add {G}`-shaped oracle text is already covered by
  the same general mechanism.

**Regression avoided, not introduced:** none - the four `todo!()` fills had no existing callers to
regress, confirmed via a repo-wide search before writing them.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 609 passed, 0 failed (595 prior + 14 new,
in new `crates/engine/tests/phase5_color_identity.rs`): `ManaCost::colors` correctly unions colored/
hybrid (both colors)/Phyrexian (counts as colored) symbols and excludes generic/colorless/snow;
`CardDefinition::colors` falls back from color indicator to mana-cost colors to colorless correctly;
CR 903.4's own worked example reproduced verbatim (Bosh, Iron Golem: `{8}` cost, red identity from
its ability text alone); reminder text is excluded (a card whose only colored symbol sits in
parentheses has empty identity); a card's identity includes colors from both its mana cost and its
rules text together; Partner commander union works; CR 903.5c's own worked example reproduced
verbatim (Wort, the Raidmother: a red card legal, a blue card illegal under her red/green identity);
basic land legality falls out of the general mechanism correctly (a Forest legal under green,
illegal under mono-blue, and vice versa for a Mountain). All 595 prior tests unaffected. No CLI
scenario for this slice - these are pure library functions with no `PlayerAction`/command surface to
drive through the TOML → Engine → CLI pipeline (nothing in the real engine pipeline calls them,
matching the confirmed "not wired into `new_game`" scope decision), so Rust test coverage is the
whole verification story here, same as any other pure-function slice (e.g. `cost.rs`'s
`apply_cost_delta`).

**STRUCT DEVIATIONS:** None - no new fields on any existing type, only new free functions in a new
module.

**INTERFACE DEVIATIONS:** None beyond what the pre-slice report scoped - `color_identity`/
`commander_color_identity`/`illegal_cards_for_color_identity` were all named directly, with the
exact signatures used.

**SCOPE DEVIATIONS:** None - the four `todo!()` fills were anticipated in the pre-slice report
("small side-effect... fills in several `todo!()` stubs"), not discovered mid-slice.

**UNRESOLVED (named during this slice, not silently narrowed):**
- Colors defined by characteristic-defining abilities (CR 903.4's own CDA clause) aren't included -
  no color-CDA mechanism exists in this engine (only P/T CDAs, via `PTValue::CharacteristicDefining`).
- A double-faced card's back face (CR 903.4d) and Adventure alternative characteristics (CR 903.4e)
  aren't included - neither mechanic exists in this engine (explicit non-goals elsewhere in this
  backlog).
- CR 903.4b (a commander granting a player a color choice before the game) isn't modeled - no card
  in this engine's test surface produces that kind of pregame choice.
- CR 903.5a/b (100-card deck size, unique English names) stay explicitly out of scope -
  `PlayerConfig.library: Vec<CardDefinition>` has no deck-list identity to check either rule
  against; a caller assembling one from an external decklist would need to enforce these itself
  before ever building a `GameConfig`.

## Phase 5: phase5/layer-engine-dependency slice

Branch `phase5/layer-engine-dependency`, off master. Closes backlog item 1.5a (CR 613.6-613.8, the
dependency system that can override timestamp ordering within a single layer/sublayer) - the first
of the four sub-slices (5a-5d) split out from the original layer-engine backlog item once layers
1/2/3 (copy/control/text) and the dependency system turned out to need their own design passes.

**Scope decision confirmed with the human before writing code:** a real finding surfaced while
reading `layers.rs`/`types/effect.rs` before writing anything - none of today's `EffectPrimitive`
variants (`SetPT`/`ModifyPT`/`AddKeyword`/`AddCardType`/`AddColor`/`SwitchPT`) can actually produce
an observable CR 613.8a dependency (they're all fixed values or unions, never a function of another
effect's current output), and `EffectFilter`/`AffectedFilter::Primitive` only ever read immutable
`CardDefinition` data. Proving the reorder algorithm would have had nothing real to test against
without also adding one "reads the running value" primitive. Confirmed: add
`EffectPrimitive::DoublePT` (layer 7c, CR 613.3 - e.g. Berserk's "target creature's power is
doubled") as part of this slice rather than proving the algorithm only against synthetic fixtures
with no card-text equivalent.

**What got implemented:**
- `types/effect.rs`: new `EffectPrimitive::DoublePT { power: bool, toughness: bool }` - doubles
  whichever field is `true`, layer 7c. Its result is defined by whatever value is already there,
  which is exactly CR 613.8a's dependency condition, so it always depends on a same-field `ModifyPT`
  (or another `DoublePT`) in the same sublayer. `Layer`'s own doc comment (previously a TODO
  explaining the gap) rewritten to describe what's real now and what's still open (see UNRESOLVED).
- `layers.rs`: new `order_by_dependency` - given a single layer's already-filtered effects, repeatedly
  picks the earliest-timestamp effect among those with no still-unplaced dependency (CR 613.8b:
  independent effects and simultaneously-ready dependents both order by timestamp), falling back to
  a plain timestamp sort of everything still unplaced the moment nothing is ever "ready" (CR 613.8b's
  last sentence - a dependency loop). New `depends_on(effect, other)` - CR 613.8a's actual test,
  scoped to what can be checked today: `DoublePT` depends on any same-field `ModifyPT` in the same
  sublayer, and on another `DoublePT` sharing a field (a real mutual/2-cycle case - doubling is
  commutative so the numeric result doesn't actually depend on order, but the dependency relationship
  itself is real per the rule's letter, making it a genuine test of the cycle-fallback path). Every
  other primitive pairing returns `false` (independent) - `Script`-payload effects too, always
  independent this slice. `applicable_effects_in_layer` now calls `order_by_dependency` instead of a
  bare `sort_by_key`; CR 613.8c's "re-evaluate after each step" requirement falls out for free
  architecturally, since `effective_characteristics` (and this reorder within it) is recomputed from
  scratch on every call, nothing cached. `effective_characteristics`'s layer-7c loop gained a
  `DoublePT` arm alongside `ModifyPT`.

**Regression avoided, not introduced:** none - `order_by_dependency` produces the exact same order
plain timestamp sorting would whenever nothing is dependent (every prior layer-engine test file
still passes unmodified), since a `depends_on` check against any non-`DoublePT` pairing is `false`.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 614 passed, 0 failed (609 prior + 5 new, in
new `crates/engine/tests/phase5_layer_engine_dependency.rs`): two independent `ModifyPT`s still just
sum regardless of order (regression check); a `DoublePT` at an *earlier* timestamp than the
`ModifyPT` it depends on still waits for the modify to apply first (2/2 base, +2/+2, then doubled =
8/8) - the test that actually proves dependency overrides timestamp, not just agrees with it by
coincidence; the same scenario with timestamps the other way around gives the identical 8/8, showing
dependency and timestamp agreeing isn't what made the first case work; a `DoublePT{power: true,
toughness: false}` only depends on/touches power, toughness ordering and value unaffected; two
mutually-dependent `DoublePT`s (a 2-cycle) don't hang or panic, falling back to timestamp order with
the (order-invariant, since doubling commutes) correct 8/8 result. All 609 prior tests unaffected,
including every existing `phase5_layer_engine_*` test file. No CLI/TOML scenario - matches precedent
from the 7a (CDA) and 7e (switching) sub-slices, where layer-ordering fixtures were Rust-integration-
test-only; `crates/scenario/src/lib.rs`'s `ContinuousEffectSpec` still only wires 4 of the (now 8)
built primitives into TOML, unchanged by this slice.

**STRUCT DEVIATIONS:** None beyond what the pre-slice report scoped exactly (`EffectPrimitive::
DoublePT`, confirmed with the human before writing code) - this *is* the plan, not a deviation.

**INTERFACE DEVIATIONS:** None - `order_by_dependency`/`depends_on` are both new, internal to
`layers.rs`, exactly the shape anticipated (a generic reorder over a pluggable `depends_on`
predicate, not hardcoded to `DoublePT` specifically, so future layers can plug in without rework).

**SCOPE DEVIATIONS:** None - the `DoublePT` addition was confirmed with the human before
implementation started (see the scope decision above), not decided unilaterally mid-slice.

**UNRESOLVED (named during this slice, not silently narrowed):**
- `EffectPayload::Script`/`AffectedFilter::Script` effects are always treated as independent - a
  script-declared dependency would need the script bridge to expose partial/in-progress layer state
  mid-computation, which `state_to_lua`'s snapshot deliberately doesn't do yet (it's a full-board
  snapshot taken once, not layer-resolved - see that function's own doc comment).
- Cross-layer/cross-sublayer dependency isn't modeled - CR 613.8 is explicitly same-layer/sublayer
  only; cross-layer sequencing is already handled by the fixed layer order in
  `effective_characteristics`, not by this algorithm, so this is faithful to the rule rather than a
  gap.
- Layers 4/5/6/7b/7e keep plain timestamp ordering - nothing in them is shaped as a function of
  another effect's output, so no same-layer dependency can arise there with today's primitives; only
  7c (`ModifyPT` vs. `DoublePT`) gets real dependency-aware ordering this slice.
- Layers 1/2/3 (copy/control/text-changing) still don't exist at all - untouched, per backlog items
  5b/5c/5d, next in this sub-sequence.

## Phase 5: phase5/layer-engine-control slice

Branch `phase5/layer-engine-control`, off master. Closes the query half of backlog item 1.5b (CR
613.1b/108.4, control-changing effects) - the second of the four sub-slices (5a-5d) split out from
the original layer-engine backlog item.

**Scope decision confirmed with the human before writing code:** the item's own pre-existing text
named a real fork - model control as a genuine layer-2 live query (recomputed every call, like
power/toughness) or as a durable state mutation (a `GainControl` event directly mutates
`GameObject.controller`, matching the `Tapped`/`Untapped` convention). Initially proposed the
mutation shortcut as smaller and self-contained; the human asked to scope the full query-based
implementation instead, on the reasoning that the mutation approach would need its own hand-rolled
grant-stack to correctly handle overlapping/expiring control effects - duplicating exactly what the
layer engine's own CR 613.7/613.8 timestamp/dependency ordering (built in the immediately prior
`phase5/layer-engine-dependency` slice) already solves generically, and would leave `Layer::
ControlChangingEffects` permanently dead code. Confirmed: go with the live query, but split it from
the read-site migration - this slice proves `effective_controller` in isolation (same "prove the
query, wire consumers later" bootstrap pattern the very first `phase5/layer-engine-basics` slice
used for the whole layer engine), a follow-up slice migrates the ~15 real `GameObject.controller`
read-sites once this exists.

**What got implemented:**
- `types/effect.rs`: new `EffectPrimitive::GainControl(PlayerId)` - layer 2, override semantics
  (later timestamp replaces earlier outright, not additive - same shape as `SetPT`/layer 7b). No new
  CR 613.8 dependency case needed: a `GainControl`'s result never depends on whether another
  `GainControl` already applied (it's a flat override, not a function of a running value the way
  `DoublePT` is), so `depends_on`'s existing wildcard fallthrough plus plain CR 613.7 timestamp
  ordering is already correct here - confirmed by reading `layers::depends_on` before writing any
  new code, not assumed. `Layer`'s own doc comment (the layer-2 TODO block) rewritten to describe
  what's real now and what's still deferred.
- `layers.rs`: new `pub fn effective_controller(state, script_runtime, object) -> PlayerId` -
  mirrors `effective_characteristics`'s own 7b loop exactly: starts from `GameObject.controller` (the
  base value, still the sole source of truth - nothing mutates it), then the last (by timestamp)
  applicable `GainControl` effect in `Layer::ControlChangingEffects` overrides it, reusing the
  existing `applicable_effects_in_layer`/`resolve_primitive` machinery unchanged.

**Regression avoided, not introduced:** none - `effective_controller` is a new, standalone function;
nothing existing calls it yet, so nothing existing could regress.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 618 passed, 0 failed (614 prior + 4 new, in
new `crates/engine/tests/phase5_layer_engine_control.rs`): no `GainControl` effect returns the
object's plain `controller` field unchanged (base case); one `GainControl` effect overrides the base
controller; two `GainControl` effects at different timestamps - the later one wins outright, not
additively, mirroring `SetPT`'s own override test; an `EffectFilter`-scoped `GainControl` (Opponent-
only) affects only the matching object, leaving an unscoped one's controller untouched. All 614
prior tests unaffected. No CLI/TOML scenario - matches precedent from every other layer-ordering
sub-slice (7a/7c/7e/dependency), Rust-integration-test-only; `crates/scenario/src/lib.rs`'s
`ContinuousEffectSpec` unchanged by this slice (still only wires 4 of the now-9 built primitives into
TOML).

**STRUCT DEVIATIONS:** None beyond what the pre-slice report scoped exactly (`EffectPrimitive::
GainControl`, confirmed with the human before writing code).

**INTERFACE DEVIATIONS:** None - `effective_controller`'s signature is exactly `effective_characteristics`'s
own shape (`state, script_runtime, object -> value`), as anticipated.

**SCOPE DEVIATIONS:** None - the query-only split (deferring the read-site migration) was confirmed
with the human before implementation started, not decided unilaterally mid-slice.

**UNRESOLVED (named during this slice, not silently narrowed):**
- Nothing in live gameplay can observe a `GainControl` effect's result yet - the ~15 real
  `GameObject.controller` read-sites (combat, targeting, cost filters, `has_summoning_sickness`,
  `EffectFilter::applies_to`/`FilterSpec::matches`) still read the raw field directly. Named
  follow-up slice, not a gap introduced silently - see this item's own backlog note (above) for why
  that migration is a real design fork of its own (whether `EffectFilter::applies_to`/`FilterSpec::
  matches`, deliberately `&ScriptRuntime`-free today, take a pre-resolved effective-controller
  parameter instead of reading `.controller` directly, or some other shape).
- No command/`AbilityEffect`/script binding produces a `GainControl` effect yet (no
  `Game.Ops.GainControl`) - test fixtures build it via struct literal, same bootstrap-before-scripting
  convention every other layer-engine sub-slice used.
- 11b's `StackObject.controller`-as-owner-stand-in gap (CR 800.4a/800.4c, `sba.rs`'s
  `remove_players_objects`) is still open - `StackObject` has no separate owner field, and this
  slice's query isn't consulted there either, since that's part of the same deferred read-site
  migration.

## Phase 5: phase5/layer-engine-control-consumers slice

Branch `phase5/layer-engine-control-consumers`, off master. Closes the consumer half of backlog
item 1.5b - migrates every real `GameObject.controller` read-site to `layers::effective_controller`
instead of the raw field, the follow-up split out from the `phase5/layer-engine-control` query slice.

**Real design finding surfaced while implementing, not anticipated in either slice's pre-slice
report:** `layers.rs`'s own internals (`applicable_effects_in_layer`'s `AffectedFilter::Script`
resolution, `ContinuousEffect::applies_to`'s `EffectFilter::controller` check) could NOT be migrated
to `effective_controller` - `effective_controller` itself calls `applicable_effects_in_layer(...,
Layer::ControlChangingEffects)`, so resolving layer 2's own filtering through `effective_controller`
would make that call recurse into itself infinitely the moment `applicable_effects_in_layer` is asked
to filter layer 2 effects. Every *other* layer (4,5,6,7a,7b,7c,7e) would be safe to migrate this way
(no mutual recursion), but migrating only some layers while leaving layer 2 raw would be an
inconsistent, easy-to-forget special case for a marginal correctness gain (nothing filters on a
control-changed source's card_types/keywords in a way this actually changes today). Decision: leave
`layers.rs`'s own internal filtering exactly as it was, for every layer uniformly - this is a
deliberate, structural exclusion, not an oversight, and confirmed safe by construction rather than
guessed at.

**What got migrated (all real `GameObject.controller` reads that could observe a `GainControl`
effect):**
- `commands.rs`: `target_is_legal`'s `TargetFilter::Object` controller check; `apply_damage_event`'s
  Lifelink life-gain destination; `ActivateAbility`'s "you control the source" check;
  `DeclareAttackers`/`DeclareBlockers`'s five controller checks; `has_summoning_sickness` (gained a
  `&ScriptRuntime` param - all 3 callers already had one in scope).
- `engine.rs`: `untap_step`'s "untap what the active player controls."
- `sba.rs`: `find_legend_rule_group`'s CR 704.5j controller/name grouping.
- `types/cost.rs`: `FilterSpec::matches` gained an `object_controller: PlayerId` parameter (kept the
  type itself `ScriptRuntime`-free, same as `ContinuousEffect::applies_to` - the caller resolves the
  live controller and passes it in) - real callers are `PaymentPlan::validate`'s Sacrifice/
  TapPermanents checks (which also gained a `&ScriptRuntime` param, since it discovers candidate
  objects mid-iteration and can't pre-resolve them all up front the way a single-object caller can).
- `types/effect.rs`: `event_target_matches`/`ReplacementEffect::matches` gained a `&ScriptRuntime`
  param directly (a different treatment than `FilterSpec::matches` - justified since `PaymentPlan::
  validate` had already broken the "no ScriptRuntime" convention for the same practical reason, and
  replacement effects are a wholly separate `state.replacement_effects` list from `state.
  continuous_effects`, so no recursion risk exists here the way it does for `ContinuousEffect::
  applies_to`). `replacement.rs`'s `affected_player_for`/`apply_replacements`/`apply_replacements_from`
  threaded `&ScriptRuntime` through to reach it; `sba.rs`'s `resolve_deaths` too (its own call into
  `apply_replacements`).

**Explicitly excluded, named not silently skipped (found by tracing each site, not assumed):**
- `commands.rs`'s `PlayLand`/`CastSpell` zone checks, `engine.rs`'s `submit_discard`, `engine.rs`'s
  search/tutor candidate filter, `PaymentPlan::validate`'s Discard cost, `types/cost.rs`'s
  `apply_cost_modifiers` - all check `.controller` on a Hand/Graveyard/Command/Library-zone card or a
  spell on the stack, where it always equals `.owner` in this engine and no `GainControl` effect (CR
  108.4, permanents only) can realistically target something in those zones. Migrating would be a
  functional no-op; left as raw-field reads with a comment explaining why at each site.
- `types/ability.rs`'s `DiesMatching`/`LeavesBattlefieldMatching` conditions read `lki.controller` (a
  frozen `LastKnownInfo` snapshot, CR 603.10) - correctly stays raw, matching how LKI is treated
  everywhere else in this codebase (frozen at capture time, never re-resolved live).
- `script.rs`'s `state_to_lua`/`card_summary_to_lua` (the Lua-visible `controller` field) - left as
  the raw/base value, consistent with that same function's own pre-existing, already-documented
  "not layer-resolved" stance for power/toughness.
- `StackObject.controller`/`RegisteredAbility`/`LastKnownInfo.controller` throughout `engine.rs`/
  `triggers.rs` - different fields entirely, already frozen/baked-in-at-push-time by design.

**Regression avoided, not introduced:** none - every migrated site produces the identical result to
before whenever no `GainControl` effect is active (confirmed by the full prior test suite passing
unmodified), since `effective_controller` degrades to the plain field read in that case.

**VERIFICATION RESULT:** Pass. `cargo test --workspace`: 626 passed, 0 failed (618 prior + 8 new, in
new `crates/engine/tests/phase5_control_change_consumers.rs`, driving the real `Engine`/
`execute_command` pipeline, not just the bare query): the effective (not original) controller can
declare a `GainControl`'d creature as an attacker/blocker/activate its ability, and the original
owner can no longer do any of the three; summoning sickness is checked against the new controller's
own `last_active_turn_number` - a creature controlled since turn 5 reads as sick under a new
controller whose own most recent turn started on turn 1 (`5 >= 1`), even though the *same* creature
would read as long-since-controlled and not sick if (incorrectly) checked against a veteran original
owner whose last active turn was 10 (`5 >= 10` false) - the concrete case this migration fixes, not
just a synthetic assertion; a `GainControl`'d creature can be sacrificed by its new controller to pay
a "sacrifice a creature you control" cost even though a different player owns it. All 618 prior tests
unaffected.

**STRUCT DEVIATIONS:** None - no new fields, only signature changes to existing functions/methods
exactly as the pre-slice report scoped (`FilterSpec::matches`, `ContinuousEffect::applies_to` -
except the recursion finding excluded it after all, see below).

**INTERFACE DEVIATIONS:** `has_summoning_sickness`, `PaymentPlan::validate`, `event_target_matches`/
`ReplacementEffect::matches`, `apply_replacements`/`apply_replacements_from`/`resolve_deaths` all
gained a `&ScriptRuntime` parameter - anticipated in the pre-slice report's "needs `&ScriptRuntime`
threaded one level in" bucket, not a deviation. `FilterSpec::matches` gained a plain `PlayerId`
parameter instead (pre-resolved by the caller) - exactly the "pre-resolve-and-pass-in" treatment the
report proposed for it.

**SCOPE DEVIATIONS:** `ContinuousEffect::applies_to` (and `layers.rs`'s own internal `AffectedFilter::
Script`/`EffectFilter::controller` resolution) was in the pre-slice report's "needs the pre-resolve-
and-pass-in treatment" bucket - discovered while implementing that this would create infinite
recursion for layer 2 specifically (see the design finding above) and pulled out into its own
deliberate exclusion instead. `engine.rs`'s search/tutor filter and `PaymentPlan::validate`'s Discard
cost were originally scoped as real migration sites in the pre-slice report; re-examining them while
implementing showed both check a Library/Hand-zone card, the same "no-op, no `GainControl` effect can
reach here" reasoning already applied to `PlayLand`/`CastSpell` - moved to the exclusion list instead
of migrated.

**UNRESOLVED (named during this slice, not silently narrowed):**
- No command/`AbilityEffect`/script binding produces a `GainControl` effect yet - test fixtures build
  it via struct literal, same bootstrap convention every layer-engine sub-slice used. Closed for the
  resolving-spell/ability creation path, `phase5/gain-control-effect` (docs/implementation.md, below)
  - a static ability's own aura-sourced control change is still open, see that slice's own notes.
  `controlled_since_turn` still isn't bumped by anything resembling a real "gain control" mutation
  event either (the query-only slice's own UNRESOLVED note, still true) - this slice's summoning-
  sickness test demonstrates the *query* is now correct by directly constructing a `controlled_
  since_turn` value, not by proving a real gameplay action updates it. `phase5/gain-control-effect`
  deliberately left this alone too, rather than half-fixing it.
- 11b's `StackObject.controller`-as-owner-stand-in gap (`sba.rs`'s `remove_players_objects`) is still
  open - unrelated to this migration, `StackObject` has no owner field at all to migrate.
- `layers.rs`'s own internal filtering stays on the raw field for every layer (not just layer 2), a
  deliberate, uniform exclusion (see the design finding above) - a control-changed permanent's
  keywords/card_types won't correctly gate *other* continuous/replacement effects' own `EffectFilter`
  checks against it. No test in this engine's current surface exercises that combination.

## Phase 5: phase5/layer-engine-text slice (backlog item 1, sub-slice 5d — final layer-engine sub-slice)

Branch `phase5/layer-engine-text`, off master. Closes backlog item 1's last open sub-slice — CR
613.1c layer 3 (text-changing effects), narrowed to CR 612.8 name-setting only, exactly as that
item's own pre-existing text recommended scoping it once this slice's requirements pass happened.
The general CR 612.2 "correct usage" word/symbol substitution case, and the single-card rules
(612.5-612.7/612.9/612.10), stay unmodeled.

**What got implemented:** `EffectPrimitive::SetName(String)`, the first non-`Copy` variant of that
enum (`EffectPrimitive` dropped its `Copy` derive to accommodate it — `layers::resolve_primitive`'s
`EffectPayload::Primitive(p) => Some(*p)` became `Some(p.clone())`; `script.rs`'s `EffectPrimitiveTag`
wrapper dropped `Copy` too, keeping `Clone`). Wired into `layers::effective_characteristics` as a new
`Layer::TextChanging` pass, positioned before layers 4-7 (CR 613.3's ordering), same override-wins-
outright shape `GainControl`/`SetPT` already established — a later-timestamp `SetName` replaces an
earlier one's result entirely, doesn't combine with it, so no CR 613.8a dependency case applies (same
reasoning `GainControl`'s own doc comment gives). `Game.Layers.TextChanging`/`Game.Effects.SetName(str)`
Lua binding pair added alongside the other six; `invoke_continuous_effect`'s "Lua asks, Rust decides"
decode table gained a seventh recognized key, `set_name = "..."`, checked first in the fixed order.
`validate_layer_effect_pairing` gained the matching `(Layer::TextChanging, EffectPrimitive::SetName(_))`
pair so a script mismatching layer and effect still gets a real `:Register()`-time error, not a
silently inert effect.

**Real consumer found for free, not built this slice:** `sba.rs`'s `find_legend_rule_group` already
groups by `effective_characteristics(...).name`, not the raw `CardDefinition` field (predates this
slice — it was already reading the effective query's output, just one that always equaled the raw
name until now). A renamed legendary immediately participates in CR 704.5j correctly with zero new
plumbing, the same "turned out to need no new plumbing" finding the layer-engine-consumers sub-slice
hit for combat damage/blocking. Proven directly: `renaming_a_legendary_to_match_another_triggers_
the_legend_rule` in the new test file confirms two differently-named legendaries under one controller
don't violate the rule until a `SetName` effect makes one's effective name collide with the other's,
at which point `check_state_based_actions` really does pause on `NeedsLegendRuleChoice`.

**Deliberately not migrated:** `commander_casts` (`commands.rs`, keyed by `definition.name`) and
color-identity stay on the raw printed name, not `effective_characteristics(...).name`. Commander tax
and color identity track the commander's *printed* identity, not whatever a renamed-in-play object's
name happens to be this turn — a deliberate distinction, not a missed migration (the opposite of why
`layers.rs`'s own internal filtering couldn't migrate to `effective_controller` in the control-
consumers slice — that was a recursion hazard; this is an intentional identity-vs-live-name split).

**No TOML/CLI scenario fixture this slice** — a deliberate scope match with the sibling sub-slices
immediately before this one (layer-engine-control, -switching, -dependency, -copy all shipped without
one too), not an oversight. `ContinuousEffectSpec`'s TOML DSL (`scenario/src/lib.rs`) only ever covered
type/color/PT-setting/PT-modifying layers — extending it to cover `SetName` would be its own small
scope-creep, not something this "narrow it hard" slice's own pre-existing recommendation asked for.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 5 new tests in
`crates/engine/tests/phase5_layer_engine_text.rs` (no-effect baseline, `SetName` overriding the base
name, later-timestamp `SetName` winning outright over an earlier one, `self_only` filter scoping, and
the legend-rule real-consumer test above), all prior tests unaffected (638 total, 0 failed). Confirmed
via `cargo build --workspace` too — dropping `EffectPrimitive`'s `Copy` derive compiled clean with no
other call site relying on it.

**STRUCT DEVIATIONS:** None beyond the one predicted going in (`EffectPrimitive::SetName(String)`
itself) — the `Copy`-derive removal was anticipated risk, not a surprise, once a `String`-carrying
variant was the plan.

**UNRESOLVED (named during this slice, not silently narrowed):**
- General CR 612.2 text-substitution ("changes only correctly-used words") and the single-card rules
  (612.5 Exchange of Words, 612.6 Volrath's Shapeshifter, 612.7 Spy Kit, 612.9 name stickers, 612.10
  splice) are all still unbuilt — `CardDefinition.oracle_text` remains a plain, never-parsed `String`.
- "Loses all abilities" (a text-removal shape, CR 612.3's own carve-out notwithstanding — a card can
  say this directly) still isn't modeled; layer 6's `AddKeyword` stays union-only, same as before this
  slice.
- No `ContinuousEffectSpec`/TOML DSL support for `SetName` — test fixtures build it via Rust struct
  literal or the new Lua binding, not the scenario format.
- This closes backlog item 1 in its entirety — layers 1 (copy, `phase5/layer-engine-copy`), 2
  (control, `phase5/layer-engine-control`/`-control-consumers`), 3 (text, this slice), 4/5 (type/color,
  `phase5/layer-engine-types-colors`), 6/7b/7c (`phase5/layer-engine-basics`), 7a (CDA,
  `phase5/layer-engine-cda`), 7d (counters, folded into `resolve_base_pt`), 7e (switching,
  `phase5/layer-engine-switching`), and the CR 613.6-8 dependency system (`phase5/layer-engine-
  dependency`) are all real now. Only backlog item 16 (Planeswalkers/Battles/DFCs/split/Sagas,
  explicit non-goal) remains open in the slice backlog.

## Phase 5: phase5/trample slice (CR 702.19a/b/d)

Branch `phase5/trample`, off master. Not its own numbered backlog item — picked off the "Known
unresolved gaps" list's combat-keywords entry (above) as the highest-value next slice once backlog
item 1 closed: trample is common on real cards, unlike banding, and is a bounded extension of the
`first-strike` slice's own combat-damage-assignment code rather than a new subsystem. Scoped to CR
702.19a/b/d only — trample over planeswalkers (702.19c/e/f/g) is a separate keyword, not modeled,
since planeswalkers/battles are backlog item 16's explicit non-goal (every attack target in practice
is `Target::Player`).

**Real pre-existing bug found while scoping, fixed as part of this slice (not split out — squarely
blocks correct trample semantics):** `AssignCombatDamage` built its per-attacker blocker map
(`blockers_by_attacker`) only from *living* blockers. An attacker whose blockers had all died before
the damage-assignment step (declared blocked, then removed from combat by some other effect) fell into
the exact same `None` branch as a never-blocked attacker, and so dealt its full power straight to the
defending player — contradicting CR 509.1h/510.1c ("blocked" is fixed at declare-blockers time and
doesn't change even if every blocker later leaves; a blocked attacker with zero living blockers deals
*no* combat damage, unless it has trample — CR 702.19d is the one case that *does* send it to the
player). Fixed by tracking `declared_blocked_attackers` from the raw, unfiltered `combat.blocks`,
kept deliberately separate from `blockers_by_attacker`'s alive-only view.

**What got implemented:**
- `KeywordSet::TRAMPLE` (bit 14), parsed in `script.rs`'s `parse_keyword_name` and `scenario/src/
  lib.rs`'s `parse_keywords` — same two sites every prior combat keyword touched, no layer-engine
  changes (trample is read via `effective_keywords`, same as first/double strike, not a continuous-
  effect primitive).
- Two new `commands.rs` helpers: `effective_toughness` (mirrors `effective_power`) and
  `lethal_damage_needed` (CR 702.19b: effective toughness minus already-marked damage, floored at 0,
  netted to 1 under the attacker's Deathtouch per CR 702.2b — an unresolved toughness reads as
  `u32::MAX`, "needs everything," rather than silently treating unknown as satisfied).
- `AssignCombatDamage`'s "who must submit a chosen split" set widened from "2+ living blockers" to
  "2+ living blockers, OR exactly 1 living blocker with trample" — a single trampling blocker still
  needs a player-chosen lethal-vs-excess split, unlike the old hardcoded "sole blocker gets
  everything" shortcut.
- Split validation for a trampler: the blocker-amounts sum may now be *less* than the attacker's
  power (the shortfall implicitly spills to the original target) — but only once every blocker in the
  split has been assigned at least `lethal_damage_needed`. Falling short of any blocker's lethal
  amount forces the entire assigned amount to stay on blockers (CR 702.19b's own "in that case can't
  assign any damage to the player" clause), rejected outright rather than silently clamped.
- The assign-and-deal pass computes the remainder (`power - assigned_to_blockers`) for any attacker
  that submitted a split and emits an extra `DamageDealt` to the original target when it's nonzero —
  covers both the widened single-blocker-trample case and the pre-existing 2+-blocker case uniformly.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 9 new tests in
`crates/engine/tests/phase5_trample.rs` (unblocked trample is a no-op; a single trampling blocker now
requires a split; the split correctly divides lethal-vs-excess; shorting a blocker's lethal blocks any
excess from reaching the player; assigning everything to the blocker leaves the player untouched;
multi-blocker trample; Deathtouch+trample needing only 1 damage to call a blocker lethal; the CR
509.1h/510.1c bug-fix regression itself, both without and with trample per CR 702.19d), all prior
tests unaffected (647 total, 0 failed) — including every pre-existing multi-blocker split test in
`phase5_combat_damage.rs`/`phase5_first_strike.rs`, confirming the widened validation didn't change
behavior for a non-trampling attacker.

**STRUCT DEVIATIONS:** None — no new fields on any existing type, only the one new keyword bit and
two new free functions in `commands.rs`, exactly as scoped going in.

**UNRESOLVED (named during this slice, not silently narrowed):**
- Trample over planeswalkers (CR 702.19c/e/f/g) stays unmodeled — a separate keyword this engine has
  no reason to add until planeswalkers themselves exist (backlog item 16).
- No `ContinuousEffectSpec`/TOML DSL support for `Trample` — same deliberate scope match with the
  layer-engine sub-slices immediately before this one; test fixtures build it via the Rust keyword
  bitflag directly.
- Banding, and the rest of CR 702's combat-keyword list beyond first/double strike/trample, are still
  unbuilt — updated the "Known unresolved gaps" list (above) to reflect trample closing rather than
  leaving it stale.

## Phase 5: phase5/multi-target-resolution slice (CR 601.2d/608.2b)

Branch `phase5/multi-target-resolution`, off master. Not its own numbered backlog item — pulled from
the "Known unresolved gaps" list's targeting entry.

**Found while scoping, bigger than the gaps-list entry named:** every `AbilityEffect` resolution arm
in `engine.rs` (`Damage`, `Sacrifice`, `Destroy`, `Exile`, `Counters`) read `top.targets.first()` only
— already documented at each site as a deliberate narrowing ("single fixed target only... same
narrowing targeting-basics already established"), so not a hidden bug the way trample's dead-blocker
conflation was, just a bigger pre-existing narrowing than the gaps list called out by name: a
`TargetRequirement` with `count: 2+` validated fine at cast time, but only ever affected the first
chosen target at resolution — the rest were silently ignored. Combined with CR 608.2b's fizzle check
already being correctly all-or-nothing for the *resolve-or-not* decision, this meant the actual
partial-fizzle text ("still resolves, but won't affect those targets that are illegal") had never
been exercisable, since there was never more than one real target in play.

**What got implemented:**
- `ResolveStackTop`'s fizzle computation now derives `legal_targets: Vec<Target>` — the subset of
  `top.targets` still legal against `top.target_requirement`'s filter — once, up front. `fizzles`
  becomes `legal_targets.is_empty()` (for a real, count > 0 requirement), an equivalent but simpler
  refactor of the previous `.all(|t| !target_is_legal(...))` check.
- All five `AbilityEffect` resolution arms changed from `top.targets.first()` to iterating every
  entry in `legal_targets` independently: `Damage` deals its full `amount` to each (CR 601.2d's
  "divide as you choose" stays unmodeled — no split, every legal target gets the whole amount);
  `Sacrifice`/`Destroy`/`Exile`/`Counters` apply their own per-object body to each `Target::Object`
  entry, skipping any non-Object entry the same "silent no-op" way the single-target code already
  did.
- Net effect is CR 608.2b's real partial fizzle for free: a target that's individually illegal by
  resolution time is simply absent from `legal_targets`, so it's skipped during effect application
  without needing a second bespoke check.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 5 new tests in
`crates/engine/tests/phase5_multi_target_resolution.rs` (a 2-target Damage spell hits both targets
independently, not split; one target leaving the battlefield before resolution only skips that one,
doesn't fizzle the spell; both targets leaving fully fizzles it; a 2-target Destroy spell kills both
independently; one target already gone still lets Destroy affect the other), all prior tests
unaffected (652 total, 0 failed) — including every existing single-target (`count: 1`) test across
`phase5_script_ops_damage.rs`/`phase5_script_ops_destroy.rs`/`phase5_script_ops_exile.rs`/etc., proving
the refactor didn't change count=1 behavior at all.

**STRUCT DEVIATIONS:** None — no new fields on any type; `legal_targets` is a local `Vec` computed
inline in `ResolveStackTop`, not stored anywhere.

**UNRESOLVED (named during this slice, not silently narrowed):**
- CR 601.2c/115.6 "up to N"/variable target counts — `TargetRequirement::count` still exact only,
  deliberately deferred as its own smaller, separable follow-up rather than bundled in here.
- Multi-"target"-word spells ("target creature and target player") — `CardDefinition.target_
  requirement` is still a single `Option<TargetSpec>`, not modeled.
- `TargetSpec::Script`/`ScriptRuntime::invoke_target` — still dead code, unrelated to this slice.
- CR 601.2d's actual division/distribution (a player choosing how to split a fixed amount among
  several targets, e.g. "divide 3 damage among one or two target creatures") — still unmodeled; this
  slice's `Damage` gives every legal target the *full* amount independently, not a chosen split.

## Phase 5: phase5/targeting-up-to-n slice (CR 601.2c/115.6)

Branch `phase5/targeting-up-to-n`, off master. Not its own numbered backlog item — the previous
slice's own UNRESOLVED section named this as the natural next follow-up.

**Found while designing this, fixed as part of the same slice:** `ResolveStackTop`'s fizzle check
used to gate on `req.count > 0` as a proxy for "were any targets actually chosen." That coincided with
reality under the old exact-count-only requirement (`validate_targets` always forced `targets.len()
== count`, so `count > 0` implied at least one target was chosen) — but breaks once "up to N" lets a
caster legally choose zero targets while `count` itself stays nonzero. Fixed by gating on
`!top.targets.is_empty()` (what was actually chosen) instead of `req.count` (the schema's own max).

**What got implemented:**
- `TargetRequirement` (types/target.rs) gains `up_to: bool` — `false` (default, unchanged behavior)
  means exactly `count`; `true` means anywhere from 0 to `count` (CR 601.2c/115.6). "N or more" stays
  unmodeled, named explicitly rather than silently guessed at.
- `commands.rs`'s `validate_targets`: accepts `targets.len() <= count` under `up_to`, still `==`
  otherwise — dedup and per-target legality checks are unchanged, applying to whatever subset was
  actually chosen.
- `engine.rs`'s `ResolveStackTop` fizzle gate: `!top.targets.is_empty()` instead of `req.count > 0`,
  per the finding above.
- `scenario/src/lib.rs`'s `TargetRequirementSpec` gains a matching `#[serde(default)] up_to: bool`.
  Confirmed (again) there's no Lua/script-builder path for target requirements at all — `CardDefinition.
  target_requirement` is still a static field only Rust fixtures/the scenario TOML populate.
- Every existing test fixture across the workspace constructing a `TargetRequirement` struct literal
  needed a new `up_to: false` field (20 call sites across 11 test files) — a mechanical ripple, not a
  behavior change; confirmed via `cargo test --workspace --no-run` catching every site before running
  anything.

**No TOML/CLI scenario fixture this slice** — considered, but the scenario format's assertion
vocabulary (`assert_object_zone`/`assert_ledger_len`/etc.) can't actually distinguish "resolved
normally with zero targets chosen" from "fizzled" here: a fizzled spell still moves to the graveyard
just like a normal resolution does, and with zero targets chosen there's no `DamageDealt` event
either way to check for. The real behavior (no `Fizzled` event logged) needs direct ledger inspection,
which only the Rust integration tests below can do - a bespoke assertion step just for this would be
its own small feature, not worth it for one slice's fixture.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 7 new tests in
`crates/engine/tests/phase5_targeting_up_to_n.rs` (0 targets chosen is legal and resolves; 0 targets
chosen never logs a `Fizzled` event, the gate-fix's own regression test; 1 of 2 allowed targets is
legal; the maximum (2) is legal; 3 (over max) is rejected; a duplicate among the chosen is rejected;
the lone chosen target becoming illegal before resolution still fully fizzles), all prior tests
unaffected (659 total, 0 failed).

**STRUCT DEVIATIONS:** None beyond the one predicted going in (`TargetRequirement::up_to` itself) —
the 20-site test-fixture ripple was anticipated risk of adding a new struct field, not a surprise.

**UNRESOLVED (named during this slice, not silently narrowed):**
- "N or more" (CR 601.2c/115.6's other variable shape) — still unmodeled.
- Multi-"target"-word spells and `TargetSpec::Script`/`invoke_target` — unrelated to this slice, still
  open from before it.
- CR 601.2d's division/distribution — still unmodeled, unrelated to count shape.

## Phase 5: phase5/conditional-continuous-effects slice (CR 611.2c)

Branch `phase5/conditional-continuous-effects`, off master. Scoped tightly to
`InterveningIfPrimitive::ControllerControlsAtLeast` — the pre-slice write-up's one concrete
condition shape — per its own explicit note that `EffectDuration`'s broader inert-duration gap
(`UntilYourNextTurn`/`UntilEndOfCombat` never expire) stays untouched.

**Deviation found while implementing — dropping `Copy`:** `InterveningIfPrimitive` and
`InterveningIfSpec` derived `Clone, Copy, Debug` going in. Adding `ControllerControlsAtLeast {
filter: FilterSpec, count: u32 }` broke `Copy` (`FilterSpec` owns a `Vec`/`String`) — anticipated
(the pre-slice note already flagged the same `Clone`-only fate `ConditionPrimitive` hit for the same
reason), but the ripple was two real call sites, not zero: `engine.rs`'s `ResolveStackTop` read
`ability.intervening_if` by value out of a `&RegisteredAbility`, and `script.rs`'s
`TriggeredAbilityBuilder::Register` did the same out of `&mut self` — both became `.clone()`. Also
`InterveningIfTag` (script.rs's `mlua::UserData` wrapper) had to drop `Copy` the same way
`ConditionTag` already had, and its one consumer (`SetInterveningIf`'s `ud.borrow::<InterveningIfTag>
()?.0`) needed a `.clone()` added.

**Deviation from the pre-slice note — raw controller, not `effective_controller`:** the note said
`holds()` would resolve "you" via `layers::effective_controller`, "same as `EffectFilter::controller`
does elsewhere." Checking that claim while implementing found it was wrong: `ContinuousEffect::
applies_to`'s own `FilterController` check reads the raw `GameObject.controller` field directly, not
`effective_controller` — confirmed by `Layer::ControlChangingEffects`'s own doc comment, which already
states plainly that "nothing in live gameplay can observe a `GainControl` effect's result yet."
Matched that existing convention instead: `ControllerControlsAtLeast` resolves both "you" (the
source's controller) and each candidate permanent's controller off the raw field. This also sidesteps
a real recursion risk the note didn't call out — `effective_controller` itself calls
`applicable_effects_in_layer`, so having a condition check inside that same function call back into
`effective_controller` would recurse for any conditional effect sitting in `Layer::
ControlChangingEffects` itself (not a shape this slice's tests exercise, but worth naming rather than
silently building in a latent stack-overflow).

**Found while writing the TOML fixture — scenario DSL had no way to express this at all:**
`ContinuousEffectSpec` (scenario/src/lib.rs) had no field for a condition. Added
`condition_card_types: Vec<String>` / `condition_count: Option<u32>` (the latter present = build a
`ControllerControlsAtLeast` condition, same "presence signals the feature" convention `up_to`/
`condition_count` siblings elsewhere already use) — not explicitly scoped by the pre-slice note, but
needed to write a real `[[scenarios]]` regression test rather than only Rust integration tests, and
mechanical given `build_continuous_effect` already existed as the one conversion site.

**What got implemented:**
- `types/effect.rs`: `ContinuousEffect` gains `pub condition: Option<InterveningIfSpec>` — `None`
  (every effect built before this slice) is unconditional, unchanged behavior.
- `types/ability.rs`: `InterveningIfPrimitive::ControllerControlsAtLeast { filter: FilterSpec, count:
  u32 }`; `holds()`'s new arm counts `state.objects` in `ZoneKind::Battlefield` matching `filter` from
  `controller`'s perspective and compares against `count`.
- `layers.rs`: `applicable_effects_in_layer` (the one call-site every layer already funnels through)
  checks `e.condition` — `None` passes through, `Some(Primitive(p))` calls `p.holds(...)`,
  `Some(Script(handle))` calls `script_runtime.invoke_intervening_if(...)` (same fallback-to-`false`-
  on-error stance every other unresolvable script value in this engine already takes) — before the
  existing affected-filter match, so a condition-failing effect contributes nothing to the layer at
  all, re-evaluated fresh on every call.
- `script.rs`: `Game.InterveningIf.ControlsAtLeast(filter_table, count)` (reuses `decode_filter_table`,
  same table shape `Game.Conditions.DiesMatching`/`Game.Costs.Sacrifice` already accept) alongside the
  existing `AtOrBelowLife`; `StaticAbilityBuilder` gains a `condition` field and `:SetCondition(Game.
  InterveningIf.* | function)`, decode logic copied verbatim from `TriggeredAbilityBuilder::
  SetInterveningIf`; `Register()` carries `this.condition.clone()` onto the built `ContinuousEffect`.
- `scenario/src/lib.rs`: `ContinuousEffectSpec.condition_card_types`/`condition_count`;
  `build_continuous_effect` builds the matching `InterveningIfSpec::Primitive(ControllerControlsAtLeast
  { .. })` when `condition_count` is set, always with `FilterController::You` (the one real pattern
  this slice covers, "as long as *you* control...").
- Every existing `ContinuousEffect { .. }` struct literal across the workspace (18 files, ~19 call
  sites) needed a new `condition: None` field — a mechanical ripple confirmed via `cargo test
  --workspace --no-run` catching every site before running anything, same pattern the `up_to` slice's
  own 20-site ripple already established.

**CR requirements:** 611.2c (a continuous effect can be conditional on a state that's checked
continuously, not fixed when the effect starts existing). One concrete condition shape this slice:
"controls N+ matching permanents" — the overwhelmingly common real card-text pattern ("as long as you
control a [type]", "for each — as long as you control three or more [things]").

**TODOs marked, not silently narrowed:**
- `EffectDuration`'s broader inert-duration gap (`UntilYourNextTurn`/`UntilEndOfCombat` never expire)
  — explicitly not touched, per the pre-slice note's own scoping.
- Only one `InterveningIfPrimitive` condition shape now exists for continuous effects (life-total and
  control-count); hand size, "still on the battlefield," and other board-state shapes stay follow-ups
  — same pre-existing TODO on that enum, now shared by both its consumers (trigger intervening-if and
  continuous-effect condition).
- `ControllerControlsAtLeast` reads raw `GameObject.controller`, not `layers::effective_controller` —
  named above, not a silent gap; revisit once the `phase5/layer-engine-control` migration (backlog)
  actually threads effective control into live gameplay consumers.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 669 total, 0 failed, including:
  - 4 new unit tests in `types/ability.rs` (`holds()` directly: count exactly met; below the count;
    drops the instant a matching permanent leaves state.objects, re-derived fresh; a `you`-controller
    filter ignores an opponent's matching permanents).
  - 6 new integration tests in `crates/engine/tests/phase5_conditional_continuous_effects.rs`: a
    conditional anthem applies when the control count is met / doesn't when it isn't / stops applying
    the instant a matching permanent leaves (no cache to invalidate); an unconditional effect
    (`condition: None`) is completely unaffected; the `Game.InterveningIf.ControlsAtLeast`/
    `:SetCondition` Lua round-trip end to end through `run_card_script`; a `Script`-backed
    `:SetCondition(function...)` condition resolving via `invoke_intervening_if`.
  - 1 new TOML scenario fixture (`phase5_conditional_continuous_effects.toml`), same indirect-
    assertion technique `phase5_layer_engine_basics.toml`'s unconditional anthem fixture already uses
    (the scenario DSL has no direct P/T assertion — CR 611.2c's live-recompute condition is validated
    indirectly via CR 704.5g's lethal-toughness SBA: a bear the anthem *would* save from 2 marked
    damage stays lethal and dies here, since the fixture's single artifact never meets the anthem's
    2-artifact condition).
  - all prior tests unaffected.

**STRUCT DEVIATIONS:** `InterveningIfPrimitive`/`InterveningIfSpec`/`InterveningIfTag` dropped `Copy`
(anticipated in the pre-slice note, but not spelled out as a struct deviation there) — see above for
the two real call-site ripples that caused. `ContinuousEffectSpec` gained two fields
(`condition_card_types`/`condition_count`) not named in the pre-slice note at all.

**UNRESOLVED (named during this slice, not silently narrowed):**
- "N or more" was already the shape built (not "exactly N" or "N or fewer") — no further variable-
  count gap here, unlike the `up_to`/targeting slice's own separate "N or more" targeting gap.
- Conditional effects targeting `Layer::ControlChangingEffects` itself could recurse through
  `effective_controller` if this slice's raw-controller choice is ever revisited — flagged above, not
  exercised by any test since no card needs it yet.
- No TOML/CLI support for a `Script`-backed condition (only the `Primitive` `ControlsAtLeast` shape) —
  the scenario format builds `ContinuousEffect` directly as a Rust struct with no script bridge at
  that layer at all (same standing gap `ContinuousEffectSpec`'s own doc comment already named before
  this slice).

## Phase 5: phase5/regeneration slice (CR 614.8/701.19)

Branch `phase5/regeneration`, off master. Picked from the "Known unresolved gaps" list rather than a
numbered backlog item — the full numbered backlog (items 1-17) closed with the previous slice, leaving
only item 16 (planeswalkers/battles/DFCs/split/sagas, an explicit non-goal).

**Pre-slice scoping finding, presented to the human before branching:** this engine's death pipeline
didn't distinguish "destroy" from "any other battlefield → graveyard move." `sba::
collect_dying_creatures` returned one flat `Vec<ObjectId>` covering both CR 704.5f (zero toughness —
NOT destruction) and CR 704.5g (lethal damage — IS destruction) with no tag telling them apart, and
`resolve_deaths` further mixed those with CR 704.5k world-rule losers (also not destruction). Reusing
`EventFilter::WouldDie` unmodified for a regeneration shield would have let it incorrectly save a
0-toughness creature — a real rules bug, not a nitpick. Recommended scope (confirmed by the human
before implementation): wire regeneration into the CR 704.5g SBA path, correctly excluding 704.5f/k,
and defer the scripted `Destroy` op as a follow-up.

**Deviation from the recommended scope — Destroy-op support turned out cheap, included it anyway:**
once the `is_destroy` plumbing was in place, adding the scripted `Destroy` op cost one line (`false` →
`true` at that call site) plus swapping `commit_death` for the new dispatch helper — the pre-existing
CR 616.1 ordering-choice gap on `Destroy` (2+ matching replacement effects silently no-op, a gap that
predates this slice) wasn't made any worse by that, since a lone regeneration shield still resolves
through the ordinary `Resolved` path. Included it rather than leaving free correctness on the table.

**What got implemented:**
- `types/event.rs`: `GameEventKind::Regenerated { object }` — the proposed destroy `ZoneChange` is
  *replaced* with this (CR 614.6's own model: a replaced event never happens, a modified one occurs
  instead), not rewritten into a different zone move — the permanent never leaves the battlefield, so
  no new `ObjectId` is minted (CR 400.7 doesn't apply).
- `types/effect.rs`: `ReplacementPayload::Regenerate` — no new `EventFilter` variant; it's paired with
  the existing `WouldDie` shape. `ReplacementEffect::matches` gained an `is_destroy: bool` parameter
  (not a field on `GameEventKind::ZoneChange` — that would have rippled through the ~60 unrelated
  `ZoneChange` construction sites across the codebase for zero benefit, since nothing but the two real
  destroy producers ever needs to say it) — a `Regenerate`-payload effect only matches when the caller
  says `is_destroy: true`.
- `sba.rs`: new `DyingCandidate { id, is_destroy }` replaces the bare `ObjectId` `collect_dying_creatures`/
  `collect_world_rule_losers`/`resolve_deaths` passed around — `collect_dying_creatures` now tags its
  704.5f branch `false` and its 704.5g branch `true`; `collect_world_rule_losers` always tags `false`.
  New `commit_regeneration` (strip `damage_marked`/`deathtouched`, set `tapped`, remove from
  `state.combat`'s `attackers`/`blocks` — CR 701.19's actual side effects) alongside the existing
  `commit_death`, and a new `commit_death_or_regeneration` dispatcher both `resolve_deaths` and
  `Engine::submit_replacement_choice`'s SBA-resume branch call instead of `commit_death` directly.
- `engine.rs`: `is_destroy` threaded through the *entire* CR 616.1 ordering-choice pause/resume chain —
  `ReplacementResumeTarget::SbaDeath` gained its own `is_destroy` field (the currently-paused event's
  tag, since it's already popped off `pending_deaths` by the time the pause fires), `sba::
  SbaOutcome::NeedsOrderingChoice`/`sba::DeathResolution::NeedsOrderingChoice` both carry it alongside
  their own `pending_deaths: VecDeque<DyingCandidate>`. `Sacrifice`/`Exile` script ops pass
  `is_destroy: false` (neither is destruction — confirmed by reading each site, not assumed); `Destroy`
  passes `true` (see the scope-widening note above); the generic command-events `ReplacementCheck`
  pipeline and its own resume always pass `false` (nothing routed through it is ever a destroy
  proposal — the two real destroy producers each run their own inline replacement check instead,
  unchanged from before this slice).
- `CR 704.5j` legend-rule losers (`submit_legend_rule_choice`) also tagged `is_destroy: false` — CR
  704.5j is "put into its owner's graveyard," not "destroy," same as 704.5f/k.

**CR requirements:** 614.8 (regeneration is a destruction-replacement effect), 701.19a (spell/ability-
sourced shield: one-shot, "the next time... this turn") and 701.19b (static-ability-sourced shield:
repeatable, reapplies every time) — both map directly onto the pre-existing `ConsumptionPolicy::
OneShot`/`Repeatable` split, no new machinery needed for that distinction. 701.19c ("can't be
regenerated" stops a shield from being *created*, not just from applying) is named but not modeled —
see UNRESOLVED.

**TODOs marked, not silently narrowed:**
- CR 701.19c ("can't be regenerated") — no mechanism exists to prevent a shield from being created at
  all; this engine has no equivalent of a keyword/flag for it yet.
- No `Game.Ops.Regenerate`/Lua builder for creating a shield — same standing "no Lua builder for
  replacement effects" gap every `ReplacementEffect` already has (`replacement.rs`'s own doc comment).
  Shields built via Rust struct literal only this slice, matching every existing replacement-effect
  test.
- The pre-existing CR 616.1 ordering-choice gap on the scripted `Destroy` op (2+ matches silently
  no-op rather than pausing) is unaffected by this slice — not made worse, not fixed either.
- No "whenever this regenerates" trigger condition — real but rare oracle text (e.g. Broodhatch
  Nantuko); `GameEventKind::Regenerated` ships with no matching `ConditionPrimitive` yet.

**VERIFICATION RESULT:** Pass. `cargo test --workspace` — 678 total, 0 failed, including 9 new tests in
`crates/engine/tests/phase5_regeneration.rs`: a `OneShot` shield saves a creature with lethal damage
(damage cleared, tapped, same `ObjectId`); the same shield does NOT save a second lethal hit
afterward (consumed); a `Repeatable` shield survives multiple lethal hits; a 0-toughness creature is
NOT saved by a shield (the core rules nuance this slice exists to get right); a regenerating attacker
is removed from `state.combat`; an unshielded creature is destroyed normally (regression); two
matching replacement effects (one a regeneration shield, one a plain `WouldDie` exile effect) still
pause for a real CR 616.1 ordering choice — both at the `sba::check_state_based_actions` unit level and
through the full `Engine` pause/resume pipeline (`AwaitingReplacementChoice` →
`submit_replacement_choice`), proving the `is_destroy` threading survives a real pause/resume round
trip; a scripted `Destroy` spell is intercepted by a regeneration shield the same way SBA-driven
destruction is. All prior tests (669 before this slice) unaffected.

**STRUCT DEVIATIONS:** `sba::DyingCandidate` (new, `pub` — not anticipated as a *public* type in the
scoping report, but had to be: `SbaOutcome::NeedsOrderingChoice`/`DeathResolution::NeedsOrderingChoice`
are both already `pub`/`pub(crate)`-visible to integration tests, so their `pending_deaths` field's
element type needed the same visibility, caught by the compiler rather than guessed at). `sba::
DeathResolution::NeedsOrderingChoice`, `sba::SbaOutcome::NeedsOrderingChoice`, and `engine.rs`'s
`ReplacementResumeTarget::SbaDeath` each gained an `is_destroy: bool` field, exactly as scoped going
in.

**INTERFACE DEVIATIONS:** `replacement::apply_replacements`/`apply_replacements_from`/`ReplacementEffect::
matches` all gained an `is_destroy: bool` parameter — anticipated. `sba::resolve_deaths`/`Engine::
resume_sba_death` changed their queue parameter from `VecDeque<ObjectId>` to `VecDeque<DyingCandidate>`
— anticipated. `sba::commit_death_or_regeneration` (new) — not named in the pre-slice report, which
only mentioned `commit_regeneration`; added once it became clear both `resolve_deaths` and the SBA
ordering-choice resume branch needed the same dispatch logic, to avoid duplicating it.

**SCOPE DEVIATIONS:** the scripted `Destroy` op (`engine.rs`) — see the "turned out cheap" note above.
Everywhere `apply_replacements` is called needed its own explicit `is_destroy` decision (`Sacrifice`/
`Exile`/the generic `ReplacementCheck` pipeline all `false`; `Destroy` `true`) — a mechanical but
real ripple through every one of that function's ~6 real call sites, not just the two that actually
produce `true`.

**UNRESOLVED:** see the TODOs above — CR 701.19c, no Lua builder, the pre-existing Destroy ordering-
choice gap, and no regeneration trigger condition. Also: `ConsumptionPolicy::LimitedUses(n)` was never
exercised by this slice's own tests (only `OneShot`/`Repeatable`) — real Magic has no "regenerate N
times" card text, so this wasn't a gap worth manufacturing a test for, just noting the combination is
untested rather than implying it's broken.

## Phase 5: phase5/destroy-ordering-choice slice (CR 616.1)

Branch `phase5/destroy-ordering-choice`, off master. Picked from the "Known unresolved gaps" list
rather than a numbered backlog item — the numbered backlog (1-17) closed with the previous slice,
leaving only item 16 (planeswalkers/battles/DFCs/split/sagas, an explicit non-goal) unstarted.

**Pre-slice scoping finding:** the scripted `Destroy`/`Sacrifice`/`Exile` ops (`engine.rs`'s
`ResolveStackTop`) each loop over their legal targets independently, proposing+replacement-checking+
inline-committing each target's own `ZoneChange` (a `ZoneChange` always describes an already-happened
move — the same "already happened" convention every scripted zone-move op uses, unlike `Damage`/
`Counters`' "push an unapplied event" shape). Before this slice, a target whose proposed event matched
2+ `ReplacementEffect`s hit `ReplacementOutcome::NeedsOrderingChoice` and the arm's own `{ .. }` catch
silently did nothing — the target simply never moved, no pause, no player choice raised at all, unlike
every SBA-driven death (already correctly paused via `ReplacementResumeTarget::SbaDeath`, wired up back
in `phase5/replacement-prevention-effects`). Confirmed via the actual `regeneration.rs`/`sba.rs` code
before branching, not assumed from the doc note alone.

**What got implemented:**
- `engine.rs`: new `ScriptedZoneMoveKind` enum (`Destroy`/`Sacrifice`/`Exile`) and a new
  `ReplacementResumeTarget::ScriptedZoneMove { kind, remaining: VecDeque<ObjectId>, stack_id }` variant,
  the same shape role `SbaDeath`'s own `pending_deaths` queue plays for SBA-driven death.
- Two new `Engine` methods replace the three inline per-target loops: `continue_scripted_zone_move`
  drains the target queue one at a time (propose → `apply_replacements` → commit), returning a real
  `ProcessState::AwaitingReplacementChoice` the instant any target needs one rather than silently
  skipping it; `commit_scripted_zone_move` holds the per-kind commit logic (`commit_death_or_
  regeneration` for Destroy, `commit_death` for Sacrifice, the inline shared-Exile-zone move for Exile)
  shared between the fresh-resolution path and the post-choice resume path, unchanged behavior from
  before this slice, just extracted so both paths call the same code.
- `submit_replacement_choice` gained a `ScriptedZoneMove` resume arm: applies the chosen effect,
  commits the resolved event via `commit_scripted_zone_move`, then calls back into
  `continue_scripted_zone_move` to keep draining the rest of that op's own queue — re-pausing again if
  the *next* independent target also needs an ordering choice.
- The three `ResolveStackTop` match arms (`Sacrifice`/`Destroy`/`Exile`) now just collect legal targets
  into a `VecDeque<ObjectId>` and hand off to `continue_scripted_zone_move`, returning
  `StepStatus::Progressed` — all the indestructibility/battlefield-presence/is_destroy logic that used
  to live inline in each arm moved into the shared helper, one copy instead of three.

**CR requirements:** 616.1 (2+ replacement/prevention effects matching one event → affected player
chooses application order) — now honored for all three scripted zone-move ops, not just SBA-driven
death.

**A dead end, not a fix:** while writing this slice's own multi-target test, a fixture with a broad
`controller: None` "exile it instead" effect on one target incidentally intercepted a *different*,
unrelated creature's own independent death in the same op — a test-fixture bug (the effect needed
`controller: Some(FilterController::You)` to only protect its own controller's creatures), not an
engine bug. Chasing that symptom down a different path also surfaced what looked like a real bug in
`sba::commit_death` (its `zone_mut(to, Some(owner))` call looks like it can't ever resolve a
replacement-redirected `to: Exile`, since Exile is meant to be a shared zone) — attempting that fix
broke an existing, previously-passing `phase5_dies_triggers.rs` test, revealing `commit_death`'s
`Some(owner)` lookup is this codebase's *actual*, if debatable, established convention for Exile
(distinct from the dedicated scripted-`Exile`-op commit path's own `owner: None` lookup for the same
zone kind — two different conventions for the same zone, pre-existing and orthogonal to this slice's
own scope). Reverted the `sba.rs` change and fixed the test fixture instead, once the fixture bug (not
an engine bug) was isolated as the actual root cause.

**TODOs marked, not silently narrowed:**
- Exile's own ordering-choice arm is fixed by the same refactor but isn't exercised end-to-end by any
  test: `EventFilter` only has two shapes today (`DamageDealt`, `WouldDie`), and `WouldDie` only ever
  matches a proposed `ZoneChange { from: Battlefield, to: Graveyard, .. }` — a scripted Exile's own
  proposed `to: Exile` can never match either shape, so no fixture can currently construct 2
  replacement effects that both match an Exile op's proposed event. Not a gap this slice introduces or
  needs to close (same standing "no Lua builder for replacement effects, only two `EventFilter` shapes"
  gap already named in this doc's "Known unresolved gaps" list) — noted in the test file's own module
  doc so the missing Exile-specific test isn't mistaken for an oversight.
- The Exile-zone-ownership inconsistency surfaced above (`commit_death`'s `Some(owner)` vs. the
  dedicated Exile-op commit's `owner: None`, for what should be the same shared zone) is real and
  worth a future look, but reconciling it would ripple through zone setup in test fixtures/scenario
  loading well beyond this slice's own scope — left as-is, not fixed here.

**Tests added** (`crates/engine/tests/phase5_destroy_ordering_choice.rs`, 5 new tests): a scripted
`Destroy` with 2 matching replacement effects (a regeneration shield + an "exile instead" effect) now
pauses instead of silently no-oping (the exact bug this slice fixes); choosing the regeneration shield
saves the creature; choosing the other effect first lets *it* win instead (proves the order genuinely
determines the outcome, not just that resolution completes); a 2-target `Destroy` pauses on one target
and still destroys the second, independent target afterward (the core of the fix — the remaining queue
keeps draining past a pause, it doesn't stop at the first one); and the same pause/resume/consumption
proof for the scripted `Sacrifice` op (which can never intercept via regeneration — `is_destroy: false`
always — so its own test uses two distinct "exile instead" effects instead, distinguished by which one
gets consumed).

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 5 new tests; all prior
tests (678 before this slice) unaffected.

**STRUCT DEVIATIONS:** `ScriptedZoneMoveKind` (new, `pub`) and `ReplacementResumeTarget::
ScriptedZoneMove` (new variant) — both anticipated going in, matching the pre-slice scoping report.

**SCOPE DEVIATIONS:** none beyond the dead-end investigation noted above (which left no trace in the
final diff — `sba.rs` is unchanged from before this slice).

**UNRESOLVED:** see the TODOs above — Exile's own untested ordering-choice arm, and the pre-existing
Exile-zone-ownership inconsistency between `commit_death` and the dedicated Exile-op commit path.

## Phase 5: phase5/token-cessation slice (CR 111.7/704.5d)

Branch `phase5/token-cessation`, off master. Picked from the "Known unresolved gaps" list — the
numbered backlog (1-17) closed several slices ago, leaving only item 16 (planeswalkers/battles/DFCs/
split/sagas, an explicit non-goal) unstarted.

**Pre-slice scoping finding:** confirmed via `engine.rs`'s `CreateToken` resolution TODO and a read of
`commands::retire_object` — a departing object's *old* id is purged (`state.objects.remove`), but every
death-commit path (`commit_death`/`commit_death_or_regeneration`/the Exile-op inline move) immediately
re-inserts a *new* id for the post-move object in its destination zone, cloned from `last_known_info`
(which carries `is_token` forward). Nothing ever noticed that new object was a token sitting somewhere
other than the battlefield — it just stayed there forever, indistinguishable from a real card.

**What got implemented:**
- `types/event.rs`: `GameEventKind::TokenCeased { object }` — no new `ObjectId` minted (nothing to move
  to; the object just stops existing), unlike every real death this engine already models.
- `sba.rs`: new `collect_ceasing_tokens` (scans every zone except Battlefield for `GameObject.is_token`)
  and `commit_token_cessation` (removes it from its zone's contents, purges it via
  `commands::retire_object`, produces the ledger event) — wired into the very top of
  `check_state_based_actions`, before that same pass's own `collect_dying_creatures`/`resolve_deaths`
  run. Deterministic (no player choice), so its events/departed-abilities fold directly into the same
  accumulators the rest of the function already threads through.
- Timing (the actual point of this slice): CR 704.3's "simultaneous, based on the state as of this
  check" means a token dying via *this* pass's own SBA collection isn't "in a zone other than the
  battlefield" until *after* this pass ends — running the scan first, against zones as they stand at
  the top of the call, defers cessation to the *next* pass for free, satisfying CR 111.7's own
  parenthetical ("applicable triggered abilities will trigger before the token ceases to exist") without
  any special-casing. Verified directly: a token with lethal damage marked dies on pass 1 and is still a
  live object after that same call returns; only a second `check_state_based_actions` call (the next
  pass) ceases it.

**CR requirements:** 704.5d (token cessation, primary), satisfying 111.7's own timing note. CR 111.8 (a
token that already left the battlefield can't move zones again — "remains in its current zone instead")
is a related but separate rule, not covered.

**TODOs marked, not silently narrowed:**
- CR 111.8 — no mechanism stops an already-off-battlefield token from being targeted by a return-to-
  hand/battlefield effect; would need a check at every zone-change command site, out of scope here.
- CR 704.5e (copies of spells/cards in the wrong zone ceasing) — a separate, related SBA; tokens ⊄
  spell/card copies, not touched by this slice.
- No trigger condition keys off `TokenCeased` itself (same "ships with no matching `ConditionPrimitive`
  yet" precedent `GameEventKind::Regenerated` set) — real but rare oracle text.
- A pre-existing, unrelated gap surfaced along the way (not fixed, not this slice's to fix):
  `resolve_deaths`'s own `NeedsOrderingChoice` arm already drops any `departed_triggered` accumulated
  before it pauses (documented at that call site before this slice). This slice's own token-cessation
  departed-abilities ride the same accumulator, so they're vulnerable to the exact same pre-existing
  drop in the same rare interleaving (a token ceasing in the same pass some *other* death needs a CR
  616.1 ordering choice, and that token happens to have a real "when this leaves play" ability). Not hit
  by this slice's own tests; flagged at the call site, not fixed, same precedent that gap's own comment
  already set.

**Tests added** (`crates/engine/tests/phase5_token_cessation.rs`, 5 new tests): a token with lethal
damage dies on one SBA pass and only ceases on the next (not the same one); a non-token creature with
the same fate never ceases, even run out to the fixed point; a token already sitting in Exile (never
touched the battlefield this pass) ceases on the very first check — proving the rule isn't graveyard-
specific; a token's own "Dies" trigger still fires and resolves through the full `Engine` pipeline
despite the token ceasing a pass later (proving trigger-matching's ledger/LKI-based design, not a live
`state.objects` lookup, is unaffected); a non-token creature stays in the graveyard through the same
full pipeline (regression).

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 5 new tests; all prior
tests unaffected.

**STRUCT DEVIATIONS:** none — `GameEventKind::TokenCeased` matches the pre-slice scoping exactly.

**INTERFACE DEVIATIONS:** none — `collect_ceasing_tokens`/`commit_token_cessation` are both new,
private, and match the scoping report's shape.

**SCOPE DEVIATIONS:** none.

**UNRESOLVED:** see the TODOs above — CR 111.8, CR 704.5e, no `TokenCeased` trigger condition, and the
pre-existing `NeedsOrderingChoice`-drops-departed-abilities gap this slice's own token cessation now
also rides (not worsened, not fixed).

## Phase 5: phase5/effect-duration-expiration slice (CR 511.2/500.5a, CR 723.1-style)

Branch `phase5/effect-duration-expiration`, off master. Picked from the "Known unresolved gaps" list —
`EffectDuration::UntilYourNextTurn`/`UntilEndOfCombat` existed on the enum but nothing ever drained
them; only `UntilEndOfTurn` had a real drain point (Cleanup).

**Pre-slice scoping finding:** `WhileSourceOnBattlefield`/`UntilSourceLeaves` turned out not to be part
of this gap at all — every zone change in this engine mints a fresh `ObjectId` and purges the old one
(the CR 400.7 fix's own precedent), so `layers::applicable_effects_in_layer`'s existing
`state.object(e.source)` check already treats "source's original id no longer resolves" as
inapplicable, which is exactly what those two durations mean. Only `UntilYourNextTurn`/
`UntilEndOfCombat` genuinely had no drain point.

Also confirmed: no card/script path creates a `ContinuousEffect` with either duration yet
(`StaticAbilityBuilder::Register` hardcodes `Permanent`; `script_api::add_continuous_effect` is an
unrelated unbound `todo!()` stub). This slice closes the drain mechanism only, exercised via
directly-constructed `ContinuousEffect`s pushed straight into test state — same precedent
`phase5_conditional_continuous_effects.rs` already set.

**What got implemented (`engine.rs`, `advance_turn_step`):**
- CR 511.2/500.5a: the `(Phase::Combat, Step::EndOfCombat)` match arm (already the exact "combat
  phase ends" transition, clearing `state.combat`) now also retains-out any `UntilEndOfCombat` effect.
  Expires at the end of the combat phase itself, not the beginning of the End of Combat step — verified
  directly: the effect is still present after only the first of two players has passed priority at that
  step (still mid-step), and gone only once the transition to `PostCombatMain` actually happens.
- CR 723.1-style "until your next turn": the `(Phase::Ending, Step::Cleanup)` match arm (already where
  `active_player` rotates to the next player in turn order) now also retains-out any
  `UntilYourNextTurn` effect whose source's *current* controller equals the *new* active player.
  Reasoning locked in during scoping: turn order cycles through every player in a fixed sequence, so
  the first time `active_player` becomes a given player again after the effect was created **is**
  that player's next turn by definition — no extra "turns since creation" bookkeeping needed. Verified
  both directions: an effect created on its own controller's turn survives the very next cleanup
  (active player becomes someone else) and only drains once cleanup rotates back to the controller.

**TODOs marked, not silently narrowed:**
- No Lua-facing (or Rust-facing) way to actually create a temporary continuous effect exists yet —
  `StaticAbilityBuilder` is setup-time-only and permanent-duration-only; this slice only makes the
  *expiration* side of the enum real. A spell-resolution-time "add a temporary continuous effect"
  builder is still unbuilt.
- A `UntilYourNextTurn` effect whose `source` no longer resolves (e.g. sourced by a spell that already
  left the stack for the graveyard — which, per the zone-change-mints-new-id pattern above, is the
  *normal* case for anything actually cast, not an edge case) can never be matched to a controller and
  is left inert in `state.continuous_effects` forever rather than drained. Same category as the
  pre-existing "Layer 2 control" and "script effects don't participate in dependency" notes already in
  the gaps list — flagged, not fixed. Directly covered by its own test
  (`until_your_next_turn_effect_with_unresolvable_source_is_left_inert_rather_than_drained`) so the
  behavior is at least deliberate and pinned, not silently wrong.
- CR 724.2d (an effect that ends the combat phase early, skipping straight past the End of Combat step)
  isn't modeled anywhere in this engine, so that path couldn't be exercised — `UntilEndOfCombat` only
  ever drains via the normal `EndOfCombat -> PostCombatMain` transition today.

**Tests added** (`crates/engine/tests/phase5_effect_duration_expiration.rs`, 6 new tests): an
`UntilEndOfCombat` effect survives mid-step after only one player has passed and drains exactly when
the combat phase ends, with `Permanent`/`UntilEndOfTurn` effects unaffected by that same transition; an
`UntilYourNextTurn` effect survives cleanup into the opponent's turn and drains only once the turn
cycles back to its controller; the same duration with an unresolvable source is left inert rather than
drained (regression-pinning the known limitation above); an `UntilYourNextTurn` effect is untouched by
the unrelated `EndOfCombat` drain.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 6 new tests; all prior
tests unaffected.

**STRUCT DEVIATIONS:** none — no fields added to `EffectDuration`/`ContinuousEffect`.

**INTERFACE DEVIATIONS:** none — both drains are inline `retain()` calls in already-existing
`advance_turn_step` match arms, no new functions.

**SCOPE DEVIATIONS:** none — both edits landed exactly in the two match arms the scoping report named.

**UNRESOLVED:** see the TODOs above — no creation path for temporary continuous effects yet, the
unresolvable-source-leaves-it-inert limitation, and CR 724.2d's early-combat-end interaction being
unexercisable since that mechanic doesn't exist in this engine.

## Phase 5: phase5/pump-effect slice (CR 611.2a, `AbilityEffect::Pump`)

Branch `phase5/pump-effect`, off master. Picked as the direct follow-up to `phase5/effect-duration-
expiration`'s own UNRESOLVED note: that slice built real drain paths for `EffectDuration::
UntilYourNextTurn`/`UntilEndOfCombat`, but confirmed nothing could actually create an effect with
either duration — `StaticAbilityBuilder` is setup-time/permanent-only, and `script_api::
add_continuous_effect` is an unrelated, unbound `todo!()` stub. This slice closes the single most
common real-card shape that needs one: "target creature gets +X/+Y until <duration>" (CR 611.2a).

**Scoping finding, locked in before writing code:** a fully generic "create any temporary continuous
effect from a resolving spell" builder was rejected as over-scoped — every existing `AbilityEffect`
primitive (`Damage`, `Destroy`, `Counters`, ...) is its own narrow, concrete shape resolved by
`ResolveStackTop`, not a generic "apply any effect" mechanism, so `Pump` follows that same precedent
rather than introducing a new one.

The harder design question was targeting: `EffectFilter.self_only` was defined (by
`phase5/layer-engine-*`) as "the affected object is `ContinuousEffect.source`" — modeling an aura's own
attachment target, since `source` up to now always meant *the permanent whose own ability generated the
effect*. For Pump, the affected object is the spell's *chosen target*, not the spell itself. Rather than
add a new filter shape, this slice reuses the exact same mechanism: the pushed `ContinuousEffect`'s
`source` field is set to the *target's* `ObjectId` with `self_only: true`. Bonus, not a coincidence:
since every zone change mints a fresh `ObjectId` (the standing precedent this whole area leans on), a
Pump effect automatically can't outlive whatever zone its target occupied when the effect was created —
free correctness, no extra bookkeeping, for the same reason `WhileSourceOnBattlefield`/
`UntilSourceLeaves` already work for free elsewhere.

**What got implemented:**
- `types/ability.rs`: new `AbilityEffect::Pump { power: i32, toughness: i32, duration:
  EffectDuration }`.
- `types/effect.rs`: `EffectDuration` gained `#[derive(Clone, Debug)]` — needed once a value of that
  type had to live inside `AbilityEffect`, which already derives both.
- `engine.rs` (`ResolveStackTop`'s effect match): a new arm mirroring `Counters`'s "loop over every
  legal `Target::Object`, mutate state immediately" shape (no `GameEventKind` exists for "a continuous
  effect was added" — static-ability registration isn't ledger-logged either, same convention). One
  `SequenceNo` is allocated per resolution and shared across every target's own pushed effect (CR
  613.7a: simultaneous effects from the same resolution).
- `script.rs`: `Game.Ops.Pump(power, toughness, duration)` and a new `Game.Durations.*` tag table
  (`UntilEndOfTurn`/`UntilYourNextTurn`/`UntilEndOfCombat`/`Permanent` only — see `DurationTag`'s own
  doc comment for why `WhileSourceOnBattlefield`/`UntilSourceLeaves`/`WhileConditionHolds`/`Custom`
  aren't exposed here). Not exercised by this slice's own tests, same "wired for future real cards, but
  the actual test coverage goes through `CardDefinition.effect`'s Rust-native fast path" precedent
  every other `AbilityEffect` primitive's own test file already follows (see `phase5_script_ops_
  damage.rs`).
- `commands.rs`: added the new variant to `ActivateAbility`'s existing non-mana-effect match arm (same
  path `Damage`/`Counters`/etc. already take).

**TODOs marked, not silently narrowed:**
- No "divide as you choose" (Pump could theoretically apply a different amount per target, real oracle
  text almost never does) — same CR 601.2d stance `Damage`/`Counters` already take, not re-litigated
  here.
- The general "add any temporary continuous effect" builder (arbitrary layer/effect/filter, not just
  P/T) is still unbuilt — deliberately deferred per this slice's own scoping finding above, not an
  oversight.

**Tests added** (`crates/engine/tests/phase5_pump_effect.rs`, 4 new tests, harness copied from
`phase5_script_ops_damage.rs`): a cast Pump spell boosts its target's effective P/T immediately on
resolution; the boost is pinned to the actual target, not any other permanent on the battlefield; an
`UntilEndOfTurn` Pump expires by the start of the next turn (proving this slice's creation path and the
prior slice's drain path connect end to end); a `Permanent`-duration Pump survives the same turn
transition.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 4 new tests; all prior
tests unaffected.

**STRUCT DEVIATIONS:** `EffectDuration` needed `#[derive(Clone, Debug)]` added — not named in advance
(this slice wasn't run through phases 1-4 separately), but a mechanical consequence of embedding it in
`AbilityEffect`, not a design change.

**INTERFACE DEVIATIONS:** none beyond the new variant/bindings themselves — no existing function
signature changed shape, only `commands.rs`'s already-open-ended non-mana-effect match arm gained one
more pattern.

**SCOPE DEVIATIONS:** none — every edit landed exactly where the scoping finding above said it would
(one `AbilityEffect` variant, one `ResolveStackTop` arm, one Lua binding pair, one match-arm addition).

**UNRESOLVED:** see the TODOs above — no generic temporary-effect builder, and per-target division of
the pump amount isn't modeled (same standing gap `Damage`/`Counters` already carry).

## Phase 5: phase5/discarded-trigger slice (CR 603.10-style, `ConditionPrimitive::Discarded`)

Branch `phase5/discarded-trigger`, off master. Picked from the "Known unresolved gaps" list's Discard
bullet — which turned out to be stale before any code was written: `ConditionPrimitive::DiscardsCard`/
`EachDiscardsCard` ("whenever you/a player discards *a* card") were already closed by the earlier
`phase5/trigger-completeness-cleanup` slice, for both discard sources. What was actually still missing
is the self-referential shape real oracle text also uses: "when *this* card is discarded" — no
condition matched "the discarded object is this ability's own source," the same way `Dies`/
`LeavesBattlefield` already do for a battlefield departure.

**What got implemented:**
- `types/ability.rs`: new `ConditionPrimitive::Discarded`, matching a `ZoneChange{from: Hand, to:
  Graveyard, last_known_info: Some(lki), ..}` event where `lki.object_id == source` — byte-for-byte the
  same shape `Dies` already uses, just Hand→Graveyard instead of Battlefield→Graveyard.
- `triggers.rs`: `collect_triggers`'s departed-ability LKI fallback loop only let `Dies`/
  `LeavesBattlefield` through (a source that no longer resolves live needs this special path — every
  other condition shape assumes a live source). Added `Discarded` to both of that loop's match arms
  (the plain and `PrimitiveOncePerTurn` cases) — this was the one actual reachability gate; without it,
  the new variant would exist but never fire.
- `script.rs`: `Game.Conditions.Discarded` binding, alongside the existing `Dies`/`LeavesBattlefield`/
  `DiscardsCard` ones.
- `commands.rs`: fixed a stale doc comment on `move_to_graveyard_for_cost` that still claimed "no
  `ConditionPrimitive` shape matches a Hand-departure LKI yet" — true before the `DiscardsCard` fix,
  false since.

**CR grounding, stated honestly:** the look-back-in-time mechanism is CR 603.10, but 603.10a's own
enumerated list doesn't explicitly name "leaves the hand" among its categories (it lists leaves-the-
battlefield, sacrifice, leaves-a-graveyard, and object-all-players-can-see-enters-hand/library). This
extends the same mechanism by symmetry with `Dies`/`LeavesBattlefield`, not a chapter-and-verse CR
match — noted rather than over-cited.

**TODOs marked, not silently narrowed:** none new — both discard paths already stash their departed
`Triggered` abilities (closed by the earlier `DiscardsCard` fix), so there was no plumbing gap left to
flag here, only the condition-matching gap this slice closes.

**Tests added:**
- `types/ability.rs` unit tests (3 new, mirroring `Dies`'s own): `Discarded` matches a Hand→Graveyard
  move by last-known object id; doesn't match a different card's discard; doesn't match a battlefield
  death (proving it's not just `Dies` misfiring on shared last-known-info shape).
- `crates/engine/tests/phase5_discarded_trigger.rs` (3 new): a `Discarded` trigger fires off a cost-
  payment discard (activating an ability with a `Discard` additional cost, paid with the watching card
  itself) and resolves for real (`GainLife(1)` applied); fires off the cleanup hand-size discard the
  same way; does *not* fire when a *different* card from the same hand is discarded instead (proving
  identity, not just zone-shape, gates it).

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 6 new tests (3 unit + 3
integration); all prior tests unaffected.

**STRUCT DEVIATIONS:** none — no fields added, only a new unit variant.

**INTERFACE DEVIATIONS:** none.

**SCOPE DEVIATIONS:** none — landed in exactly the three files (plus one stale-comment fix) the scoping
report named.

**UNRESOLVED:** none new. `draw_step`'s library-side LKI stash remains the one still-dormant sibling
(practically unreachable, no card needs it) — unaffected by this slice, same standing note.

## Phase 5: phase5/max-hand-size slice (CR 402.2, `HandSizeModifier`)

Branch `phase5/max-hand-size`, off master. Picked from the "Known unresolved gaps" list's CR 402.2
bullet — `Engine::begin_cleanup_step` had a bare hardcoded `MAX_HAND_SIZE: usize = 7`, consulting
nothing, so no "you have no maximum hand size"/"your maximum hand size is N" effect could change
cleanup behavior.

**Scope discovery, flagged before writing code:** every one of this project's ~85 test fixture files
constructs `GameState` as a full struct literal (no `Default`/spread). The clean design — a dedicated
`HandSizeModifier` list mirroring `cost.rs`'s already-established `CostModifier` precedent, rather than
squeezing a player-level rule into the CR-613-only `Layer` enum or `state.continuous_effects` (which
would break `Layer`'s own "exactly the CR 613.1 layer list, nothing else" invariant) — meant adding a
field to `GameState`, so every one of those ~85 literals needed it added. Done mechanically via a
scripted insertion right after each file's own `cost_modifiers: Vec::new(),` line, then the compiler
caught the one file the script's pattern didn't cover (`crates/scenario/src/lib.rs`, a different
initializer shape) as a hard `E0063` missing-field error - exactly the "self-correcting" safety net that
made this large-footprint mechanical change low-risk despite its size.

**What got implemented:**
- `types/player.rs`: `HandSizeEffect { NoMaximum, SetTo(u32) }` + `HandSizeModifier { source,
  controller, affected: FilterController, effect, timestamp }` — `controller`/`affected` mirror
  `CostModifier`'s own `controller`/`filter.controller` split exactly (whose "you"/"opponent"
  perspective, and who's actually affected). `effective_max_hand_size(modifiers, player, base) ->
  Option<u32>` — `None` means "no maximum." Multiple applicable modifiers don't stack; the one with the
  latest `timestamp` wins outright, the same CR 613.7-style "latest wins" convention this engine's flat-
  override layer effects (e.g. `SetPT`) already use for a conflict, even though hand size isn't a CR 613
  layer at all (no dependency/613.8 analysis needed for the same reason those flat overrides don't need
  it either).
- `types/game.rs`: new `pub hand_size_modifiers: Vec<HandSizeModifier>` field.
- `engine.rs`: `begin_cleanup_step` calls `effective_max_hand_size` instead of the hardcoded literal; a
  `None` result skips the discard check entirely (CR 402.2's "no maximum" case) rather than computing
  against some sentinel number. The `ProcessState::AwaitingDiscard` resume arm (re-displays the pause
  after a step that made no progress) recomputes the same way, rather than re-hardcoding its own `7` -
  it was a second, independent hardcoded site the scoping investigation surfaced.

**TODOs marked, not silently narrowed:** no `Game.CreateHandSizeModifier` Lua builder this slice — same
"direct Rust struct-literal fixtures only" bootstrap stance `CostModifier` itself took before any script
builder existed for it.

**Tests added** (`crates/engine/tests/phase5_max_hand_size.rs`, 4 new tests): a `NoMaximum` modifier
suppresses the cleanup discard pause entirely even with a 20-card hand; a `SetTo(10)` modifier changes
the `AwaitingDiscard` threshold from the default 7; two conflicting modifiers (`SetTo(10)` then
`NoMaximum`, later timestamp) resolve to the later one; an `Opponent`-scoped modifier doesn't affect its
own controller's cleanup (still discards to the default 7).

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 4 new tests; all ~85
mechanically-updated fixture files and every prior test unaffected.

**STRUCT DEVIATIONS:** none beyond the new field/types themselves, which the scoping report already
named up front as the size-defining consequence of this slice's design choice.

**INTERFACE DEVIATIONS:** none — `effective_max_hand_size` is new, not a signature change to anything
existing.

**SCOPE DEVIATIONS:** one file outside `crates/engine` (`crates/scenario/src/lib.rs`) needed the same
field addition — not named in the scoping report (which only anticipated `crates/engine`'s own
fixtures), caught immediately by the compiler rather than discovered later.

**UNRESOLVED:** none new.

## Phase 5: phase5/cant-be-regenerated slice (CR 701.19a/c, `AbilityEffect::Regenerate`)

Branch `phase5/cant-be-regenerated`, off master. Picked from the "Known unresolved gaps" list's
regeneration bullet — CR 701.19c ("effects that say a permanent can't be regenerated... cause
regeneration shields to not be applied") had no mechanism at all.

**Scoping finding, changed the shape of this slice:** `phase5/regeneration` built the *drain*/
resolution side of regeneration (`ReplacementPayload::Regenerate`, `commit_regeneration`) but never a
*creation* path — every one of that slice's own tests constructs a `ReplacementEffect` shield directly
via a Rust struct literal, the same "no Lua/Rust creation surface yet" gap `phase5/effect-duration-
expiration` hit before `phase5/pump-effect` built one. CR 701.19c is about stopping shield *creation*,
so it had nothing to attach to until a creation path existed — same shape those two slices had, so this
one absorbed both for the same reason `pump-effect` did: the gate is only provable end-to-end with
something real to gate.

**Design choice:** rather than a new dedicated mechanism, "can't be regenerated" is
`KeywordSet::CANT_BE_REGENERATED` - the same layer-6 (ability-adding/restricting) machinery every other
keyword already uses, checked via the existing `commands::effective_keywords` (which already merges a
printed keyword with anything a `Game.Effects.AddKeyword`-driven continuous effect grants). This means
"creatures your opponents control can't be regenerated"-style effects work for free the moment a card
grants the keyword via a static ability - no new query API needed, unlike `HandSizeModifier`'s own
slice, which genuinely had nowhere existing to live. Direct precedent: `INDESTRUCTIBLE` is checked the
same way, at the same "before even proposing the event" point, not as a property of the event/effect
itself.

**What got implemented:**
- `types/card.rs`: `KeywordSet::CANT_BE_REGENERATED` (bit 15 - the last one that fits in the existing
  `u16` backing type without widening it). `script.rs`'s `parse_keyword_name` gained the matching
  `"CantBeRegenerated"` string mapping (`Game.Effects.AddKeyword("CantBeRegenerated")`), for free.
- `types/ability.rs`: new `AbilityEffect::Regenerate` (CR 701.19a, "Regenerate target creature") -
  mirrors `Pump`'s exact shape: every legal target independently gets its own one-shot
  `ReplacementEffect` (`EventFilter::WouldDie` + `ReplacementPayload::Regenerate`) pushed straight into
  `state.replacement_effects`, pinned via `self_only: true` the same way `Pump` pins its own
  `ContinuousEffect`. CR 701.19c's actual gate: `effective_keywords(...).contains(CANT_BE_REGENERATED)`
  checked per-target before pushing - a matching target gets no shield, silently, while every other
  legal target in the same resolution still gets one independently.
- `engine.rs`/`commands.rs`: `ResolveStackTop` arm + the `ActivateAbility` non-mana-effect match arm,
  both mechanical additions mirroring `Pump`'s own.
- `script.rs`: `Game.Ops.Regenerate()` binding (no arguments, same toggle shape as `Destroy`/
  `Shuffle`) - not exercised by this slice's own tests, same "wired for future real cards" precedent
  `Pump`'s own Lua binding already set.

**CR requirements:** 701.19a (spell/ability-sourced shield creation - the new primitive itself);
701.19c (can't-be-regenerated gates creation, not application - `KeywordSet` check at the moment
`AbilityEffect::Regenerate` would push a shield). 701.19b (static-ability-sourced, repeatable shields)
remains out of scope - no static-ability creation path exists for shields either, unaffected by this
slice, same standing gap `Pump`'s own doc comment already names for `StaticAbilityBuilder`.

**TODOs marked, not silently narrowed:** none new - this slice closed exactly what it set out to,
riding entirely on machinery (`effective_keywords`, the `Pump`-shaped resolution arm) that already
existed.

**Tests added** (`crates/engine/tests/phase5_regenerate_ability.rs`, 3 new tests): a cast Regenerate
spell creates a shield that later saves its target from lethal damage (damage cleared, tapped,
`GameEventKind::Regenerated` logged) when checked via `sba::check_state_based_actions` directly; the
same spell against a `CANT_BE_REGENERATED` target creates no shield at all, yet the spell itself still
fully resolves (stack empties, priority returns normally) - CR 701.19c's core point - and the
now-shieldless creature actually dies to the same lethal damage; the shield is pinned to its actual
target, not an untargeted bystander creature on the same battlefield. All three needed damage marked
*after* casting/resolving, not in the initial state - marking it upfront let the engine's own automatic
SBA pass either fizzle the not-yet-resolved spell (CR 608.2b, invalid target) or kill the creature
before the test's own manual `check_state_based_actions` call had anything left to prove; caught by two
tests actually failing during development, not discovered by inspection.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 3 new tests; all prior
tests unaffected.

**STRUCT DEVIATIONS:** none.

**INTERFACE DEVIATIONS:** none beyond the new variant/bindings themselves.

**SCOPE DEVIATIONS:** none — every edit landed exactly where the scoping finding said it would.

**UNRESOLVED:** CR 701.19b's static-ability-sourced (repeatable) shield creation path remains unbuilt,
same as `StaticAbilityBuilder`'s standing "no Lua/TOML duration path" gap generally. No "divide as you
choose" concern here (Regenerate has no amount to divide).

## Phase 5: phase5/script-error-surfacing slice (script-error swallowing, cross-cutting)

Branch `phase5/script-error-surfacing`, off master. Picked from the "Known unresolved gaps" list's
cross-cutting bullet — a Lua runtime error mid-resolution silently became "condition false"/"didn't
match" at every one of several call sites, with nothing recording that it happened at all. A real
script bug and "the card's ability legitimately didn't apply" were indistinguishable from the ledger
alone.

**Scoping finding, locked in before writing code:** a full sweep (`.unwrap_or(false)`/`.ok().flatten()`
across `engine.rs`/`triggers.rs`/`layers.rs`) found six real script-invocation sites, not the whole grep
hit list — several matches were unrelated `Option` chains, not script calls at all. Of the six real
ones, three live in `layers.rs`'s pure characteristic-query call graph
(`applicable_effects_in_layer`'s intervening-if/affected-filter checks, `resolve_primitive`'s CDA
resolution) - `&GameState`-only functions with no mutation pipeline to log through, called from
`effective_characteristics`'s many callers throughout the engine. Fixing those would mean threading
error-reporting through that entire call graph - a much larger, architecturally different change than
this slice's scope. Named as a deliberate exception rather than silently left unfixed alongside the
three sites this slice actually closes (`engine.rs`'s `ResolveStackTop` intervening-if recheck,
`triggers.rs::collect_triggers`'s three script call sites, all reachable from `Engine`'s own mutation
pipeline).

**What got implemented:**
- `types/event.rs`: `GameEventKind::ScriptError { message: String }` — same free-text-diagnostic shape
  `StateBasedActionApplied` already established (itself still unconstructed anywhere, same "ships with
  no matching case yet" precedent this now rides too). Never itself replacement-checked or a trigger
  condition target.
- `engine.rs`: `ResolveStackTop`'s Script intervening-if arm now matches on the real `Result` — on
  `Err`, logs a `ScriptError` via the existing `log_events` helper before falling back to the same
  conservative "doesn't hold" default as before.
- `triggers.rs`: `collect_triggers`'s signature changed from `Vec<PendingTrigger>` to
  `(Vec<PendingTrigger>, Vec<String>)` — it only has `&GameState`, not `&mut Engine`, so it can't log a
  real event itself; it collects error messages instead, and its one caller
  (`engine.rs`'s `ProcessState::CollectTriggers`) turns them into real `ScriptError` events once the
  loop holding an immutable borrow of `state.ledger` releases it. All three of its own script call
  sites (the live-source condition check, the live-source intervening-if check, the departed-loop
  intervening-if check) now report their errors this way instead of silently defaulting.
- `layers.rs`: no code change, both real script call sites got a doc-comment addition naming this
  slice's own deliberate exception explicitly, cross-referencing each other.
- 18 existing test call sites of `collect_triggers` across 3 test files updated for the new tuple
  return (`let pending = collect_triggers(...)` → `let (pending, _) = collect_triggers(...)`) - a
  mechanical, compiler-caught ripple, not a design change.
- Fixed two unrelated stale doc comments discovered while touching this code: `triggers.rs`'s own
  "Only Dies/LeavesBattlefield" note hadn't been updated when `phase5/discarded-trigger` added
  `Discarded` to the same departed-loop match arms.

**TODOs marked, not silently narrowed:** the `layers.rs` exception above is the one deliberately
unfixed piece of this gap — named, not silently dropped.

**Tests added** (`crates/engine/tests/phase5_script_error_surfacing.rs`, 4 new tests): three unit-level
tests calling `collect_triggers` directly against a real `ScriptRuntime` loaded with an always-erroring
script — a Script condition error, a live-source Script intervening-if error, and a departed-loop
(`Dies`-shaped) Script intervening-if error each fall back to "didn't match"/"doesn't hold" and report
exactly one error message naming which check failed; one full-`Engine` test (mirroring
`phase5_intervening_if.rs`'s own "response spell invalidates the condition between trigger and
resolution" shape) where a script errors only once life drops below a threshold - the trigger queues
cleanly (no error yet, life still high), a response spell drops life below the threshold, and the
resolution-time recheck hits the erroring branch, fizzling the trigger (a real `Fizzled` event, `GainLife`
never applied) with at least one real `ScriptError` event logged alongside it. That last test's
assertion is `>= 1` errors, not an exact count — `collect_triggers` evaluates every ability's
intervening-if against every event regardless of whether that event's own condition matched (a
pre-existing, unrelated characteristic, not something this slice changes), so once life drops, every
subsequent event re-triggers the same erroring branch; discovered by the test actually failing on an
exact-count assertion during development, not by inspection.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none new).
`cargo test --workspace` — all test binaries green, 0 failures, including the 4 new tests; all prior
tests (after the 18 mechanical call-site updates) unaffected.

**STRUCT DEVIATIONS:** none.

**INTERFACE DEVIATIONS:** `triggers::collect_triggers`'s return type changed from `Vec<PendingTrigger>`
to `(Vec<PendingTrigger>, Vec<String>)` — anticipated in the scoping finding, but its 18-call-site test
ripple wasn't sized until the compiler enumerated them.

**SCOPE DEVIATIONS:** two stale doc-comment fixes (`triggers.rs`, noted above) — found while touching
adjacent code, not part of the original scoping report, fixed rather than left for a future slice to
trip over.

**UNRESOLVED:** the `layers.rs` exception (see above) — the one deliberately unfixed piece of the
original gap, named rather than silently dropped.

## Phase 5: phase5/static-regeneration slice (CR 701.19b, `Game.CreateReplacementEffect`)

Branch `phase5/static-regeneration`, off master. Picked from the "Known unresolved gaps" list's
regeneration bullet — CR 701.19b ("if the effect of a static ability regenerates a permanent, it
replaces destruction with an alternate effect *each time* that permanent would be destroyed") was
the one real half of regeneration `phase5/cant-be-regenerated` left standing: that slice built the
CR 701.19a one-shot (`OneShot`) creation path only.

**Scoping finding, before writing code:** `ConsumptionPolicy::Repeatable` and the `EventFilter::
WouldDie`/`ReplacementPayload::Regenerate` shape already existed and were already fully proven by
`replacement.rs`'s own unit tests (and, it turned out, by `phase5_regeneration.rs`'s own
`a_repeatable_shield_survives_multiple_lethal_hits` test) — the only real gap was a creation path.
Grepping every real (non-test) `ReplacementEffect {` construction site confirmed it: exactly one
existed anywhere, `engine.rs`'s `AbilityEffect::Regenerate` arm (`OneShot` only). A regeneration
shield isn't a CR 613 layer effect — it rewrites an event, not a characteristic — so it can't route
through `Game.CreateStaticAbility`/`state.continuous_effects`; `types/effect.rs`'s own doc comment
on `ReplacementSpec::Script` already named the fix as "a natural future sibling to `Game.
CreateStaticAbility`, not built here." This slice is that sibling, deliberately scoped to the one
payload shape it needed to prove: `Game.CreateReplacementEffect` always produces a `Repeatable`
`WouldDie`/`Regenerate` shield. Damage prevention/redirection and non-regenerate zone-destination
substitution stay Lua-inaccessible, named rather than guessed at.

**Correctness finding, made while designing (not discovered by a failing test):** CR 701.19c
("can't be regenerated" effects cause regeneration shields "to not be applied") has to be checked
fresh every time a shield is asked to apply, not just once at creation time the way `phase5/cant-
be-regenerated`'s one-shot path checks it. That worked for a one-shot shield because it's created
and consumed within the same narrow window. A static shield is registered once (e.g. on ETB) and
then expected to keep working "each time" per CR 701.19b's own wording — but the target's
CANT_BE_REGENERATED status can change after that registration. `ReplacementEffect::matches`'s
`WouldDie`/`Regenerate` arm now checks `layers::effective_characteristics(...).keywords` there
directly (types/effect.rs already calls `layers::effective_controller` the same way from
`event_target_matches`, same module boundary, no new dependency direction). The one-shot creation-
time skip in `engine.rs` stays as a cheap optimization; the match-time check is now the
authoritative gate for both paths.

**Second correctness finding, caught by two previously-passing tests actually failing, not by
inspection:** `commands::retire_object`/`purge_granted_abilities` purge `state.abilities`/
`state.continuous_effects` by source but never touched `state.replacement_effects` — harmless
before this slice (every real entry was `OneShot`/`LimitedUses` and self-consumed via
`apply_one_replacement`), a real leak once a `Repeatable` entry could outlive its source. Fixed by
extending both functions' existing `retain` calls, same pattern already established for the other
two lists. This *changed* two existing tests' literal expectations
(`phase5_destroy_ordering_choice.rs`'s `destroy_spell_choosing_the_other_effect_first_lets_it_win_
instead`, `phase5_regeneration.rs`'s `a_shield_does_not_save_a_zero_toughness_creature`) — both had
asserted an unconsumed `OneShot` regeneration shield was "still sitting there," pinned to a bear
`ObjectId` that the very same test had just retired via CR 400.7 (exile-instead / zero-toughness
death). That assertion was validating a symptom of the leak, not the feature either test actually
exists to prove (CR 616.1 ordering / CR 704.5f non-destruction) — updated both to expect the now-
purged, correct count instead of reverting the fix.

**What got implemented:**
- `types/effect.rs`: `ReplacementEffect::matches`'s `WouldDie`/`Regenerate` arm gained the
  match-time `CANT_BE_REGENERATED` check described above. `ReplacementSpec`'s own doc comment
  updated — `Primitive` now has a real (if narrow) Lua construction path.
- `commands.rs`: `retire_object`/`purge_granted_abilities` both gained a
  `state.replacement_effects.retain(|r| r.source != id)` line, doc comments updated to match.
- `script.rs`: `ScriptRegistry.pending_replacement: Vec<ReplacementEffect>` (mirrors
  `pending_static`, no `timestamp` to stamp — `ReplacementEffect` has none). New
  `ReplacementEffectBuilder` (mirrors `StaticAbilityBuilder`'s shape, minus `:SetLayer`/`:SetEffect`
  — there's no layer/effect pairing to validate here): `:SetAffected(table)` decodes straight to
  `EffectFilter` (not `AffectedFilter` — `EventFilter` has no `Script`-affected shape, so a
  function argument is a real error, not silently accepted); `:Register()` always builds the
  `Repeatable`/`WouldDie`/`Regenerate` shape and queues it. `run_card_script`'s drain step gained a
  third block, `pending_replacement` into `state.replacement_effects`. Bound as
  `Game.CreateReplacementEffect` alongside the other two builders in `bind_game_globals`.
- `docs/lua_scripts.md`: new "Self-registering a repeatable regeneration shield" section, same
  style as the static-ability/activated-ability sections either side of it.

**CR requirements:** 701.19b (static-ability-sourced, repeatable shield creation - the new
primitive this slice closes); 701.19c re-verified at application time, not just creation time (see
finding above), for both the static and one-shot paths.

**TODOs marked, not silently narrowed:** `Game.CreateReplacementEffect`'s own doc comment names the
single-payload-shape scoping (Regenerate only) as deliberate, same convention every prior builder
slice used for its own first cut.

**Tests added** (`crates/engine/tests/phase5_static_regeneration.rs`, 3 new tests): a card's
`initial_effect` calling `Game.CreateReplacementEffect(c):SetAffected({self_only=true}):Register()`
produces exactly the expected `Repeatable`/`WouldDie`/`Regenerate` shield in `state.
replacement_effects` (proves the Lua wiring, mirroring `phase5_static_ability_self_registration.
rs`'s own "unit-level" pattern - a directly-constructed shield stands in for the ETB-registered one
in the two tests below, same convention that file's own SBA test uses for a `ContinuousEffect`); a
static shield survives being used and saves its target from two separate lethal hits in the same
turn (the CR 701.19b headline behavior, contrasted directly against `phase5_regenerate_ability.rs`'s
own one-shot tests); a target that gains `CANT_BE_REGENERATED` *after* the static shield already
exists still dies for real, proving the match-time gate is what's actually authoritative (there's
no creation-time moment in this path to have caught it instead). A 4th test,
`commands.rs::tests::retire_object_purges_only_the_retired_source`, extended (not new) to also
assert a `ReplacementEffect` sourced from the retired id is purged and one from a different id
survives — alongside the existing ability/continuous-effect assertions it already made.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none
new). `cargo test --workspace` — all test binaries green, 0 failures, 718 tests total, including the
3 new tests in `phase5_static_regeneration.rs` and the extended `retire_object` unit test. Two
pre-existing tests needed their own assertions updated (not reverted) per the second correctness
finding above; every other prior test unaffected.

**STRUCT DEVIATIONS:** none — `ReplacementEffect`/`ConsumptionPolicy`/`EventFilter`/
`ReplacementPayload` were all already exactly the shape this slice needed.

**INTERFACE DEVIATIONS:** none beyond the new builder/binding/queue themselves.

**SCOPE DEVIATIONS:** the `retire_object`/`purge_granted_abilities` fix and the match-time CANT_BE_
REGENERATED check were both found while implementing (see the two findings above), not in the
original one-line pick from the gaps list — both are necessary for CR 701.19b to actually be
correct, not independent cleanup, so folded into this slice rather than deferred.

**UNRESOLVED:** `EventFilter`'s two-shapes-only limit, and `Game.CreateReplacementEffect`'s
single-payload-shape scoping, remain the same standing gap named in `docs/implementation.md`'s own
gaps list — damage prevention/redirection and non-regenerate zone-destination substitution still
have no Lua construction path.

## Phase 5: phase5/static-ability-duration slice (CR 611.2a-style, `StaticAbilityBuilder::SetDuration`)

Branch `phase5/static-ability-duration`, off master. Picked from the "Known unresolved gaps" list's
Static abilities bullet — `StaticAbilityBuilder::Register()` always hardcoded `duration: EffectDuration
::Permanent`, no Lua path to a temporary static-shaped `ContinuousEffect` (e.g. "artifacts you control
gain flying until end of turn").

**Scoping finding, before writing code:** the drain mechanism for every `EffectDuration` variant
already existed and was already fully proven by `phase5_effect_duration_expiration.rs`'s own tests,
generic over any `ContinuousEffect.duration` regardless of construction path. `Game.Durations.*`
(the 4-tag `DurationTag` table) was already bound and already consumed by `Game.Ops.Pump`'s own
duration argument. The only real gap was `StaticAbilityBuilder` itself never reading a duration from
Lua at all — a pure wiring gap, not a new mechanism.

**What got implemented:**
- `script.rs`: `StaticAbilityBuilder` gained a `duration: EffectDuration` field (defaults to
  `Permanent` at the one real construction site, same value this builder always produced before).
  New `:SetDuration(Game.Durations.*)` method — identical `AnyUserData`/`DurationTag` decode
  `Game.Ops.Pump`'s duration argument already uses. `Register()` now reads `this.duration.clone()`
  instead of the hardcoded `EffectDuration::Permanent`.
- `docs/lua_scripts.md`: static-ability section gained a `:SetDuration` paragraph + example, same
  style as the existing `:SetCondition` callout, cross-referencing `Game.Ops.Pump`'s own duration
  argument and `phase5/effect-duration-expiration`'s drain mechanism.
- `docs/implementation.md`: "Known unresolved gaps" list's Static abilities bullet updated to
  reflect closure.

**CR requirements:** CR 611.2a-style limited-duration static effects — no single CR citation covers
"a static ability's own continuous effect can be temporary" as its own numbered rule (CR 611.2 is
about spell/ability resolution effects generally); this closes the builder-side gap so a card script
can express that shape at all, reusing rules already covered elsewhere (CR 514.2 cleanup, CR
511.2/500.5a end of combat, CR 723.1-style until-your-next-turn) for the actual expiration.

**TODOs marked, not silently narrowed:** `WhileConditionHolds`/`Custom` durations stay unexposed to
`Game.Durations` (same "only 4 with a real drain path" stance the table's own doc comment already
states) — untouched by this slice, named rather than silently expanded. A static ability's duration
being itself conditional/script-decided (rather than a fixed tag) is out of scope, same fixed-tag-only
stance `Game.Ops.Pump` already took.

**Tests added** (`crates/engine/tests/phase5_static_ability_duration.rs`, 6 new tests): five
unit-level tests calling `run_card_script` directly, one per `Game.Durations.*` tag
(`UntilEndOfTurn`/`UntilYourNextTurn`/`UntilEndOfCombat`/explicit `Permanent`) confirming each
decodes into the matching `ContinuousEffect.duration`, plus one confirming omitting `:SetDuration()`
still defaults to `Permanent` (regression guard against the default flip breaking every existing
static-ability test). One end-to-end test: a Lua-registered `UntilEndOfTurn` static ability (via
`run_card_script`, same mechanism `PlayLand`/`CastSpell` resolution already drives — that path
itself proven separately by `phase5_static_ability_self_registration.rs`) seeded onto a real `Engine`
sitting at the Ending/End step, driven through both players passing into cleanup — the effect is
gone by the time the engine pauses again in the next turn's Upkeep (CR 502.4: no priority during
Untap, so the engine runs straight through it), proving the Lua wiring and the pre-existing drain
mechanism actually connect end to end.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none
new). `cargo test --workspace` — all test binaries green, 0 failures, including the 6 new tests; every
prior test unaffected (no existing `StaticAbilityBuilder` construction site needed updating beyond
the one real one in `script.rs` itself).

**STRUCT DEVIATIONS:** none.

**INTERFACE DEVIATIONS:** none beyond the new `:SetDuration` method itself — additive, no existing
method signature changed.

**SCOPE DEVIATIONS:** none.

**UNRESOLVED:** none new — `WhileConditionHolds`/`Custom` durations remain the same standing
`Game.Durations` scoping gap named elsewhere in this doc.

## Fix: layer 5 read `color_indicator` alone instead of `CardDefinition::colors()` (CR 105.2a/202.2)

Found while answering "how far from a real card database" — not picked from the gaps list, a stale
TODO caught by inspection. `layers.rs`'s `effective_characteristics` seeded its layer-5 base value
as `def.color_indicator.unwrap_or(ColorSet::empty())`, with a comment saying this was a stopgap
until `CardDefinition::colors()`/`ManaCost::colors()` got filled in. Both had already been filled in
for real (`types/card.rs::CardDefinition::colors()`, `types/mana.rs::ManaCost::colors()`) by some
earlier slice's mana-cost work, but nobody circled back to update this one caller — so every real
colored permanent (almost none of which set `color_indicator`, since that field only exists for the
rare card that needs an explicit override) was silently resolving as colorless in the layer engine.
A live, wrong answer today, not a missing feature — worth fixing directly rather than waiting for a
slice to trip over it.

**What got implemented:** `layers.rs`'s layer-5 seed changed from the raw `color_indicator` read to
`def.colors()` — CR 105.2a's own "indicator overrides, otherwise derive from the mana cost" split,
already correctly implemented there. Stale comment replaced with one explaining what changed and
why. No other call site needed touching — `def.colors()` was already fully correct, just uncalled
from here.

**Tests added** (`crates/engine/tests/phase5_layer_engine_types_colors.rs`, 2 new tests): a creature
with a `{W}{U}{1}` mana cost and no `color_indicator` now resolves to White/Blue in
`effective_characteristics` (the regression this fix closes); a creature with both a mana cost *and*
an explicit `color_indicator` still resolves to just the indicator's color, not a union of the two
(CR 105.2a's override, not additive, confirming the fix didn't change that half of the split).

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none
new). `cargo test --workspace` — all test binaries green, 0 failures, including the 2 new tests;
every prior test unaffected (no existing fixture relied on the old colorless-by-default behavior —
every test that cared about color already set `color_indicator` directly).

**UNRESOLVED:** none new. `SetCardTypes`/`SetColors` (the full "loses all other colors" overwrite
shape) and supertype/subtype gains remain the same standing gaps named in `types/effect.rs`'s own
TODOs, untouched by this fix.

## Phase 5: phase5/replacement-effect-shapes slice (CR 615/614.13, `Game.CreateReplacementEffect` generalized)

Branch `phase5/replacement-effect-shapes`, off master. Picked from the "Known unresolved gaps"
list's Replacement-effects bullet — `Game.CreateReplacementEffect` always hardcoded
`EventFilter::WouldDie`/`ReplacementPayload::Regenerate`/`ConsumptionPolicy::Repeatable`
(`phase5/static-regeneration`'s own deliberate CR 701.19b-only scoping), even though
`ReplacementEffect::matches`/`apply` already handled `DamageDealt`/`PreventDamage` (CR 615) and
`WouldDie`/`ReplaceZoneDestination` (CR 614.13-style "instead" substitution) correctly in Rust —
proven by `engine.rs`'s own `prevent_all_damage` unit test and
`phase5_combat_damage_replacement.rs`. The "no Lua construction path" note had been carried
forward, unfixed, across five prior slices.

**Scoping finding, before writing code:** since the engine mechanics for the other pairings were
already proven, this was a pure Lua-wiring gap, not a new mechanism — generalize
`ReplacementEffectBuilder` the same way `StaticAbilityBuilder::SetDuration` just generalized its
own builder: new settable fields, each defaulting to the builder's exact original behavior when
never called, so `phase5_static_regeneration.rs`'s existing tests (which only ever call
`:SetAffected` before `:Register()`) keep passing unmodified. `ReplacementPayload::
RedirectDamage(Target)` was scoped out up front — a redirect target isn't a static Lua literal the
way a zone name or a use-count is, and there was no clean answer for "which object/player" without
either a narrowing (redirect-to-source) or live target selection that doesn't obviously compose
with this queued-and-drained-later builder shape.

**Bug found and fixed while writing this slice's own end-to-end exile test, not by inspection:**
`sba::commit_death`'s zone-move always looked up the destination zone via `state.zone_mut(to,
Some(owner))` — correct for Graveyard (per-player) but wrong for Exile, which CR 400.10 makes a
*shared* zone (`owner: None`), same distinction `engine.rs`'s own stack-resolution zone move
already made correctly. `to` had only ever been `Graveyard` before this slice gave
`ReplaceZoneDestination` a real Lua path, so the wrong-owner lookup was unreachable code, not a
live bug, until now: with `to: Exile`, `zone_mut` found no matching zone at all, and the object was
silently never re-inserted anywhere. Fixed by giving `commit_death` the same
`Battlefield`/`Exile => None, _ => Some(owner)` branch `engine.rs`'s own zone-move code already
uses. This exposed a second, wider problem: six existing test fixtures
(`phase5_destroy_ordering_choice.rs`, `phase5_dies_triggers.rs`, `phase5_legend_world_rule.rs`,
`phase5_regeneration.rs`, `phase5_sba_death_replacement.rs`, `phase5_script_ops_destroy.rs`) had
each built their own *per-player* Exile zone (`owner: Some(p0)`, some with a second `Some(p1)`
entry alongside it) specifically to match `commit_death`'s old bug — one of them even had a comment
naming this as a known, deliberately-unreconciled inconsistency with the real shared-zone
convention. All six updated to the correct single shared `owner: None` Exile zone and the matching
`zone(ZoneKind::Exile, None)` query call sites, rather than left passing against the wrong model.

**What got implemented:**
- `script.rs`: `ReplacementEffectBuilder` gained `event_kind: EventFilterKind` (new, Lua-side-only
  enum — `EventFilter` itself has no shape until `affected` is filled in), `payload:
  ReplacementPayload`, and `consumption: ConsumptionPolicy` fields (defaults `WouldDie`/
  `Regenerate`/`Repeatable`, this builder's original values). New `:SetFilter`/`:SetPayload`/
  `:SetConsumption` methods. New `validate_replacement_filter_payload_pairing` (mirrors
  `validate_layer_effect_pairing`) — `DamageDealt` only pairs with `PreventDamage`; `WouldDie`
  pairs with `ReplaceZoneDestination`/`Regenerate`. `Game.EventFilters.DamageDealt`/`.WouldDie`
  (bare tags), `Game.ReplacementPayloads.PreventDamage`/`.Regenerate` (bare tags) and
  `.ReplaceZoneDestination(zoneName)` (new `parse_zone_kind_name` helper, same match-on-string
  pattern every other `parse_*_name` helper here uses — no zone-name parser existed in `script.rs`
  before this, only the separate `scenario` crate had one for TOML), `Game.Consumption.OneShot`/
  `.Repeatable` (bare tags) and `.LimitedUses(n)` (function).
- `sba.rs`: `commit_death`'s destination-zone-owner bug fixed (see above).
- `docs/lua_scripts.md`: "Self-registering a repeatable regeneration shield" retitled "Self-
  registering a replacement effect", extended with `:SetFilter`/`:SetPayload`/`:SetConsumption`
  and a Fog-effect worked example; explicit note on why `RedirectDamage` stays unbound.
- `docs/implementation.md`: Replacement-effects gaps-list bullet updated to reflect the builder's
  generalization (the remaining gap is `EventFilter`'s two-*shapes*-only limit, not payload/
  consumption coverage within those two shapes).

**CR requirements:** CR 615 (prevention effects, `PreventDamage`), CR 614.13-style "instead"
zone-destination substitution (`ReplaceZoneDestination`, non-regenerate case), CR 400.10 (Exile is
a shared zone — the bug fix above), CR 616 (the `OneShot`/`Repeatable`/`LimitedUses` consumption
shapes, whose ordering-choice pipeline was already proven by `phase5/destroy-ordering-choice`).

**TODOs marked, not silently narrowed:** `ReplacementPayload::RedirectDamage(Target)` stays
Lua-inaccessible, named in both `ReplacementEffectBuilder`'s own doc comment and the gaps-list
bullet. `EventFilter` itself still only covers two event *kinds* — protection (CR 702.16),
ETB-modifier replacements, and "would draw a card" substitution all remain unmodeled; this slice
only generalized construction within the two shapes that already existed.

**Tests added** (`crates/engine/tests/phase5_replacement_effect_shapes.rs`, 7 new tests): four
unit-level tests via `run_card_script` — `DamageDealt`/`PreventDamage`/`OneShot` and
`WouldDie`/`ReplaceZoneDestination("Exile")` each register the expected struct, a mismatched
`DamageDealt`/`ReplaceZoneDestination` pairing is a real `Register()`-time error, and omitting
every new setter still defaults to `WouldDie`/`Regenerate`/`Repeatable` (regression guard). Three
end-to-end tests through a real `Engine`: a Lua-registered Fog effect (`DamageDealt`/
`PreventDamage`/`OneShot`) actually zeroes real combat damage and is consumed after use; a
Lua-registered `ReplaceZoneDestination("Exile")` effect actually exiles a dying creature (the test
that caught the `commit_death` bug above — first asserting the event, then the *new* object's
actual zone, not the old retired id); a `LimitedUses(1)` shield stops protecting after its one use.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none
new). `cargo test --workspace --no-fail-fast` — every test binary green, 0 failures, including the
7 new tests and the 6 existing test files whose Exile-zone fixtures were corrected to match the
real shared-zone convention (see the bug-fix finding above) rather than reverted.

**STRUCT DEVIATIONS:** none.

**INTERFACE DEVIATIONS:** none beyond the new `:SetFilter`/`:SetPayload`/`:SetConsumption` methods
themselves — additive, no existing method signature changed.

**SCOPE DEVIATIONS:** the `sba::commit_death` Exile-zone-owner fix and its six-file test-fixture
ripple were both found while implementing (see the bug-fix finding above), not anticipated in the
original one-line pick from the gaps list — necessary for `ReplaceZoneDestination` to actually be
correct for any zone other than Graveyard, not independent cleanup, so folded into this slice
rather than deferred.

**UNRESOLVED:** `ReplacementPayload::RedirectDamage(Target)` and `EventFilter`'s two-shapes-only
limit remain the same standing gaps named in `docs/implementation.md`'s own gaps list.

## Phase 5: phase5/gain-control-effect slice (CR 108.4/613.1, `AbilityEffect::GainControl`)

Branch `phase5/gain-control-effect`, off master. Picked from `phase5/layer-engine-control-
consumers`'s own UNRESOLVED note — every real consumer of `layers::effective_controller` (combat,
targeting, cost filters, `has_summoning_sickness`, `FilterSpec::matches`) already reads the *live*
controller correctly, but nothing could create a `GainControl` effect at all; every existing test
built one via a bare `ContinuousEffect` struct literal.

**Scoping finding, before writing code:** two possible creation paths exist for a real "gain
control" card - a resolving spell/ability (Threaten-style, "gain control of target creature until
end of turn") or a static ability on an attached permanent (Control Magic-style aura, "you control
enchanted creature"). The aura path needs real attach/enchant machinery (`GameObject.attached_to`/
`.attachments` are inert placeholder fields — confirmed by grep, nothing ever writes to them) — a
separate, larger gap. Scoped to the resolving-spell path only, mirroring `AbilityEffect::Pump`'s
exact shape (same per-target `ContinuousEffect` construction, `self_only: true` pinned to the
target, no script call needed) since the only real difference is which layer/primitive pair gets
built. The new controller is always the resolving effect's own controller (`StackObject::
controller`) — no player argument needed from Lua, since every real "gain control" card in Magic
targets a creature and gives control to its own caster.

**What got implemented:**
- `types/ability.rs`: `AbilityEffect::GainControl(EffectDuration)` — new variant, same shape as
  `Pump` minus the power/toughness fields.
- `script.rs`: `Game.Ops.GainControl(duration)` binding, same `AnyUserData`/`DurationTag` decode
  `Game.Ops.Pump`'s duration argument already uses.
- `engine.rs`: new `ResolveStackTop` match arm, textually adjacent to `Pump`'s own — same per-target
  loop and `ContinuousEffect` construction, `layer: ControlChangingEffects`/`effect: GainControl
  (top.controller)` instead of `PowerToughnessModifying`/`ModifyPT`.
- `commands.rs`: `ActivateAbility`'s non-mana-effect catch-all pattern gained `Some(AbilityEffect::
  GainControl(_))` — a mechanical, compiler-caught addition (this primitive uses the stack like
  every other non-mana ability effect).
- `docs/lua_scripts.md`: `Game.Ops.GainControl` documented alongside the `:SetDuration` /
  `Game.Ops.Pump` cross-reference (this codebase has no dedicated section for `AbilityEffect`
  resolution-effect primitives as a group — `Damage`/`Destroy`/`Counters`/`Pump`/`Regenerate` aren't
  documented as a table either, a pre-existing gap wider than this slice, not attempted here beyond
  giving the new primitive itself a real mention).
- `docs/implementation.md`: `phase5/layer-engine-control-consumers`'s UNRESOLVED note updated to
  reflect the resolving-spell path's closure.

**CR requirements:** CR 108.4/613.1 layer 2 (control-changing effects), CR 611.2a's "duration
stated by the spell or ability creating it" (same convention `Pump` already established).

**TODOs marked, not silently narrowed:**
- Aura/static-ability-sourced control change (Control Magic-style) stays unbuilt — needs real
  attach/enchant machinery first, named in `AbilityEffect::GainControl`'s own doc comment and this
  write-up.
- `GameObject.controlled_since_turn` still isn't bumped by a live control-change event — left alone
  deliberately, not half-fixed: naively stamping it at gain-control time would introduce a *new*
  bug (the stale bumped value would persist on the object after the effect's duration expires and
  control reverts via the layer engine, making the original controller incorrectly read as having
  only just reacquired it). Correctly modeling "since when has each controller controlled this"
  needs more than one scalar field on `GameObject` — named, not guessed at.

**Tests added** (`crates/engine/tests/phase5_gain_control_effect.rs`, 4 new tests): a Threaten-style
sorcery resolving against an opponent's creature makes the caster the real `effective_controller`,
proven both by the query directly and by a concrete combat-legality check (the new controller can
declare the creature as an attacker — same shape `phase5_control_change_consumers.rs`'s own
end-to-end test uses, but through the real Lua/`AbilityEffect` creation path this time instead of a
hand-built `ContinuousEffect`); the effect is pinned to its target, not the spell's own source
(mirrors `phase5_pump_effect.rs`'s own pinning test); an `UntilEndOfTurn` gain-control effect drains
at cleanup and control reverts to the original owner; a `Permanent`-duration gain-control effect
survives into the next turn.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none
new). `cargo test --workspace --no-fail-fast` — every test binary green, 0 failures, including the
4 new tests; every prior test unaffected.

**STRUCT DEVIATIONS:** none.

**INTERFACE DEVIATIONS:** none beyond the new `AbilityEffect` variant itself — additive, no
existing variant or function signature changed.

**SCOPE DEVIATIONS:** none.

**UNRESOLVED:** the aura/static-ability control-change path and `controlled_since_turn` (see the
two TODOs above) — both named, not silently dropped.

## Phase 5: phase5/protection-from-color slice (CR 702.16, protection from color)

Branch `phase5/protection-from-color`, off master. Picked from the "Known unresolved gaps" list -
protection had no representation anywhere in this engine, not even a `KeywordSet` bit. Scoped to
CR 702.16e's "protection from color" shape (by far the most common real form) and three of the four
"DEBT" restrictions - damage (702.16a), targeting (702.16c), blocking (702.16d). Enchant/Equip
(702.16b) stays unmodeled, named rather than attempted - no attach/enchant machinery exists in this
engine yet (confirmed absent, `GameObject.attached_to`/`.attachments` are never written to
anywhere), same gap `phase5/gain-control-effect` already named for its own aura-sourced follow-up.

**Scoping finding, before writing code:** a shortcut was considered and rejected - treating
protection's damage half like Lifelink/Deathtouch/Wither/Infect, a post-hoc special case in
`commands::apply_damage_event`. That doesn't fit: those keywords change what already-committed
damage *does*; protection has to stop the damage from happening at all, which only the replacement-
check pipeline (pre-commit) can do. The real design finding: protection isn't a `state.
replacement_effects` entry a card registers - it's a static, always-on characteristic of the object
itself (like a keyword), so enforcement is three hardcoded checks at the sites that already gate
the equivalent real-gameplay action, the same "checked up front, not a registered replacement"
precedent CR 702.12b indestructible already set ahead of `Destroy` - not three new generic
mechanisms.

**Why a separate field, not a `KeywordSet` bit:** protection has to carry *which* color(s), which a
flat bitflag can't express. `EffectiveCharacteristics` gained `protection_colors: ColorSet`, folded
in in the same layer-6 pass as `keywords` from a sibling `EffectPrimitive::AddProtectionColor`
(union, same shape `AddKeyword` already uses) - reuses `StaticAbilityBuilder`/`Game.
CreateStaticAbility` wholesale, no new builder needed, and `:SetDuration` (already generic) covers
"gains protection until end of turn" for free.

**What got implemented:**
- `types/effect.rs`: `EffectPrimitive::AddProtectionColor(ColorSet)`. `EffectiveCharacteristics.
  protection_colors: ColorSet`.
- `layers.rs`: layer-6 fold in `effective_characteristics` gained a third match arm alongside
  `AddKeyword`.
- `script.rs`: `Game.Effects.AddProtectionColor(letter)`, pairing-validation entry for
  `(AbilityAdding, AddProtectionColor)`.
- `replacement.rs`: new `apply_protection_from_color` - checked fresh at the top of every
  `apply_replacements_from` loop iteration (idempotent, so safe to call unconditionally every pass),
  ahead of the registered-`ReplacementEffect` match. Re-checking every iteration rather than once
  up front matters for real correctness: it means a mid-resolution `RedirectDamage` retarget is
  caught against the *new* target, not just the original proposal.
- `commands.rs`: new `target_has_protection_from`, called from both `TargetFilter::Object` and
  `TargetFilter::AnyTarget` arms of `target_is_legal`. `target_is_legal`/`validate_targets` both
  gained a `source: ObjectId` parameter (the spell/ability's own id, not the caster - protection
  cares about the source's color, same distinction `FilterSpec::matches`'s own `object_controller`
  parameter already established for a different check) - already in scope at both real call sites
  (`CastSpell`'s `card`, `ActivateAbility`'s `source`), just not threaded through before. `Declare
  Blockers`'s existing Flying/Reach/Menace check block gained a sibling protection-from-color check,
  same `effective_characteristics` read.
- `engine.rs`: both other `target_is_legal` call sites (`submit_script_choice`'s `Game.
  SelectTargets` validation, and the CR 608.2b resolution-time recheck) updated for the new
  parameter - `stack_id`/`top.stack_id` respectively.
- `docs/lua_scripts.md`: `AddProtectionColor` documented in the static-ability section, with a
  worked example and a summary of all three enforcement sites.
- `docs/implementation.md`: new gaps-list bullet.

**CR requirements:** CR 702.16a (damage), 702.16c (targeting), 702.16d (blocking), narrowed to
702.16e's color-only quality grammar. 702.16b (enchant/equip) and the type/subtype/player/name/
"everything" quality shapes are named follow-ups, not attempted.

**TODOs marked, not silently narrowed:**
- Protection from card type/subtype/a specific player/name/"everything" - only color this slice.
- Enchant/Equip protection (702.16b) - needs attach mechanics, doesn't exist (see scoping finding
  above).

**Tests added** (`crates/engine/tests/phase5_protection_from_color.rs`, 8 new tests): Lua
self-registration (`Game.Effects.AddProtectionColor("R")` produces the right primitive and shows up
in `effective_characteristics.protection_colors`); combat damage from a matching color is prevented,
from a different color still lands; casting a matching-color spell at a protected target is
rejected, a colorless spell still works; a spell fizzles (CR 608.2b, the pre-existing generic
mechanism - not a new one) when its target gains protection after being targeted but before
resolution; a matching-color creature can't block a protected attacker, a different-color creature
still can.

**VERIFICATION RESULT:** Pass. `cargo build --workspace` clean (only pre-existing warnings, none
new). `cargo test --workspace --no-fail-fast` - every test binary green, 0 failures, including the
8 new tests; every prior test unaffected (the `target_is_legal`/`validate_targets` signature change
was mechanical - every existing call site already had the needed `ObjectId` in scope).

**STRUCT DEVIATIONS:** none.

**INTERFACE DEVIATIONS:** `target_is_legal`/`validate_targets` both gained a `source: ObjectId`
parameter - anticipated in the pre-slice report ("target_is_legal... gain the casting spell/
activated ability's own source id... same signature change FilterSpec::matches took"), not a
deviation.

**SCOPE DEVIATIONS:** none.

**UNRESOLVED:** protection from card type/subtype/player/name/everything, and Enchant/Equip - both
named above, not silently dropped. `phase5/lose-all-abilities` (a separate, still-unmerged branch)
introduces `EffectPrimitive::RemoveAllAbilities`, which resets `keywords` in the same layer-6 fold
this slice touched - when that branch merges, its reset will need extending to also clear
`protection_colors`, or "loses all abilities" would leave protection behind. Not fixed here since
the primitive doesn't exist on this branch yet; flagged so it isn't missed at merge time.
