# Stackfish — MTG Simulator Engine — Design Doc

> Living doc. Update as decisions are made/changed through phases 1-7 of `docs/workflow.md`.
> Status: Phase 0 (in progress).

## Purpose

A deterministic Magic: The Gathering rules engine, written in Rust, architecturally based on
`edo9300/ygopro-core` (see `docs/ygo_pro_reference.md`) but redesigned around MTG's rules model
(see `docs/mtg_adaptation_reference.md` and `docs/comp_rules.md`). Headless core; UI/network is a
separate host application.

Supports normal games of Magic, including Commander, for N players (not hardcoded to 2 or 4).

## Non-goals (v1)

- No silver-border / casual-format mechanics: Vanguard, Planes/Phenomena, Conspiracies, Ante,
  Stickers, Attractions, Dungeons.
- No planeswalkers, Sagas, Battles, double-faced cards, split/flip/adventure cards, Class cards
  in the initial card-type implementation — but the data model and layer/effect systems must
  leave room for them (stubbed type slots, not a wall).
- No netcode/matchmaking — that's host-application concern.
- No card database tooling — card data (oracle text, costs, types, P/T, and a reference to or
  literal of the Lua script implementing the card) comes from an external, separately-maintained
  database project. This engine consumes that data; it doesn't author or update it.

## Architecture

Reuse engine *style* from ygopro-core: single authoritative core, persistent runtime object
instances, embedded scripting for card-specific behavior, core enforces legality (scripts only
request). Replace the YGO rules kernel entirely with an MTG-native one per the adaptation doc:

- Stack + priority (not YGO chain links)
- N-player turn/phase/step framework with generalized APNAP (active player + all others in
  turn order, not hardcoded to 2)
- Continuous-effect layer engine (CR 613)
- Replacement/prevention pre-commit rewrite pipeline
- State-based actions (SBA) fixed-point loop
- Trigger queue with APNAP stack-insertion ordering
- Structured cost/payment system

### Step-able core (replay requirement)

The rules processor is implemented as a pull-based state machine: `engine.step()` advances by
one ledger entry and returns. Live play drives `step()` in a tight loop between player decision
points (no pausing needed). A separate replay viewer re-feeds the same seed + command list into
a fresh engine instance and calls `step()` one at a time, so it can pause/resume/auto-play through
an exact reconstruction. One engine, two callers — no separate "replay mode" code path.

Implication: every state mutation must happen as the result of applying a logged `GameEvent` —
nothing mutates state out-of-band. This is also what gives deterministic replay correctness (same
seed + command stream -> same event stream) for free, beyond just supporting scrubbing.

### Event log granularity

Resolved in Phase 1: the ledger logs every micro-step (each priority pass, each SBA application,
each trigger queued, each replacement applied), not just coarse card-level events. It doubles as
the live in-game action log and the replay viewer's step unit — those two consumers get different
default filtering/grouping over the same underlying stream, not two separate logging strategies.
First-pass event taxonomy lives in `crates/engine/src/event.rs`; expect additions as later phases
surface gaps.

## Core modules

(From the adaptation doc, adjusted for N-player + step-able core.)

1. **Game Instance** — owns full mutable state for one game (N players, not fixed count).
2. **Object Store** — runtime `GameObject` instances (cards/tokens/spells/abilities/emblems).
3. **Zone Containers** — Library, Hand, Battlefield, Graveyard, Exile, Stack, Command (includes
   Commander zone for commander games).
4. **Rules Processor** — step-able state machine for phases/steps/priority/turn order.
5. **Command Executor** — validates and executes `EngineCommand`s.
6. **Event Ledger** — immutable, ordered, append-only `GameEvent` log. Single source of truth for
   state reconstruction, live action log, and replay.
7. **Replacement/Prevention Engine** — rewrites proposed events before commit.
8. **Continuous Layer Engine** — computes derived characteristics (CR 613 layers).
9. **Trigger Engine** — captures and enqueues triggered abilities, APNAP-ordered onto the stack.
10. **Script Runtime** — embedded Lua VM (`mlua`) exposing a card-author-friendly bound API.
11. **Protocol Emitter** — sends prompts/state/log entries to host application.

## Data model

Implemented in `crates/engine/src/` (Phase 1 complete — see `docs/implementation.md` for the
file-by-file rationale and decisions made while building it). High-level shape, per the
adaptation doc, unchanged from the original plan:

