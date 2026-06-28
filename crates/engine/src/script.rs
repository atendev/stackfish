use mlua::Lua;

use crate::types::card::CardDefinition;
use crate::types::event::GameEvent;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};

pub enum ScriptError {
    LoadError(String),
    RuntimeError(String),
}

/// Borrowed view handed to a Lua callback for the duration of one
/// condition/cost/target/operation invocation.
///
/// `state` is currently the full `&GameState` — this is exactly the
/// hidden-information leak surface DESIGN.md flags as an open question (a
/// script could enumerate an opponent's hand off a raw state reference).
/// Needs a visibility-filtered view before this is real; kept as the full
/// state here only to get the call shape down for this pass.
pub struct ScriptContext<'a> {
    pub state: &'a GameState,
    pub source: ObjectId,
    pub controller: PlayerId,
    pub event: Option<&'a GameEvent>,
    pub chosen_targets: Vec<Target>,
}

/// Owns the embedded Lua VM and every loaded card script. One `ScriptRuntime`
/// per running engine process, not per-game — scripts load once and are
/// reused across games, per DESIGN.md's "restart to reload, no hot-reload"
/// decision.
pub struct ScriptRuntime {
    lua: Lua,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        todo!()
    }

    /// Loads `definition.script_ref`'s source into the VM and runs its
    /// setup (the ygopro-style `initial_effect(c)` equivalent — registers
    /// the card's abilities/continuous effects/replacement effects via the
    /// `Game.*` API in `script_api.rs`). Once per `CardDefinition`, not once
    /// per `GameObject` instance of that card.
    pub fn load_card_script(&mut self, definition: &CardDefinition) -> Result<ScriptHandle, ScriptError> {
        todo!()
    }

    pub fn invoke_condition(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<bool, ScriptError> {
        todo!()
    }

    pub fn invoke_cost(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<(), ScriptError> {
        todo!()
    }

    /// Real target selection generally requires suspending mid-script to
    /// ask the player (same caveat as `script_api::select_targets`) — this
    /// signature is almost certainly wrong as a synchronous `-> Vec<Target>`
    /// and will need Lua coroutine support (`Lua::create_thread` plus
    /// yield/resume spanning `Engine::step()` calls) once that's designed.
    /// Kept synchronous for this first pass to get the call shape down.
    pub fn invoke_target(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<Vec<Target>, ScriptError> {
        todo!()
    }

    pub fn invoke_operation(&self, handle: ScriptHandle, ctx: &mut ScriptContext) -> Result<(), ScriptError> {
        todo!()
    }
}
