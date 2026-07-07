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
   2. **B: layers 4/5** (type-changing, color-changing) — same `EffectPrimitive` shape as
      `AddKeyword`/`SetPT` (`SetType`/`AddType`, `SetColor`/`AddColor`), mechanical extension of
      the existing query.
   3. **C: layer 7a** (characteristic-defining P/T) — needs a real `&ScriptRuntime` threaded into
      `effective_characteristics` to invoke a card's CDA callback. Natural pairing point to also
      finally evaluate `AffectedFilter::Script`/`EffectPayload::Script` (both dead code today),
      since the script-runtime plumbing arrives in the same slice.
   4. **D: layer 7e** (power/toughness switching) — narrow, low effort once 7b/7c exist.
   5. Layers 1/2/3 (copy/control/text-changing) and the CR 613.6-8 dependency system — each needs
      its own real subsystem (copy-object semantics, control-transfer command, text-rewrite
      representation, dependency detection), not just a query extension. Stay last.
2. **Targeting system** — `CastSpell`/`StackObject` accept `targets` but nothing validates
   legality/count, rechecks at resolution, or fizzles. Blocks the majority of real spells.
3. **Static ability self-registration** (`Game.CreateStaticAbility`) — pairs with #1, needed for
   any printed static ability text to actually attach a `ContinuousEffect`.
4. **General activated abilities** — extend past the mana-tap-cost/single-tap-non-mana shape that
   just landed: sacrifice/discard/other costs, multi-ability-per-card stable indexing (currently
   positional by registration order, flagged fragile since the mana-ability slice), `Game.
   CreateActivatedAbility`.
5. **Replacement/prevention effects (CR 614-616)** — `replacement.rs` is still a no-op past the
   empty-registry fast path. Needed for "if this would die, exile it instead," damage prevention/
   redirection.
6. **Cost system completeness** — additional costs, alternative costs, `{X}` costs, hybrid/
   Phyrexian/snow mana payment, cost reduction/increase.
7. **Trigger system completeness** — once-per-turn/intervening-if (CR 603.2-603.3), last-known-
   info fallback for a trigger whose source already left the battlefield (CR 603.10, flagged since
   the script-bridge-trigger slice), non-ETB trigger conditions (leaves-battlefield, upkeep,
   damage-dealt, etc — only `Game.Conditions.Enters`/`Always` exist today).
8. **Script API expansion** — more `Game.Ops` primitives (DrawCards, Damage, Counters, Destroy,
   Exile, Sacrifice, CreateToken — only `GainLife` exists today), and a hidden-information-safe
   `ScriptContext` (currently leaks the full `&GameState`, a standing DESIGN.md open question).
9. **Player-input-mid-script (Lua coroutines)** — needed for "choose one," "may," and modal spells.
   `invoke_target`/`Game.SelectTargets` are stubbed synchronous on purpose; real fix needs
   `Lua::create_thread` yield/resume spanning `Engine::step()` calls, flagged unresolved since
   Phase 2 part 3.
10. **SBA completeness** — empty-library draw loss (704.5b), 10+ poison (704.5c), legend rule
    (704.5j), commander damage ≥ 21 wiring (`PlayerState::commander_damage_from` already exists,
    unused).
11. **Multiplayer/Commander generalization** — combat is 2-player only today (`CombatState`/
    `DeclareBlockers` assume one defending player); command-zone tax/damage/elimination-without-
    ending-the-game aren't wired despite being named in DESIGN.md's Commander section.
12. **First/double strike (CR 702.4/702.7)** and remaining combat-keyword edges.
13. **Mana system completeness** — lands/abilities that add more than one mana or a player-chosen
    color, beyond the flat `AbilityEffect::AddMana(Vec<ManaUnit>)` shape.
14. **Zone/library helpers** — `Zone::new`/`push_top`/`top` still `todo!()`; needed for shuffle/
    scry/surveil/search/tutor effects.
15. **CR 601.6 unwind** — a rejected `submit_payment` plan leaves the engine stuck in
    `AwaitingPayment` forever; nothing returns the card to hand or un-pushes the stack object.
16. **Planeswalkers/Battles/DFCs/split/Sagas** — explicit non-goals in DESIGN.md's v1 scope;
    lowest priority unless that scope changes.

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

## Mistakes caught (don't repeat)

- Claimed hybrid Phyrexian mana symbols (`{W/U/P}`) needed a 3-element container because they're
  "a true 3-alternative choice." Wrong framing — confused *payment options* with *symbol slots*.
  An atom can itself carry multiple payment options (`Phyrexian(Color)` = color or life), so a
  2-slot `Hybrid` of two atoms can yield more than 2 total ways to pay. Before declaring a data
  shape "can't represent X," check whether an existing variant already absorbs the complexity.
