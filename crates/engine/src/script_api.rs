//! Functions in this file are bound into the Lua global `Game` table — e.g.
//! `register_triggered_ability` below becomes `Game.RegisterTriggeredAbility`
//! — the script-author-facing surface named in DESIGN.md. Each is called
//! *from* a card's Lua setup or operation callback, asking the engine to do
//! something; the engine still decides legality (DESIGN.md: "Lua asks, Rust
//! decides"). The actual `mlua` registration (binding these into the Lua
//! table) isn't written yet — these are the Rust-side signatures that
//! registration will wrap.
//!
//! TODO(keyword-name -> primitive/ability mapping layer, not built yet):
//! `types::card::KeywordSet` covers the CR 702 keywords that change how an
//! existing engine checkpoint behaves (Flying, Deathtouch, Lifelink, ...) —
//! but nothing here lets a card's script grant one by name. The ergonomic
//! shape discussed for this: something like `Game.GrantKeyword(card,
//! "Flying")`, where this file owns a name -> `KeywordSet` bit lookup table
//! (for the primitives `KeywordSet` models) or, for a keyword that's really
//! sugar for an ordinary ability (Cycling, Persist, ...), expands the name
//! into a normal `register_triggered_ability`/activated-ability call
//! instead. Without this, every card wanting a standard keyword has to be
//! authored against `KeywordSet` directly rather than just naming the
//! keyword — fine for the hand-built test fixtures this phase has used so
//! far, not fine once real card data flows through here. See
//! `KeywordSet`'s own doc comment for the primitive/sugar split this table
//! would need to encode.

use crate::commands::CommandError;
use crate::types::cost::{FilterSpec, PaymentPlan};
use crate::types::effect::{EffectDuration, Layer};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};

/// `Game.RegisterTriggeredAbility` — called from a card's setup script to
/// register a new ability on the card the script is attached to. Despite
/// the name, also covers activated/static via `kind` on the resulting
/// `RegisteredAbility` — kept one entry point rather than three near-
/// identical ones, since the only real difference is which callback slots
/// are populated.
pub fn register_triggered_ability(
    _state: &mut GameState,
    _source: ObjectId,
    _condition: Option<ScriptHandle>,
    _cost: Option<ScriptHandle>,
    _target: Option<ScriptHandle>,
    _operation: ScriptHandle,
) {
    // TODO: state.abilities.push(RegisteredAbility { source, kind: ...,
    // condition, cost, target, operation }). `kind` isn't a parameter here
    // despite the function registering Triggered/Activated/Static
    // abilities per its own doc comment above - signature needs an
    // AbilityKind param added, or three separate Game.Register* entry
    // points after all (revisit the "one entry point" call made in the
    // doc comment if this turns out awkward from the Lua-author side).
    todo!()
}

/// `Game.AddContinuousEffect`.
pub fn add_continuous_effect(
    _state: &mut GameState,
    _source: ObjectId,
    _layer: Layer,
    _predicate: ScriptHandle,
    _duration: EffectDuration,
) {
    // TODO: state.continuous_effects.push(ContinuousEffect { source, layer,
    // affected_predicate: predicate, timestamp: <current SequenceNo - needs
    // a source for "current", e.g. state.ledger.len() or a dedicated
    // counter, not decided yet>, duration }).
    todo!()
}

/// `Game.ReplaceEvent`.
pub fn replace_event(_state: &mut GameState, _source: ObjectId, _pattern: ScriptHandle) {
    // TODO: state.replacement_effects.push(ReplacementEffect { source,
    // event_pattern: pattern, consumption: <not a parameter here, but
    // ReplacementEffect requires one - same kind of gap as
    // register_triggered_ability's missing AbilityKind param>}). Also note
    // ReplacementEffect has no "what to rewrite the event into" callback
    // field at all yet (see replacement.rs's TODO) - this function can
    // register the matcher but not what it actually does.
    todo!()
}

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

/// `Game.SelectTargets` — same open caveat as `ScriptRuntime::invoke_target`
/// in `script.rs`: a real implementation almost certainly can't return
/// synchronously, since it needs to ask the player and the player's answer
/// arrives as a `PlayerAction` on a later `Engine::step()` call, not within
/// this function call. Kept as a direct `-> Vec<Target>` return for this
/// first signature pass.
pub fn select_targets(
    _state: &GameState,
    _controller: PlayerId,
    _filter: FilterSpec,
    _count: u32,
) -> Vec<Target> {
    // TODO: blocked on the same coroutine design as
    // ScriptRuntime::invoke_target (script.rs) - don't implement
    // synchronously. Once that exists, this presents the filtered legal
    // target set to controller, suspends, and resumes with their choice on
    // a later Engine::step().
    todo!()
}