- Immutable shared card data (`CardDefinition`) is separate from mutable runtime instances
  (`GameObject`), which are separate again from the live, layer-computed view of an object's
  current characteristics — three distinct things, not one struct with optional overrides.
- Object identity follows MTG's object-identity rule: a zone change that creates a "new object"
  mints a new id rather than reusing the old one, with a lineage link back for cases that need to
  trace through it (e.g. last-known-information).
- Zone *order* lives in exactly one place (the zone container), never duplicated onto the object
  itself.
- `PlayerAction -> EngineCommand -> GameEvent` pipeline, per the adaptation doc, unchanged — see
  `docs/implementation.md` for why `EngineCommand` doesn't decompose into a lower command tier.

## Scripting model

- Cards implemented in Lua, embedded via `mlua`, interpreted (not transpiled to native Rust).
- Reload requires an engine restart — no hot-reload requirement, so no need for a Lua->Rust
  compilation pipeline.
- Script API is the friendly surface: `Game.RegisterTriggeredAbility`, `Game.AddContinuousEffect`,
  `Game.ReplaceEvent`, `Game.CastSpell`, `Game.PayCost`, `Game.SelectTargets`, etc. Script authors
  should not need Rust familiarity — only the Lua API surface.
- Core responsibilities stay in Rust: legality, timing, hidden information, stack/priority/SBA/
  replacement semantics. Lua only expresses card-specific intent and is asked, never trusted.

## Multiplayer / Commander

- Player count is a runtime parameter, not a compile-time constant. Turn order is a list of N
  players; APNAP = active player first, then remaining players in turn order.
- Commander-specific: Command zone holds commander(s), commander tax (generic cost increase per
  prior cast from command zone), commander damage (21 cumulative from a single commander source ->
  that player loses), color identity (affects deck legality, not in scope for the engine itself
  unless deck validation is wanted later), player elimination without ending the whole game.
- Multiplayer affects: attack target selection (attacking player chooses among multiple
  opponents), trigger/SBA evaluation across N players, what happens to zones the affected
  state when a player is eliminated (their cards leave the game — exile, generally).

## Determinism & replay

- All randomness (shuffles, coin flips, die rolls) goes through a seeded PRNG; every PRNG draw is
  itself a logged event, not an ambient side effect.
- No wall-clock dependence in any rules decision.
- Hidden-information choices are explicit `PlayerAction`s, never implicit defaults.
- Replay = same seed + same `PlayerAction`/`EngineCommand` stream -> byte-identical `GameEvent`
  stream. Replay viewer is a separate consumer of the step-able core (see above), not part of the
  live game loop.

## Open questions

- [ ] Exact mlua <-> Rust binding boundary for hidden information (avoiding leaking, e.g., an
      opponent's hand contents into a Lua callback that shouldn't see it).
- [x] Player-input-mid-script: resolved for target selection (`Game.SelectTargets`) via real Lua
      coroutines (`Lua::create_thread`/`Thread::resume`, yield/resume spanning `Engine::step()`
      calls) — see `docs/implementation.md`'s script-coroutines slice and `docs/lua_scripts.md`'s
      "Asking the player for a target mid-resolution". Still open: modal spells, "may" abilities,
      and selecting a *player* as the target (backlog items 9a/9b) — same mechanism, different
      request shapes, not built yet.
- [ ] Snapshot/checkpoint strategy for the event ledger — replaying from event 0 every time you
      want state at event N works for v1, may need periodic snapshots later for performance. Not
      a v1 concern.
- [ ] Deck/format legality validation (color identity for Commander, etc.) — explicitly out of
      scope unless revisited.

## Verification

Three workspace crates: `crates/scenario` (declarative TOML game-scenario loader/runner),
`crates/cli` (file-driven scenario runner — `cargo run -p stackfish-cli -- scenario.toml`, no live
REPL yet), and `crates/engine/tests/` (real `cargo test` integration tests over scenario files in
`tests/scenarios/`). Scenarios build `GameState` via direct struct literals rather than the
engine's own constructors, so setup/assertion-only scenarios already run for real regardless of
implementation progress; actions that hit unimplemented engine code report as such rather than
failing the run. See `docs/implementation.md`'s Phase 4 section for the full design.

## Status

Phases 0-4 complete (Design Doc, Structs, Interfaces, TODO annotation, Verification Setup).
`docs/implementation.md` has the detailed decision log for each. Next: Phase 5 (Dry Run).
