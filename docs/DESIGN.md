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
- `Game.Ops.ForEachPlayer` (`phase5/mass-choice-effects`): APNAP applied to effect *resolution* too
  (CR 101.4's "each player ..." shape, not just trigger placement) - `triggers::apnap_player_order`
  is the one shared rotation both use
- `Game.Ops.Tap`/`Untap`/`TapSelf`/`UntapSelf`/`TapAll`/`UntapAll` (`phase5/tap-untap-effects`): status flips
  (CR 701.26) mutate `tapped` in place and push `Tapped`/`Untapped` only on a real flip, like the tap-cost,
  attack and untap-step producers; they are not replacement-checked yet
- `Game.Ops.Mill`/`LoseLife`/`SacrificeSelf` (`phase5/mill-lose-life-sacrifice-self`): `Mill` and `LoseLife` pick the
  controller when the ability declares no target spec and the first legal player target when it does (never a
  fallback when that target is gone). Mill moves each card as a replacement-checked Library to Graveyard
  `ZoneChange` (`ScriptedZoneMoveKind::Mill`, committed through `commit_zone_move`); `LoseLife` is the first
  resolution-time producer of `LifeChanged`; `SacrificeSelf` reuses the `Sacrifice` path with a CR 701.21a control
  check
- `Game.Ops.Fight`/`Bite` (`phase5/fight-bite`): pairwise damage between two creatures (CR 701.14). The pair comes
  from the target spec (two-word `Multiple`, one count-2 word for Fight, or the ability source plus one count-1
  target). Each creature is the source of its own `DamageDealt` event (not combat damage), so the existing
  `apply_damage_event` and replacement pipeline give deathtouch, lifelink, wither, infect, protection and
  prevention for free; both amounts are read before either event is pushed. `s.target_requirement()` may now
  return an array, which builds `TargetSpec::Multiple`
- `Game.Ops.ReturnToBattlefield`/`ReturnAllToBattlefield` (`phase5/reanimation`): graveyard or exile to
  battlefield through `ScriptedZoneMoveKind::Reanimate { controller, tapped }`. The entry is replacement-checked,
  then `commit_reanimate` gives the card the treatment a resolving permanent spell gets (new object, card script,
  `apply_enters_battlefield_modifiers`, Aura host). CR 110.2a picks the controller (`EntryControl::You` default).
  The graveyard object's `controller` is written temporarily so entry filters see the entering player and reset to
  the owner on every path that does not enter. Persist and Undying keep their narrower `ReturnSelfWithCounter` path
- Structured cost/payment system

### Step-able core (replay requirement)

The rules processor is implemented as a pull-based state machine: `engine.step()` advances by
one state transition and returns (zero or several events). Live play drives `step()` in a tight loop between player decision
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
11. **Protocol Emitter** — `crates/host`. A `Session` wraps one engine and talks only in bytes (JSON
    over direct calls): a `setup` request creates it, `advance` and `answer` requests drive it, and
    each response carries a batch of events plus the open Decision. A `CardSource` handed in at
    creation turns `oracle_id` keys into `CardDefinition`s. See `docs/implementation/18-host-session.md`.
    The crate also ships a 25-card fixture set (`proof_cards`) and a random `Bot` that plays whole games
    through the byte interface, which is how the protocol is tested end to end. See
    `docs/implementation/19-proof-game.md`.

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
- Script API is the friendly surface: `Game.CreateTriggeredAbility`, `Game.CreateStaticAbility`,
  `Game.CreateReplacementEffect`, `Game.Ops.CastSpell`, `Game.SelectTargets`, etc.
  (see `docs/lua/index.md` for the full, current reference). Script authors should not need Rust
  familiarity — only the Lua API surface.
- Core responsibilities stay in Rust: legality, timing, hidden information, stack/priority/SBA/
  replacement semantics. Lua only expresses card-specific intent and is asked, never trusted.
  Target legality is one such responsibility: protection, hexproof and shroud are enforced in
  `commands::target_is_legal`, so a script can grant or remove the keyword but never decides
  whether a target is legal.

## Multiplayer / Commander

- Player count is a runtime parameter, not a compile-time constant. Turn order is a list of N
  players; APNAP = active player first, then remaining players in turn order.
- Commander-specific: Command zone holds commander(s), commander tax (generic cost increase per
  prior cast from command zone), commander damage (21 cumulative from a single commander source ->
  that player loses), color identity (CR 903.4/903.5c/d/702.124c - `color_identity`/
  `commander_color_identity`/`illegal_cards_for_color_identity` in `crates/engine/src/
  color_identity.rs`; pure query functions, not enforced by `Engine::new_game` or any command - a
  deck-builder or test harness calls them itself, same "engine trusts the setup" stance CR 903.3a's
  commander eligibility already takes), player elimination without ending the whole game.
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
      calls) — see `docs/implementation/05-script-bridge.md`'s script-coroutines slice and
      `docs/lua/targeting-and-choices.md`'s "Asking the player for a target mid-resolution". CR
      700.2's modal-spell cluster (a-i) is closed — see `99-backlog.md`'s Targeting entry. Still
      open: a modal *triggered* ability's own mode declaration (CR 603.3c) has no Lua builder yet
      — see `docs/lua/not-available-yet.md`.
- [ ] Snapshot/checkpoint strategy for the event ledger — replaying from event 0 every time you
      want state at event N works for v1, may need periodic snapshots later for performance. Not
      a v1 concern.
- [x] Deck/format legality validation (color identity for Commander) — resolved as pure query
      functions (`crates/engine/src/color_identity.rs`, CR 903.4/903.5c/d), not automatically
      enforced anywhere. Card-count/unique-name deck-list rules (CR 903.5a/b) stay out of scope —
      `PlayerConfig` has no deck-list concept to check.

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
