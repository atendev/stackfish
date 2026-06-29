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

## Next slice

## Mistakes caught (don't repeat)

- Claimed hybrid Phyrexian mana symbols (`{W/U/P}`) needed a 3-element container because they're
  "a true 3-alternative choice." Wrong framing — confused *payment options* with *symbol slots*.
  An atom can itself carry multiple payment options (`Phyrexian(Color)` = color or life), so a
  2-slot `Hybrid` of two atoms can yield more than 2 total ways to pay. Before declaring a data
  shape "can't represent X," check whether an existing variant already absorbs the complexity.
