//! `Game.CastSpell`/`Game.PayCost` — the two functions below are called
//! *from* a card's Lua setup or operation callback, asking the engine to do
//! something on the script's behalf; the engine still decides legality
//! (DESIGN.md: "Lua asks, Rust decides"). Neither is bound into the Lua
//! `Game` table yet — real, still-open gaps.
//!
//! This file originally sketched a third category too — ability/continuous-
//! effect/replacement-effect registration and target selection
//! (`Game.RegisterTriggeredAbility`/`Game.AddContinuousEffect`/
//! `Game.ReplaceEvent`/`Game.SelectTargets`) — but that's since been built
//! for real via a different mechanism: `script::builders`' fluent
//! `Game.Create*Ability`/`Game.CreateReplacementEffect` builders
//! (`ScriptRuntime::run_card_script` drains their queued results into
//! `GameState` with a real allocated id/timestamp) and `script::globals`'
//! coroutine-based `Game.SelectTargets` (a literal `coroutine.yield` Lua
//! closure — the synchronous `Vec<Target>` shape this file once sketched
//! couldn't have worked at all, since mlua 0.9 without the `async` feature
//! can't suspend from inside a Rust-bound closure). See those two modules,
//! not this file, for how a card actually registers an ability or asks for
//! a target today.
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

use crate::commands::CommandError;
use crate::types::cost::PaymentPlan;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId};

/// `Game.CastSpell` — a script requesting the engine cast a spell on a
/// player's behalf (e.g. "you may cast target spell card from exile
/// without paying its mana cost"), not the normal player-initiated path
/// (`PlayerActionKind::CastSpell`). Still goes through the same legality
/// checks; `free` skips the mana cost specifically — other additional
/// costs still apply unless the card's own effect says otherwise.
pub fn cast_spell(_state: &mut GameState, _card: ObjectId, _controller: PlayerId, _free: bool) -> Result<(), CommandError> {
    // TODO: build the equivalent of a CastSpell EngineCommand (modes/
    // targets/x_value need defaults or params - "cast target spell" effects
    // usually specify these themselves, so this signature is likely
    // incomplete) and run it through commands::execute_command, skipping
    // mana cost collection if free.
    todo!()
}

/// `Game.PayCost`.
pub fn pay_cost(_state: &mut GameState, _payer: PlayerId, _plan: PaymentPlan) -> Result<(), CommandError> {
    // TODO: validate plan against whatever CostSpec it's paying (not a
    // parameter here - needs one added, PaymentPlan::validate takes a
    // &CostSpec and this function doesn't have one to pass it), commit if
    // valid via the same path SubmitPayment uses in commands.rs.
    todo!()
}
