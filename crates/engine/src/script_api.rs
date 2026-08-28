//! This file originally sketched four Lua-callable functions for a card's
//! script to ask the engine to do something on its behalf (DESIGN.md: "Lua
//! asks, Rust decides"). None of the four still belong here:
//! - Ability/continuous-effect/replacement-effect registration and target
//!   selection (`Game.RegisterTriggeredAbility`/`Game.AddContinuousEffect`/
//!   `Game.ReplaceEvent`/`Game.SelectTargets`) — built for real via a
//!   different mechanism: `script::builders`' fluent `Game.Create*Ability`/
//!   `Game.CreateReplacementEffect` builders (`ScriptRuntime::run_card_script`
//!   drains their queued results into `GameState` with a real allocated
//!   id/timestamp) and `script::globals`' coroutine-based `Game.SelectTargets`
//!   (a literal `coroutine.yield` Lua closure — the synchronous `Vec<Target>`
//!   shape this file once sketched couldn't have worked at all, since mlua
//!   0.9 without the `async` feature can't suspend from inside a Rust-bound
//!   closure).
//! - `Game.CastSpell` — built for real as `Game.Ops.CastSpell` (`script::
//!   globals`), decoded into `OperationOutcome::PendingCast` and carried out
//!   by `engine::resolve::misc_effects::resolve_pending_cast`. Same "outcome
//!   describes, engine acts" split `Game.Ops.SacrificeChosen`/`DiscardChosen`
//!   already established — this file's own synchronous `&mut GameState`
//!   signature could never have worked either, for the same structural
//!   reason `Game.SelectTargets` couldn't: casting needs `commands::
//!   execute_command`'s full pipeline (payment pause, replacement checks,
//!   ...), which no function bound directly on `GameState` has access to.
//! - `Game.PayCost` — never had a real caller, and never got bound into the
//!   Lua `Game` table at all. Every real "ask a player to pay a cost mid-
//!   resolution, or something happens instead" case this engine has built
//!   (CR 118.12/118.12a's "unless paid" template — Mana Leak/Condescend/Ward
//!   via `UnlessPaidConsequence::Counter`, Echo via `SacrificeSelf`) got its
//!   own dedicated pause and `Engine::submit_unless_paid_payment`/
//!   `decline_unless_paid` pair instead, validating a `PaymentPlan` against
//!   a real `CostSpec` and committing via `commands::commit_payment`
//!   directly — the exact thing this file's own dead `pay_cost` stub wanted
//!   to generalize, confirmed twice now (Ward, then independently Echo) to
//!   need no generic Lua-facing primitive. Removed rather than implemented,
//!   same "confirmed zero real callers, deleted rather than guessed at"
//!   precedent `phase5/stack-owner-and-dead-stubs` already set for this
//!   file's Phase-3-era siblings.
//!
//! See those modules, not this file, for how a card actually registers an
//! ability, asks for a target, casts a spell, or pays a mid-resolution cost
//! today.
//!
//! TODO(keyword-name -> primitive/ability mapping layer, not built yet):
//! `types::card::KeywordSet` covers the CR 702 keywords that change how an
//! existing engine checkpoint behaves (Flying, Deathtouch, Lifelink, ...) —
//! but nothing here lets a card's script grant one by name. The ergonomic
//! shape discussed for this: something like `Game.GrantKeyword(card,
//! "Flying")`, where this file owns a name -> `KeywordSet` bit lookup table
//! (for the primitives `KeywordSet` models) or, for a keyword that's really
//! sugar for an ordinary ability (Cycling, Persist, ...), expands the name
//! into a `Game.CreateTriggeredAbility`/`Game.CreateActivatedAbility` call
//! instead (see `Game.RegisterKeyword` in `script::globals` for the one
//! keyword shape - a bare layer-6 `AddKeyword` static ability - already
//! wired this way). Without this, every card wanting a standard keyword has to be
//! authored against `KeywordSet` directly rather than just naming the
//! keyword — fine for the hand-built test fixtures this phase has used so
//! far, not fine once real card data flows through here. See
//! `KeywordSet`'s own doc comment for the primitive/sugar split this table
//! would need to encode.
