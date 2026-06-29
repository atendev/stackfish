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

## Mistakes caught (don't repeat)

- Claimed hybrid Phyrexian mana symbols (`{W/U/P}`) needed a 3-element container because they're
  "a true 3-alternative choice." Wrong framing — confused *payment options* with *symbol slots*.
  An atom can itself carry multiple payment options (`Phyrexian(Color)` = color or life), so a
  2-slot `Hybrid` of two atoms can yield more than 2 total ways to pay. Before declaring a data
  shape "can't represent X," check whether an existing variant already absorbs the complexity.
