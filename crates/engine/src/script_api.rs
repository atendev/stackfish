//! Functions in this file are bound into the Lua global `Game` table — e.g.
//! `register_triggered_ability` below becomes `Game.RegisterTriggeredAbility`
//! — the script-author-facing surface named in DESIGN.md. Each is called
//! *from* a card's Lua setup or operation callback, asking the engine to do
//! something; the engine still decides legality (DESIGN.md: "Lua asks, Rust
//! decides"). The actual `mlua` registration (binding these into the Lua
//! table) isn't written yet — these are the Rust-side signatures that
//! registration will wrap.

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
    state: &mut GameState,
    source: ObjectId,
    condition: Option<ScriptHandle>,
    cost: Option<ScriptHandle>,
    target: Option<ScriptHandle>,
    operation: ScriptHandle,
) {
    todo!()
}

/// `Game.AddContinuousEffect`.
pub fn add_continuous_effect(
    state: &mut GameState,
    source: ObjectId,
    layer: Layer,
    predicate: ScriptHandle,
    duration: EffectDuration,
) {
    todo!()
}

/// `Game.ReplaceEvent`.
pub fn replace_event(state: &mut GameState, source: ObjectId, pattern: ScriptHandle) {
    todo!()
}

/// `Game.CastSpell` — a script requesting the engine cast a spell on a
/// player's behalf (e.g. "you may cast target spell card from exile
/// without paying its mana cost"), not the normal player-initiated path
/// (`PlayerActionKind::CastSpell`). Still goes through the same legality
/// checks; `free` skips the mana cost specifically — other additional
/// costs still apply unless the card's own effect says otherwise.
pub fn cast_spell(state: &mut GameState, card: ObjectId, controller: PlayerId, free: bool) -> Result<(), CommandError> {
    todo!()
}

/// `Game.PayCost`.
pub fn pay_cost(state: &mut GameState, payer: PlayerId, plan: PaymentPlan) -> Result<(), CommandError> {
    todo!()
}

/// `Game.SelectTargets` — same open caveat as `ScriptRuntime::invoke_target`
/// in `script.rs`: a real implementation almost certainly can't return
/// synchronously, since it needs to ask the player and the player's answer
/// arrives as a `PlayerAction` on a later `Engine::step()` call, not within
/// this function call. Kept as a direct `-> Vec<Target>` return for this
/// first signature pass.
pub fn select_targets(
    state: &GameState,
    controller: PlayerId,
    filter: FilterSpec,
    count: u32,
) -> Vec<Target> {
    todo!()
}
